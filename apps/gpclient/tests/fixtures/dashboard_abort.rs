//! Compiled by the PTY test with -C panic=abort against the production hook.
#[path = "../../src/dashboard/restoration.rs"]
mod restoration;

use std::{fs::{File, OpenOptions}, io::{Read, Write}, os::fd::{AsRawFd, FromRawFd}, sync::Arc};

fn main() {
  let tty = OpenOptions::new().read(true).write(true).open("/dev/tty").unwrap();
  let error = Arc::new(unsafe { File::from_raw_fd(nix::libc::fcntl(2, nix::libc::F_DUPFD_CLOEXEC, 3)) });
  let input_flags = nix::fcntl::FdFlag::from_bits_truncate(unsafe { nix::libc::fcntl(0, nix::libc::F_GETFD) });
  let previous = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic| {
    assert_eq!(unsafe { nix::libc::fcntl(0, nix::libc::F_GETFD) }, input_flags.bits());
    previous(panic);
    eprintln!("ABORT_HOOK_RESTORED");
    let mut acknowledgement = String::new();
    std::io::stdin().read_line(&mut acknowledgement).unwrap();
  }));
  let restore = restoration::TerminalRestoration::install(tty.try_clone().unwrap(), error.clone(), None, input_flags).unwrap();
  restore.arm();
  unsafe { nix::libc::fcntl(0, nix::libc::F_SETFD, input_flags.bits() | nix::libc::FD_CLOEXEC); }
  let mut termios = std::mem::MaybeUninit::uninit();
  unsafe {
    assert_eq!(nix::libc::tcgetattr(tty.as_raw_fd(), termios.as_mut_ptr()), 0);
    let mut raw = termios.assume_init();
    nix::libc::cfmakeraw(&mut raw);
    assert_eq!(nix::libc::tcsetattr(tty.as_raw_fd(), nix::libc::TCSANOW, &raw), 0);
  }
  (&*error).write_all(b"\x1b[?1049hABORT_FIXTURE_READY").unwrap();
  let mut trigger = [0];
  (&tty).read_exact(&mut trigger).unwrap();
  panic!("intentional abort-hook fixture panic");
}
