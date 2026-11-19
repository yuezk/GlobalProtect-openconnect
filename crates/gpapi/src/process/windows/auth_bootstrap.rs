//! One-shot launch arguments for the installed Windows CLI authenticator.
use std::{env, ffi::OsStr, io, os::windows::io::AsRawHandle, process::Command, time::Duration};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::windows::named_pipe::{ClientOptions, NamedPipeServer},
  time,
};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};

use super::auth_launcher::AUTH_PARENT_PROCESS_ID_ENV;
use crate::collection::CollectionControl;

const FLAG: &str = "--windows-cli-bootstrap";
const MAX_BYTES: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15);
const RECEIVED: u8 = 1;

pub(super) struct AuthBootstrap {
  name: String,
  pipe: NamedPipeServer,
  request: Vec<u8>,
}

impl AuthBootstrap {
  pub(super) fn prepare(command: &Command) -> io::Result<Self> {
    let args: Vec<_> = command
      .get_args()
      .map(|arg| {
        arg
          .to_str()
          .ok_or_else(|| invalid("Invalid Windows authentication argument"))
      })
      .collect::<io::Result<_>>()?;
    let request = serde_json::to_vec(&args)?;
    check_length(request.len())?;
    let name = format!(r"\\.\pipe\GPConnect.Auth.{}", uuid::Uuid::new_v4());
    let pipe = crate::windows::create_user_pipe(&name, true)?;
    Ok(Self { name, pipe, request })
  }

  pub(super) fn arguments(&self) -> [&OsStr; 2] {
    [OsStr::new(FLAG), OsStr::new(&self.name)]
  }

  /// Build the short command used when the CLI and authenticator share a token.
  pub(super) fn command(&self, original: &Command) -> Command {
    let mut command = Command::new(original.get_program());
    command.args(self.arguments());
    for (name, value) in original.get_envs() {
      match value {
        Some(value) => command.env(name, value),
        None => command.env_remove(name),
      };
    }
    if let Some(directory) = original.get_current_dir() {
      command.current_dir(directory);
    }
    command
  }

  pub(super) fn send_to_child(self, pid: u32, control: &dyn CollectionControl) -> io::Result<()> {
    tokio::runtime::Handle::current().block_on(self.send(pid, control))
  }

  async fn send(mut self, expected_pid: u32, control: &dyn CollectionControl) -> io::Result<()> {
    let send = async {
      self.pipe.connect().await?;
      let mut pid = 0;
      if unsafe { GetNamedPipeClientProcessId(self.pipe.as_raw_handle(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
      }
      if pid != expected_pid {
        return Err(io::Error::new(
          io::ErrorKind::PermissionDenied,
          "Unexpected authentication pipe client",
        ));
      }
      self.pipe.write_u32(self.request.len() as u32).await?;
      self.pipe.write_all(&self.request).await?;
      if self.pipe.read_u8().await? != RECEIVED {
        return Err(invalid("Windows authentication arguments were not accepted"));
      }
      Ok(())
    };
    let check = async {
      loop {
        control.check()?;
        time::sleep(Duration::from_millis(25)).await;
      }
    };
    time::timeout(TIMEOUT, async {
      tokio::select! {
        result = send => result,
        result = check => result,
      }
    })
    .await?
  }
}

/// Recognize the internal bootstrap before handing actual arguments to the normal parser.
pub async fn receive_arguments() -> io::Result<Option<Vec<String>>> {
  let mut args = env::args_os().skip(1);
  if args.next().as_deref() != Some(OsStr::new(FLAG)) {
    return Ok(None);
  }
  let name = args
    .next()
    .ok_or_else(|| invalid("Missing Windows authentication pipe"))?;
  if args.next().is_some() {
    return Err(invalid("Unexpected Windows authentication bootstrap argument"));
  }
  let expected_pid: u32 = env::var(AUTH_PARENT_PROCESS_ID_ENV)
    .ok()
    .and_then(|value| value.parse().ok())
    .filter(|pid| *pid != 0)
    .ok_or_else(|| invalid("Missing Windows authentication parent process"))?;
  receive(&name, expected_pid).await.map(Some)
}

async fn receive(name: &OsStr, expected_pid: u32) -> io::Result<Vec<String>> {
  time::timeout(TIMEOUT, async {
    let mut pipe = ClientOptions::new().open(name)?;
    let mut pid = 0;
    if unsafe { GetNamedPipeServerProcessId(pipe.as_raw_handle(), &mut pid) } == 0 {
      return Err(io::Error::last_os_error());
    }
    if pid != expected_pid {
      return Err(io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Unexpected authentication pipe server",
      ));
    }
    let length = pipe.read_u32().await? as usize;
    check_length(length)?;
    let mut request = vec![0; length];
    pipe.read_exact(&mut request).await?;
    let args = serde_json::from_slice(&request)?;
    pipe.write_u8(RECEIVED).await?;
    Ok(args)
  })
  .await?
}

fn check_length(length: usize) -> io::Result<()> {
  if length == 0 || length > MAX_BYTES {
    return Err(invalid("Windows authentication launch arguments exceed their limit"));
  }
  Ok(())
}

fn invalid(message: &str) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn large_saml_request_round_trips_outside_the_os_command_line() {
    let request = format!("<html>{}</html>", "🎵".repeat(20_000));
    let mut command = Command::new("gpauth.exe");
    command.args(["portal.example.test", "--saml-request", &request]);
    let bootstrap = AuthBootstrap::prepare(&command).unwrap();
    assert!(request.encode_utf16().count() > 32_767);
    let launch = bootstrap.command(&command);
    assert_eq!(launch.get_args().collect::<Vec<_>>(), bootstrap.arguments());
    let name = bootstrap.name.clone();
    let pid = std::process::id();
    let (sent, received) = tokio::join!(bootstrap.send(pid, &|| Ok(())), receive(OsStr::new(&name), pid));
    sent.unwrap();
    assert_eq!(received.unwrap(), ["portal.example.test", "--saml-request", &request]);
  }
}
