use std::{
  io::{self, Read},
  os::unix::process::CommandExt,
  process::{Child, Command, Output, Stdio},
  thread,
  time::Duration,
};

use nix::{
  fcntl::{FcntlArg, OFlag, fcntl},
  sys::signal::{Signal, killpg},
  unistd::Pid,
};

use super::collection::CollectionControl;

/// Execute an already configured command. Environment, identity and executable
/// trust belong to its caller. This worker owns the process group until reaped;
/// its asynchronous caller must cancel and await it instead of dropping it.
pub(crate) fn run_controlled(
  mut command: Command,
  control: &dyn CollectionControl,
  output_limit: usize,
) -> io::Result<Output> {
  control.check()?;
  let child = command
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .process_group(0)
    .spawn()?;
  let mut child = ControlledChild(child);
  let mut stdout = child.0.stdout.take().expect("controlled stdout was piped");
  let flags = fcntl(&stdout, FcntlArg::F_GETFL).map_err(io::Error::from)?;
  fcntl(
    &stdout,
    FcntlArg::F_SETFL(OFlag::from_bits_truncate(flags) | OFlag::O_NONBLOCK),
  )
  .map_err(io::Error::from)?;
  let mut output = Vec::new();
  let mut buffer = [0u8; 8192];
  let mut eof = false;
  let mut status = None;
  loop {
    control.check()?;
    if !eof {
      match stdout.read(&mut buffer) {
        Ok(0) => eof = true,
        Ok(length) => {
          if length > output_limit.saturating_sub(output.len()) {
            return Err(io::Error::new(
              io::ErrorKind::InvalidData,
              "Command output exceeds its limit",
            ));
          }
          output.extend_from_slice(&buffer[..length]);
          continue;
        }
        Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted) => {}
        Err(error) => return Err(error),
      }
    }
    if status.is_none() {
      status = child.0.try_wait()?;
    }
    if eof && let Some(status) = status {
      return Ok(Output {
        status,
        stdout: output,
        stderr: Vec::new(),
      });
    }
    thread::sleep(Duration::from_millis(10));
  }
}

struct ControlledChild(Child);

impl Drop for ControlledChild {
  fn drop(&mut self) {
    // Reap the direct child and stop helpers retaining stdout after it exits.
    let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
    let _ = self.0.wait();
  }
}
