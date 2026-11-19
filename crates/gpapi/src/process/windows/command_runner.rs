//! Controlled Windows child execution. Pipe polling keeps cancellation responsive.
use super::{
  auth_bootstrap::AuthBootstrap,
  console_input::{self, ConsoleInput},
  desktop_auth::DesktopChild,
};
use crate::collection::CollectionControl;
use std::{
  fs::File,
  io::{self, Read, Write},
  os::windows::io::{AsRawHandle, OwnedHandle},
  process::{Child, Command, ExitStatus, Output, Stdio},
  thread,
  time::Duration,
};
use windows_sys::Win32::System::Pipes::PeekNamedPipe;

pub(crate) enum ChildInput<'a> {
  Null,
  Terminal,
  // Preserve the borrowed input type shared by callers without a Windows launcher.
  #[allow(dead_code)]
  Frame(&'a [u8]),
}

#[derive(Debug)]
pub(crate) struct ObservedOutput {
  pub output: Output,
}

pub(crate) fn run_controlled(command: Command, control: &dyn CollectionControl, limit: usize) -> io::Result<Output> {
  run_observed(
    command,
    control,
    Some(limit),
    &mut |_| {},
    &mut io::sink(),
    ChildInput::Null,
    false,
  )
  .map(|observed| observed.output)
}

pub(crate) fn run_observed(
  command: Command,
  control: &dyn CollectionControl,
  output_limit: Option<usize>,
  observer: &mut dyn FnMut(&[u8]),
  stderr_sink: &mut dyn Write,
  stdin: ChildInput<'_>,
  windows_cli_auth: bool,
) -> io::Result<ObservedOutput> {
  control.check()?;
  let bootstrap = windows_cli_auth.then(|| AuthBootstrap::prepare(&command)).transpose()?;
  let (mut child, mut stdout, mut stderr, input) = match bootstrap {
    Some(bootstrap) if crate::windows::is_elevated()? => {
      let (child, stdout, stderr, input) = DesktopChild::spawn(&command, bootstrap, &stdin, control)?;
      (ControlledChild::Desktop(child), Some(stdout), stderr, input)
    }
    bootstrap => spawn_command(command, &stdin, output_limit, bootstrap, control)?,
  };
  let input = if let (ChildInput::Frame(frame), Some(mut pipe)) = (stdin, input) {
    let frame = frame.to_vec();
    Some(thread::spawn(move || pipe.write_all(&frame)))
  } else {
    None
  };
  let mut output = Vec::new();
  let mut buffer = [0u8; 8192];
  loop {
    control.check()?;
    if let Some(pipe) = &mut stdout {
      let length = read_available(pipe, &mut buffer)?;
      if length > output_limit.unwrap().saturating_sub(output.len()) {
        return Err(io::Error::new(
          io::ErrorKind::InvalidData,
          "Command output exceeds its limit",
        ));
      }
      output.extend_from_slice(&buffer[..length]);
    }
    let length = read_available(&mut stderr, &mut buffer)?;
    observer(&buffer[..length]);
    stderr_sink.write_all(&buffer[..length])?;
    if let Some(status) = child.try_wait()? {
      // Drain the remaining buffered output after the process exits.
      if let Some(pipe) = &mut stdout {
        loop {
          let length = read_available(pipe, &mut buffer)?;
          if length == 0 {
            break;
          }
          if length > output_limit.unwrap().saturating_sub(output.len()) {
            return Err(io::Error::new(
              io::ErrorKind::InvalidData,
              "Command output exceeds its limit",
            ));
          }
          output.extend_from_slice(&buffer[..length]);
        }
      }
      loop {
        let length = read_available(&mut stderr, &mut buffer)?;
        if length == 0 {
          break;
        }
        observer(&buffer[..length]);
        stderr_sink.write_all(&buffer[..length])?;
      }
      if let Some(result) = input.and_then(|worker| worker.join().ok()) {
        result?;
      }
      return Ok(ObservedOutput {
        output: Output {
          status,
          stdout: output,
          stderr: Vec::new(),
        },
      });
    }
    thread::sleep(Duration::from_millis(10));
  }
}

fn spawn_command(
  command: Command,
  stdin: &ChildInput<'_>,
  output_limit: Option<usize>,
  bootstrap: Option<AuthBootstrap>,
  control: &dyn CollectionControl,
) -> io::Result<(ControlledChild, Option<File>, File, Option<File>)> {
  let mut command = match &bootstrap {
    Some(bootstrap) => bootstrap.command(&command),
    None => command,
  };
  // Windows desktop processes must not open a console for each helper.
  use std::os::windows::process::CommandExt;
  if !matches!(stdin, ChildInput::Terminal) {
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
  }
  command.stdout(if output_limit.is_some() {
    Stdio::piped()
  } else {
    Stdio::inherit()
  });
  command.stderr(Stdio::piped());
  let terminal = if bootstrap.is_some() && matches!(stdin, ChildInput::Terminal) {
    let input = console_input::terminal_input()?;
    console_input::is_console(&input).then_some(input)
  } else {
    None
  };
  command.stdin(match stdin {
    ChildInput::Terminal if terminal.is_none() => Stdio::inherit(),
    ChildInput::Terminal => Stdio::piped(),
    ChildInput::Null => Stdio::null(),
    ChildInput::Frame(_) => Stdio::piped(),
  });
  let mut child = StandardChild {
    child: command.spawn()?,
    console_input: None,
  };
  if let Some(bootstrap) = bootstrap {
    bootstrap.send_to_child(child.child.id(), control)?;
  }
  let stdout = child
    .child
    .stdout
    .take()
    .map(|pipe| File::from(OwnedHandle::from(pipe)));
  let stderr = File::from(OwnedHandle::from(child.child.stderr.take().expect("piped stderr")));
  let mut input = child.child.stdin.take().map(|pipe| File::from(OwnedHandle::from(pipe)));
  if let Some(terminal) = terminal {
    child.console_input = Some(ConsoleInput::start(terminal, input.take().expect("piped stdin"))?);
  }
  Ok((ControlledChild::Standard(child), stdout, stderr, input))
}

fn read_available(pipe: &mut (impl Read + AsRawHandle), buffer: &mut [u8]) -> io::Result<usize> {
  let mut available = 0;
  let success = unsafe {
    PeekNamedPipe(
      pipe.as_raw_handle(),
      std::ptr::null_mut(),
      0,
      std::ptr::null_mut(),
      &mut available,
      std::ptr::null_mut(),
    )
  };
  if success == 0 {
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(109) {
      return Ok(0);
    } // ERROR_BROKEN_PIPE
    return Err(error);
  }
  let length = buffer.len().min(available as usize);
  if length == 0 {
    return Ok(0);
  }
  pipe.read(&mut buffer[..length])
}

enum ControlledChild {
  Standard(StandardChild),
  Desktop(DesktopChild),
}

impl ControlledChild {
  fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
    match self {
      Self::Standard(child) => {
        if let Some(input) = &mut child.console_input {
          input.check()?;
        }
        child.child.try_wait()
      }
      Self::Desktop(child) => child.try_wait(),
    }
  }
}

struct StandardChild {
  child: Child,
  console_input: Option<ConsoleInput>,
}

impl Drop for StandardChild {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn captures_output_and_rejects_excess_output() {
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/C", "echo hello"]);
    let result = run_controlled(command, &|| Ok(()), 1024).unwrap();
    assert!(result.status.success());
    assert_eq!(String::from_utf8(result.stdout).unwrap().trim(), "hello");
    let mut command = Command::new("cmd.exe");
    command.args(["/D", "/C", "echo too much"]);
    assert_eq!(
      run_controlled(command, &|| Ok(()), 2).unwrap_err().kind(),
      io::ErrorKind::InvalidData
    );
  }
  #[test]
  fn cancels_a_running_helper() {
    let start = std::time::Instant::now();
    let mut command = Command::new("powershell.exe");
    command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
    let control = || {
      if start.elapsed() > Duration::from_millis(150) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
      } else {
        Ok(())
      }
    };
    assert_eq!(
      run_controlled(command, &control, 1024).unwrap_err().kind(),
      io::ErrorKind::Interrupted
    );
    assert!(start.elapsed() < Duration::from_secs(3));
  }
}
