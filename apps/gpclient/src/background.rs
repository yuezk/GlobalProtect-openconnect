//! Re-exec a worker in the foreground for sign-in, then detach only after tunnel setup.
//! No process is forked after authentication or OpenConnect has started its threads.

use std::{
  ffi::OsString,
  fs::{File, OpenOptions},
  io::{self, Write},
  os::{
    fd::{AsRawFd, FromRawFd, OwnedFd},
    unix::{
      fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
      net::UnixStream,
    },
  },
  path::Path,
  process::{ExitStatus, Stdio},
};

use anyhow::{Context, bail};
use nix::{
  fcntl::{FcntlArg, FdFlag, OFlag, fcntl},
  sys::signal::{Signal, kill},
  unistd::{Pid, dup2_stderr, dup2_stdin, dup2_stdout, setsid},
};
use tokio::{io::AsyncReadExt, process::Command};

const READY: u8 = 1;

pub(crate) struct BackgroundWorker {
  ready: UnixStream,
  log: File,
  input: File,
}

// Keep sign-in interactive again if a tunnel attempt fails before detachment.
pub(crate) struct BackgroundStdio {
  saved: Option<[OwnedFd; 3]>,
}

impl Drop for BackgroundStdio {
  fn drop(&mut self) {
    if let Some([input, output, error]) = &self.saved {
      let _ = io::stdout().flush();
      let _ = io::stderr().flush();
      let result = dup2_stdin(input)
        .and_then(|_| dup2_stdout(output))
        .and_then(|_| dup2_stderr(error));
      if let Err(error) = result {
        log::warn!("Cannot restore foreground standard streams: {error}");
      }
    }
  }
}

impl BackgroundWorker {
  pub(crate) fn new(fd: i32, log_path: &Path) -> anyhow::Result<Self> {
    anyhow::ensure!(fd >= 3, "Invalid background readiness descriptor");
    // Validate the raw descriptor before taking ownership; malformed internal args
    // must return an error rather than violate OwnedFd's validity requirement.
    if unsafe { nix::libc::fcntl(fd, nix::libc::F_GETFD) } == -1 {
      return Err(io::Error::last_os_error().into());
    }
    // The launcher passes its own readiness socket across exec, without authentication state.
    let ready = unsafe { UnixStream::from_raw_fd(fd) };
    ready.peer_addr().context("Invalid background readiness socket")?;
    fcntl(&ready, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    let log = OpenOptions::new()
      .create(true)
      .append(true)
      .mode(0o600)
      .custom_flags(OFlag::O_NOFOLLOW.bits() | OFlag::O_NONBLOCK.bits())
      .open(log_path)
      .with_context(|| format!("Cannot open background log {}", log_path.display()))?;
    let metadata = log.metadata()?;
    anyhow::ensure!(metadata.is_file(), "Background log must be a regular file");
    anyhow::ensure!(
      metadata.uid() == uzers::get_effective_uid(),
      "Background log must be owned by the user running gpclient"
    );
    anyhow::ensure!(
      metadata.permissions().mode() & 0o077 == 0,
      "Background log must not be readable or writable by other users"
    );
    let input = File::open("/dev/null")?;
    Ok(Self { ready, log, input })
  }

  pub(crate) fn redirect_stdio(&self) -> anyhow::Result<BackgroundStdio> {
    io::stdout().flush()?;
    io::stderr().flush()?;
    let save = |fd| -> anyhow::Result<OwnedFd> {
      // Duplicate atomically with close-on-exec so helpers never inherit the terminal copies.
      let saved = unsafe { nix::libc::fcntl(fd, nix::libc::F_DUPFD_CLOEXEC, 3) };
      if saved == -1 {
        return Err(io::Error::last_os_error().into());
      }
      Ok(unsafe { OwnedFd::from_raw_fd(saved) })
    };
    let stdio = BackgroundStdio {
      saved: Some([save(0)?, save(1)?, save(2)?]),
    };
    // Redirect before tunnel setup, which can start a persistent --script-tun helper.
    dup2_stdin(&self.input)?;
    dup2_stdout(&self.log)?;
    dup2_stderr(&self.log)?;
    Ok(stdio)
  }

  pub(crate) fn detach(&self, stdio: &mut BackgroundStdio) -> anyhow::Result<()> {
    // The worker inherited the launcher's foreground group, so it can prompt normally.
    // It becomes a session leader only when sign-in and tunnel configuration are done.
    setsid().context("Cannot detach the VPN session from the terminal")?;
    (&self.ready)
      .write_all(&[READY])
      .context("Background launcher exited before tunnel setup")?;
    stdio.saved.take();
    Ok(())
  }
}

pub(crate) async fn start(log_path: &Path, format: gpapi::log_format::LogFormat) -> anyhow::Result<()> {
  let command = Command::new(std::env::current_exe()?);
  let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
  launch(command, &arguments)
    .await
    .with_context(|| format!("Background VPN startup failed; log file: {}", log_path.display()))?;
  gpapi::clap::report(
    format,
    log::Level::Info,
    &format!("VPN is running in the background. Logs: {}", log_path.display()),
  );
  Ok(())
}

async fn launch(mut command: Command, arguments: &[OsString]) -> anyhow::Result<()> {
  let (parent, worker) = UnixStream::pair()?;
  parent.set_nonblocking(true)?;
  let mut ready = tokio::net::UnixStream::from_std(parent)?;
  let fd = worker.as_raw_fd();
  command.arg("--background-worker-fd").arg(fd.to_string());
  // Internal options precede user arguments, which can include the `--` terminator.
  command.args(arguments);
  command
    .stdin(Stdio::inherit())
    .stdout(Stdio::inherit())
    .stderr(Stdio::inherit());
  // Only async-signal-safe descriptor setup runs between spawn's fork and exec.
  unsafe {
    command.pre_exec(move || {
      fcntl(&worker, FcntlArg::F_SETFD(FdFlag::empty())).map_err(io::Error::from)?;
      Ok(())
    });
  }
  let mut child = command.spawn().context("Cannot start background VPN worker")?;
  // Release our copy of the worker's socket so EOF reports startup failure.
  drop(command);
  let pid = Pid::from_raw(child.id().context("Background worker has no PID")? as i32);
  let outcome = tokio::select! {
    result = ready.read_u8() => result.map_err(anyhow::Error::from),
    status = child.wait() => return startup_exit(status?),
    _ = gpapi::utils::shutdown_signal() => {
      let _ = kill(pid, Signal::SIGINT);
      child.wait().await?;
      bail!("Background connection cancelled");
    }
  };
  match outcome {
    Ok(READY) => Ok(()),
    _ => {
      let _ = kill(pid, Signal::SIGINT);
      startup_exit(child.wait().await?)
    }
  }
}

fn startup_exit(status: ExitStatus) -> anyhow::Result<()> {
  bail!("Background VPN exited before the tunnel was established ({status})")
}

#[cfg(test)]
mod tests {
  use super::*;
  use nix::unistd::{getpid, getsid};
  use std::time::Duration;

  // Re-exec the test binary so the test exercises real process/session/descriptor setup.
  // The shell consumes launch's internal FD argument instead of giving it to libtest.
  fn fixture(directory: &Path) -> Command {
    let mut command = Command::new("sh");
    command.args(["-c", "export GPCLIENT_TEST_FD=\"$2\"; exec \"$GPCLIENT_TEST_EXE\" --exact background::tests::worker_fixture --nocapture", "fixture"])
      .env("GPCLIENT_TEST_EXE", std::env::current_exe().unwrap())
      .env("GPCLIENT_TEST_DIR", directory);
    command
  }

  #[tokio::test]
  async fn startup_failure_is_not_reported_as_a_background_connection() {
    let mut command = Command::new("sh");
    command.args(["-c", "exit 7"]);
    let error = tokio::time::timeout(Duration::from_secs(5), launch(command, &[]))
      .await
      .unwrap()
      .unwrap_err();
    assert!(error.to_string().contains("before the tunnel was established"));
  }

  #[tokio::test]
  async fn detached_worker_survives_launcher_and_disconnects_by_pid() {
    let directory = tempfile::tempdir().unwrap();
    tokio::time::timeout(Duration::from_secs(5), launch(fixture(directory.path()), &[]))
      .await
      .unwrap()
      .unwrap();
    let pid = std::fs::read_to_string(directory.path().join("pid"))
      .unwrap()
      .parse::<i32>()
      .unwrap();
    let pid = Pid::from_raw(pid);
    assert_eq!(
      getsid(Some(pid)).unwrap(),
      pid,
      "Worker must own a terminal-independent session"
    );
    kill(pid, None).unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
      while !std::fs::read_to_string(directory.path().join("worker.log"))
        .unwrap()
        .contains("helper-output")
      {
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();
    let log = std::fs::read_to_string(directory.path().join("worker.log")).unwrap();
    assert!(log.contains("failed-attempt-output"));
    assert!(!log.contains("restored-foreground-output"));
    #[derive(clap::Parser)]
    struct Disconnect {
      #[command(flatten)]
      args: crate::disconnect::DisconnectArgs,
    }
    let args = <Disconnect as clap::Parser>::parse_from(["test"]);
    crate::disconnect::DisconnectHandler::new(&args.args, &directory.path().join("pid"))
      .handle()
      .await
      .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
      while !directory.path().join("stopped").exists() {
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();
  }

  #[tokio::test]
  async fn unsafe_log_path_fails_before_detaching() {
    let directory = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(directory.path().join("target"), directory.path().join("worker.log")).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), launch(fixture(directory.path()), &[]))
      .await
      .unwrap();
    assert!(result.is_err());
    assert!(!directory.path().join("target").exists());
  }

  #[tokio::test]
  async fn worker_fixture() {
    let Ok(fd) = std::env::var("GPCLIENT_TEST_FD") else {
      return;
    };
    let directory = std::path::PathBuf::from(std::env::var_os("GPCLIENT_TEST_DIR").unwrap());
    let worker = match BackgroundWorker::new(fd.parse().unwrap(), &directory.join("worker.log")) {
      Ok(worker) => worker,
      Err(error) => {
        eprintln!("{error:#}");
        std::process::exit(1);
      }
    };
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();
    std::fs::write(directory.join("pid"), getpid().to_string()).unwrap();
    // A failed attempt must restore foreground streams before another sign-in.
    {
      let _stdio = worker.redirect_stdio().unwrap();
      println!("failed-attempt-output");
    }
    println!("restored-foreground-output");
    let mut stdio = worker.redirect_stdio().unwrap();
    // Like OpenConnect's script-tun helper, this child starts before readiness.
    let mut helper = std::process::Command::new("sh")
      .args(["-c", "sleep 0.1; echo helper-output"])
      .spawn()
      .unwrap();
    worker.detach(&mut stdio).unwrap();
    println!("detached-output");
    io::stdout().flush().unwrap();
    interrupt.recv().await;
    helper.wait().unwrap();
    std::fs::write(directory.join("stopped"), "stopped").unwrap();
  }
}
