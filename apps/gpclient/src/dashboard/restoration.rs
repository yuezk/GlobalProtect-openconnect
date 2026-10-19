//! Panic restoration must remain independent of renderer and output-ingestion locks.
use std::{
  fs::File,
  io,
  os::fd::AsRawFd,
  sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
  },
};

pub(super) struct TerminalRestoration {
  terminal: File,
  error: Arc<File>,
  output: Option<Arc<File>>,
  input_flags: nix::fcntl::FdFlag,
  termios: nix::libc::termios,
  active: AtomicBool,
}

impl TerminalRestoration {
  pub(super) fn install(
    terminal: File,
    error: Arc<File>,
    output: Option<Arc<File>>,
    input_flags: nix::fcntl::FdFlag,
  ) -> io::Result<Arc<Self>> {
    let mut termios = std::mem::MaybeUninit::uninit();
    if unsafe { nix::libc::tcgetattr(terminal.as_raw_fd(), termios.as_mut_ptr()) } != 0 {
      return Err(io::Error::last_os_error());
    }
    let restoration = Arc::new(Self {
      terminal,
      error,
      output,
      input_flags,
      termios: unsafe { termios.assume_init() },
      active: AtomicBool::new(false),
    });
    let previous = std::panic::take_hook();
    let panic_restoration = restoration.clone();
    std::panic::set_hook(Box::new(move |panic| {
      panic_restoration.restore();
      previous(panic);
    }));
    Ok(restoration)
  }

  pub(super) fn arm(&self) {
    self.active.store(true, Ordering::Release);
  }
  pub(super) fn disarm(&self) {
    self.active.store(false, Ordering::Release);
  }

  pub(super) fn restore(&self) {
    if !self.active.swap(false, Ordering::AcqRel) {
      return;
    }
    unsafe {
      nix::libc::tcsetattr(self.terminal.as_raw_fd(), nix::libc::TCSANOW, &self.termios);
      let sequence = b"\x1b[?25h\x1b[?1049l";
      nix::libc::write(self.error.as_raw_fd(), sequence.as_ptr().cast(), sequence.len());
      nix::libc::dup2(self.error.as_raw_fd(), 2);
      nix::libc::fcntl(0, nix::libc::F_SETFD, self.input_flags.bits());
      if let Some(output) = &self.output {
        nix::libc::dup2(output.as_raw_fd(), 1);
      }
    }
  }
}
