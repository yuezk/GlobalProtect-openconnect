//! Own terminal output while the native worker and its children are running.
use std::{
  collections::VecDeque,
  fs::File,
  io::{self, Read, Write},
  os::fd::{AsFd, FromRawFd},
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  thread::{self, JoinHandle},
  time::{Duration, Instant},
};

use anyhow::Context;
use nix::{
  fcntl::{FcntlArg, FdFlag, OFlag, fcntl},
  poll::{PollFd, PollFlags, poll},
  unistd::{dup2_stderr, dup2_stdout},
};
use tokio_util::sync::CancellationToken;

const MAX_LINES: usize = 2_000;
const MAX_BYTES: usize = 1_048_576;
const MAX_LINE_BYTES: usize = 8_192;

#[derive(Default)]
pub(super) struct History {
  lines: VecDeque<String>,
  bytes: usize,
  discarded: u64,
  partial: [String; 2],
}

impl History {
  fn push(&mut self, line: String) {
    self.bytes += line.len();
    self.lines.push_back(line);
    while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
      if let Some(line) = self.lines.pop_front() {
        self.bytes -= line.len();
        self.discarded = self.discarded.saturating_add(1);
      }
    }
  }

  pub(super) fn tail(&self, count: usize) -> Vec<String> {
    if count == 0 {
      return Vec::new();
    }
    let mut lines = Vec::new();
    if self.discarded > 0 {
      lines.push(format!("[{} older log lines discarded]", self.discarded));
    }
    let partial_count = self.partial.iter().filter(|line| !line.is_empty()).count();
    let retained_count = count.saturating_sub(lines.len() + partial_count);
    lines.extend(
      self
        .lines
        .iter()
        .skip(self.lines.len().saturating_sub(retained_count))
        .cloned(),
    );
    lines.extend(self.partial.iter().filter(|line| !line.is_empty()).cloned());
    lines.truncate(count);
    lines
  }
}

pub(super) fn plain_text(bytes: &[u8]) -> String {
  String::from_utf8_lossy(&strip_ansi_escapes::strip(bytes))
    .chars()
    .filter(|ch| !ch.is_control() || *ch == '\t')
    .collect()
}

struct LineDecoder {
  bytes: Vec<u8>,
  truncated: bool,
  stream: usize,
}

impl LineDecoder {
  fn new(stream: usize) -> Self {
    Self {
      bytes: Vec::new(),
      truncated: false,
      stream,
    }
  }

  fn text(&self) -> String {
    let suffix = if self.truncated { " [line truncated]" } else { "" };
    format!("{}{suffix}", plain_text(&self.bytes))
  }

  fn ingest(&mut self, bytes: &[u8], history: &mut History) {
    for byte in bytes {
      if *byte == b'\n' {
        history.push(self.text());
        self.bytes.clear();
        self.truncated = false;
      } else if self.bytes.len() < MAX_LINE_BYTES {
        self.bytes.push(*byte);
      } else {
        self.truncated = true;
      }
    }
    history.partial[self.stream] = if self.bytes.is_empty() && !self.truncated {
      String::new()
    } else {
      self.text()
    };
  }

  fn finish(&mut self, history: &mut History) {
    if !self.bytes.is_empty() || self.truncated {
      history.push(self.text());
    }
    history.partial[self.stream].clear();
  }
}

/// The duplicate is close-on-exec: helpers inherit the capture pipe, never the renderer.
pub(super) fn duplicate(fd: i32) -> io::Result<File> {
  let duplicate = unsafe { nix::libc::fcntl(fd, nix::libc::F_DUPFD_CLOEXEC, 3) };
  if duplicate < 0 {
    return Err(io::Error::last_os_error());
  }
  Ok(unsafe { File::from_raw_fd(duplicate) })
}

pub(super) struct OutputRouting {
  pub(super) original_error: Arc<File>,
  original_input_flags: FdFlag,
  original_output: Option<Arc<File>>,
  history: Arc<Mutex<History>>,
  passthrough: Arc<AtomicBool>,
  stop: Arc<AtomicBool>,
  readers: Vec<JoinHandle<()>>,
  gate: Arc<Mutex<()>>,
  error: Arc<Mutex<Option<String>>>,
  restored: bool,
}

impl OutputRouting {
  pub(super) fn install(capture_stdout: bool, cancellation: CancellationToken) -> anyhow::Result<Self> {
    io::stdout().flush()?;
    io::stderr().flush()?;
    unsafe {
      nix::libc::fflush(std::ptr::null_mut());
    }
    let mut routing = Self {
      original_error: Arc::new(duplicate(2)?),
      original_input_flags: FdFlag::from_bits_truncate(fcntl(io::stdin(), FcntlArg::F_GETFD)?),
      original_output: capture_stdout.then(|| duplicate(1).map(Arc::new)).transpose()?,
      history: Arc::default(),
      passthrough: Arc::new(AtomicBool::new(true)),
      stop: Arc::default(),
      readers: Vec::new(),
      gate: Arc::default(),
      error: Arc::default(),
      restored: false,
    };
    routing.capture(1, routing.original_error.clone(), cancellation.clone())?;
    if let Some(original) = routing.original_output.clone() {
      routing.capture(0, original, cancellation)?;
    }
    // Authentication may have initialized Crossterm's global reader with fd 0.
    // Keep that descriptor stable, but do not let executed helpers inherit it.
    // Script-TUN packet I/O continues through its separate VPNFD socket.
    fcntl(
      io::stdin(),
      FcntlArg::F_SETFD(routing.original_input_flags | FdFlag::FD_CLOEXEC),
    )?;
    Ok(routing)
  }

  fn capture(&mut self, stream: usize, destination: Arc<File>, cancellation: CancellationToken) -> anyhow::Result<()> {
    let (reader, writer) = io::pipe()?;
    // Polling plus nonblocking reads lets teardown bound inherited pipe lifetimes.
    fcntl(&reader, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
    let history = self.history.clone();
    let passthrough = self.passthrough.clone();
    let stop = self.stop.clone();
    let gate = self.gate.clone();
    let error = self.error.clone();
    let thread = thread::Builder::new()
      .name(format!("dashboard-output-{stream}"))
      .spawn(move || {
        let mut reader = reader;
        let mut decoder = LineDecoder::new(stream);
        let mut buffer = [0; 4096];
        let mut deadline = None;
        loop {
          if stop.load(Ordering::Acquire) && deadline.is_none() {
            deadline = Some(Instant::now() + Duration::from_millis(250));
          }
          if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break;
          }
          let mut descriptors = [PollFd::new(reader.as_fd(), PollFlags::POLLIN)];
          match poll(&mut descriptors, 50u16) {
            Ok(0) => {
              if deadline.is_some() {
                break;
              }
              continue;
            }
            Ok(_) => {}
            Err(nix::errno::Errno::EINTR) => continue,
            Err(failure) => {
              *error.lock().unwrap_or_else(|error| error.into_inner()) =
                Some(format!("Cannot poll dashboard output: {failure}"));
              cancellation.cancel();
              break;
            }
          }
          match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(length) => {
              {
                let _gate = gate.lock().unwrap_or_else(|error| error.into_inner());
                if passthrough.load(Ordering::Acquire) {
                  if let Err(failure) = (&*destination).write_all(&buffer[..length]) {
                    *error.lock().unwrap_or_else(|error| error.into_inner()) =
                      Some(format!("Cannot forward dashboard output: {failure}"));
                    cancellation.cancel();
                  }
                }
              }
              decoder.ingest(
                &buffer[..length],
                &mut history.lock().unwrap_or_else(|error| error.into_inner()),
              );
            }
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => continue,
            Err(failure) => {
              *error.lock().unwrap_or_else(|error| error.into_inner()) =
                Some(format!("Cannot read dashboard output: {failure}"));
              cancellation.cancel();
              break;
            }
          }
        }
        decoder.finish(&mut history.lock().unwrap_or_else(|error| error.into_inner()));
      })
      .context("Cannot start dashboard output reader")?;
    self.readers.push(thread);
    if stream == 0 {
      dup2_stdout(&writer)?;
    } else {
      dup2_stderr(&writer)?;
    }
    Ok(())
  }

  pub(super) fn history(&self) -> Arc<Mutex<History>> {
    self.history.clone()
  }
  pub(super) fn passthrough(&self) -> Arc<AtomicBool> {
    self.passthrough.clone()
  }
  pub(super) fn gate(&self) -> Arc<Mutex<()>> {
    self.gate.clone()
  }
  pub(super) fn saved_output(&self) -> Option<Arc<File>> {
    self.original_output.clone()
  }
  pub(super) fn saved_input_flags(&self) -> FdFlag {
    self.original_input_flags
  }
  pub(super) fn take_error(&self) -> Option<String> {
    self.error.lock().unwrap_or_else(|error| error.into_inner()).take()
  }

  pub(super) fn restore(&mut self) {
    if self.restored {
      return;
    }
    let _ = io::stdout().flush();
    let _ = io::stderr().flush();
    unsafe {
      nix::libc::fflush(std::ptr::null_mut());
    }
    let _ = dup2_stderr(&*self.original_error);
    let _ = fcntl(io::stdin(), FcntlArg::F_SETFD(self.original_input_flags));
    if let Some(output) = &self.original_output {
      let _ = dup2_stdout(&**output);
    }
    self.stop.store(true, Ordering::Release);
    for reader in self.readers.drain(..) {
      let _ = reader.join();
    }
    self.restored = true;
  }
}

impl Drop for OutputRouting {
  fn drop(&mut self) {
    self.restore();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn split_utf8_and_unterminated_lines_are_preserved_without_escape_commands() {
    for stream in [0, 1] {
      let mut history = History::default();
      let mut decoder = LineDecoder::new(stream);
      let bytes = "connected 日本語\x1b[2J\npartial".as_bytes();
      for byte in bytes {
        decoder.ingest(&[*byte], &mut history);
      }
      assert_eq!(history.tail(10), ["connected 日本語", "partial"]);
      decoder.finish(&mut history);
      assert_eq!(history.tail(10), ["connected 日本語", "partial"]);
    }
  }

  #[test]
  fn hostile_long_lines_and_history_are_bounded() {
    let mut history = History::default();
    let mut decoder = LineDecoder::new(0);
    decoder.ingest(&vec![b'x'; MAX_LINE_BYTES * 3], &mut history);
    decoder.ingest(b"\n", &mut history);
    assert!(history.lines[0].ends_with("[line truncated]"));
    for _ in 0..MAX_LINES + 10 {
      decoder.ingest(b"line\n", &mut history);
    }
    assert!(history.lines.len() <= MAX_LINES);
    assert!(history.bytes <= MAX_BYTES);
    assert!(history.tail(10)[0].contains("discarded"));
  }
}
