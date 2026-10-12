use std::{
  io::{self, Read, Write},
  os::unix::process::CommandExt,
  process::{Child, Command, Output, Stdio},
  thread,
  time::{Duration, Instant},
};

use nix::{
  fcntl::{FcntlArg, OFlag, fcntl},
  sys::signal::{Signal, killpg},
  unistd::Pid,
};

use super::collection::CollectionControl;

/// What the child reads on stdin.
pub(crate) enum ChildInput<'a> {
  Null,
  Frame(&'a [u8]),
  /// The caller's own stdin, for a child that prompts the user.
  Terminal,
}

/// Owns and reaps the command's process group. Its async caller must cancel and await this worker.
pub(crate) fn run_controlled(
  command: Command,
  control: &dyn CollectionControl,
  output_limit: usize,
) -> io::Result<Output> {
  run(command, control, Some(output_limit), None, ChildInput::Null).map(|observed| observed.output)
}

#[derive(Debug)]
pub(crate) struct ObservedOutput {
  pub output: Output,
  pub input_error: Option<io::Error>,
}

pub(crate) fn run_observed(
  command: Command,
  control: &dyn CollectionControl,
  output_limit: Option<usize>,
  observer: &mut dyn FnMut(&[u8]),
  stderr_sink: &mut dyn Write,
  stdin: ChildInput<'_>,
) -> io::Result<ObservedOutput> {
  run(command, control, output_limit, Some((observer, stderr_sink)), stdin)
}

fn nonblocking(fd: &impl std::os::fd::AsFd) -> io::Result<()> {
  let flags = fcntl(fd, FcntlArg::F_GETFL).map_err(io::Error::from)?;
  fcntl(
    fd,
    FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
  )
  .map_err(io::Error::from)?;
  Ok(())
}

fn run(
  mut command: Command,
  control: &dyn CollectionControl,
  output_limit: Option<usize>,
  mut observation: Option<(&mut dyn FnMut(&[u8]), &mut dyn Write)>,
  stdin: ChildInput<'_>,
) -> io::Result<ObservedOutput> {
  control.check()?;
  command.stdout(if output_limit.is_some() {
    Stdio::piped()
  } else {
    Stdio::inherit()
  });
  // A child in a process group of its own is a background job, so reading the terminal would stop it with SIGTTIN.
  let own_group = !matches!(stdin, ChildInput::Terminal);
  if own_group {
    command.process_group(0);
  }
  let (stdio, stdin_frame) = match stdin {
    ChildInput::Null => (Stdio::null(), None),
    ChildInput::Frame(frame) => (Stdio::piped(), Some(frame)),
    ChildInput::Terminal => (Stdio::inherit(), None),
  };
  command.stdin(stdio);
  if observation.is_some() {
    command.stderr(Stdio::piped());
  }
  let mut child = ControlledChild {
    process: command.spawn()?,
    own_group,
    stopped: false,
  };
  let mut stdout = child.process.stdout.take();
  if let Some(pipe) = &stdout {
    nonblocking(pipe)?;
  }
  let mut stderr = if observation.is_some() {
    child.process.stderr.take()
  } else {
    None
  };
  if let Some(pipe) = &stderr {
    nonblocking(pipe)?;
  }
  let mut input = child.process.stdin.take();
  if let Some(pipe) = &input {
    nonblocking(pipe)?;
  }
  let mut input_offset = 0;
  let mut input_error = None;
  let mut output = Vec::new();
  let mut buffer = [0u8; 8192];
  let mut stdout_eof = stdout.is_none();
  let mut stderr_eof = stderr.is_none();
  let mut status = None;
  let mut exited_at = None;
  loop {
    control.check()?;
    let mut progressed = false;
    if let (Some(frame), Some(pipe)) = (stdin_frame, &mut input) {
      if input_offset < frame.len() && input_error.is_none() {
        match pipe.write(&frame[input_offset..]) {
          Ok(0) => input_error = Some(io::Error::new(io::ErrorKind::WriteZero, "Credential input pipe closed")),
          Ok(length) => {
            input_offset += length;
            progressed = true;
          }
          Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
          Err(error) => input_error = Some(error),
        }
      }
    }

    if !stdout_eof {
      match stdout.as_mut().unwrap().read(&mut buffer) {
        Ok(0) => stdout_eof = true,
        Ok(length) => {
          if length > output_limit.unwrap().saturating_sub(output.len()) {
            return Err(io::Error::new(
              io::ErrorKind::InvalidData,
              "Command output exceeds its limit",
            ));
          }
          output.extend_from_slice(&buffer[..length]);
          progressed = true;
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
        Err(error) => return Err(error),
      }
    }
    if !stderr_eof {
      match stderr.as_mut().unwrap().read(&mut buffer) {
        Ok(0) => stderr_eof = true,
        Ok(length) => {
          let (observer, sink) = observation.as_mut().unwrap();
          observer(&buffer[..length]);
          sink.write_all(&buffer[..length])?;
          progressed = true;
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
        Err(error) => return Err(error),
      }
    }
    if status.is_none()
      && let Some(exit_status) = child.process.try_wait()?
    {
      status = Some(exit_status);
      exited_at = Some(Instant::now());
      child.stop_helpers();
    }
    if let Some(status) = status {
      if (stdout_eof && stderr_eof) || exited_at.unwrap().elapsed() >= Duration::from_millis(250) {
        if stdin_frame.is_some_and(|frame| input_offset < frame.len()) && input_error.is_none() {
          input_error = Some(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "Child exited before receiving its credential",
          ));
        }
        return Ok(ObservedOutput {
          output: Output {
            status,
            stdout: output,
            stderr: Vec::new(),
          },
          input_error,
        });
      }
    }
    if !progressed {
      thread::sleep(Duration::from_millis(10));
    }
  }
}

struct ControlledChild {
  process: Child,
  own_group: bool,
  stopped: bool,
}

impl ControlledChild {
  fn stop_helpers(&mut self) {
    if !self.stopped {
      if self.own_group {
        let _ = killpg(Pid::from_raw(self.process.id() as i32), Signal::SIGKILL);
      } else {
        let _ = self.process.kill();
      }
      self.stopped = true;
    }
  }
}

impl Drop for ControlledChild {
  fn drop(&mut self) {
    self.stop_helpers();
    let _ = self.process.wait();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn drains_both_pipes_and_reaps_helpers_that_hold_them_open() {
    let mut command = Command::new("sh");
    command.args(["-c", "printf auth; printf diagnostic >&2; sleep 30 & exit 1"]);
    let mut seen = Vec::new();
    let mut sink = Vec::new();
    let start = Instant::now();
    let output = run_observed(
      command,
      &|| Ok(()),
      Some(1024),
      &mut |bytes| seen.extend_from_slice(bytes),
      &mut sink,
      ChildInput::Null,
    )
    .unwrap();
    assert!(!output.output.status.success());
    assert_eq!(output.output.stdout, b"auth");
    assert_eq!(seen, b"diagnostic");
    assert_eq!(sink, seen);
    assert!(start.elapsed() < Duration::from_secs(2));
  }
  #[test]
  fn continuous_stderr_does_not_starve_cancellation_or_stdout() {
    let mut command = Command::new("sh");
    command.args(["-c", "printf auth; while :; do printf 'diagnostic\\n' >&2; done"]);
    let start = Instant::now();
    let control = || {
      if start.elapsed() > Duration::from_millis(100) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
      } else {
        Ok(())
      }
    };
    let error = run_observed(
      command,
      &control,
      Some(1024),
      &mut |_| {},
      &mut io::sink(),
      ChildInput::Null,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(start.elapsed() < Duration::from_secs(2));
  }

  #[test]
  fn retains_broken_input_pipe_for_failed_child_without_a_diagnostic() {
    let mut command = Command::new("sh");
    command.args(["-c", "exec 0<&-; exit 2"]);
    let observed = run_observed(
      command,
      &|| Ok(()),
      None,
      &mut |_| {},
      &mut io::sink(),
      ChildInput::Frame(&vec![0; 128 * 1024]),
    )
    .unwrap();
    assert_eq!(observed.output.status.code(), Some(2));
    assert!(observed.input_error.is_some());
  }

  #[test]
  fn cancellation_interrupts_a_blocked_credential_write() {
    let mut command = Command::new("sh");
    command.args(["-c", "sleep 30"]);
    let start = Instant::now();
    let control = || {
      if start.elapsed() > Duration::from_millis(100) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
      } else {
        Ok(())
      }
    };
    let error = run_observed(
      command,
      &control,
      None,
      &mut |_| {},
      &mut io::sink(),
      ChildInput::Frame(&vec![0; 128 * 1024]),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(start.elapsed() < Duration::from_secs(2));
  }

  #[cfg(target_os = "linux")]
  #[test]
  fn terminal_input_shares_the_callers_stdin_and_process_group() {
    let mut command = Command::new("sh");
    command.args(["-c", "readlink /proc/$$/fd/0; cut -d' ' -f5 /proc/$$/stat"]);
    let observed = run_observed(
      command,
      &|| Ok(()),
      Some(1024),
      &mut |_| {},
      &mut io::sink(),
      ChildInput::Terminal,
    )
    .unwrap();
    let stdout = String::from_utf8(observed.output.stdout).unwrap();
    let stdin = std::fs::read_link("/proc/self/fd/0").unwrap();
    assert_eq!(stdout, format!("{}\n{}\n", stdin.display(), nix::unistd::getpgrp()));
  }

  #[test]
  fn cancellation_kills_a_terminal_child() {
    let mut command = Command::new("sleep");
    command.arg("30");
    let start = Instant::now();
    let control = || {
      if start.elapsed() > Duration::from_millis(100) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
      } else {
        Ok(())
      }
    };
    let error = run_observed(
      command,
      &control,
      None,
      &mut |_| {},
      &mut io::sink(),
      ChildInput::Terminal,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(start.elapsed() < Duration::from_secs(2));
  }
}
