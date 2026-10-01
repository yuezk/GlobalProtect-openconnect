use std::{
  fs,
  io::{self, Read},
  os::unix::{
    fs::{MetadataExt, PermissionsExt},
    process::CommandExt,
  },
  path::{Path, PathBuf},
  process::{Child, Command, Output, Stdio},
  thread,
  time::{Duration, Instant},
};

use nix::{
  fcntl::{FcntlArg, OFlag, fcntl},
  sys::signal::{Signal, killpg},
  unistd::Pid,
};

/// Invocation control borrowed by collection; interruption must never be treated
/// as an unavailable optional product.
pub trait CollectionControl {
  fn check(&self) -> io::Result<()>;
}

impl<F: Fn() -> io::Result<()>> CollectionControl for F {
  fn check(&self) -> io::Result<()> {
    self()
  }
}

pub struct CollectionBudget {
  deadline: Instant,
}

impl CollectionBudget {
  pub fn new(timeout: Duration) -> Self {
    Self {
      deadline: Instant::now() + timeout,
    }
  }
}

impl CollectionControl for CollectionBudget {
  fn check(&self) -> io::Result<()> {
    if Instant::now() >= self.deadline {
      Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Device information collection timed out",
      ))
    } else {
      Ok(())
    }
  }
}

const COMMAND_OUTPUT_LIMIT: usize = 256 * 1024;
const TOOL_DIRECTORIES: &[&str] = &[
  "/usr/bin",
  "/usr/sbin",
  "/bin",
  "/sbin",
  "/usr/local/bin",
  "/usr/local/sbin",
];

pub struct CollectorCommands<'a> {
  control: &'a dyn CollectionControl,
}

impl<'a> CollectorCommands<'a> {
  pub fn new(control: &'a dyn CollectionControl) -> Self {
    Self { control }
  }

  pub fn resolve(&self, name: &str) -> Option<PathBuf> {
    TOOL_DIRECTORIES
      .iter()
      .find_map(|directory| resolve_executable(&Path::new(directory).join(name), uzers::get_effective_uid() == 0))
  }

  pub fn run(&self, name: &str, args: &[&str]) -> io::Result<Option<Output>> {
    self.control.check()?;
    let Some(path) = self.resolve(name) else {
      return Ok(None);
    };
    self.run_path(&path, args)
  }

  pub fn run_path(&self, path: &Path, args: &[&str]) -> io::Result<Option<Output>> {
    self.control.check()?;
    let home = if uzers::get_effective_uid() == 0 {
      if cfg!(target_os = "macos") {
        "/var/root"
      } else {
        "/root"
      }
    } else {
      "/"
    };
    let mut command = Command::new(path);
    command
      .args(args)
      .env_clear()
      .env("PATH", command_path(uzers::get_effective_uid() == 0)?)
      .env("LC_ALL", "C")
      .env("HOME", home)
      .current_dir("/")
      .stdin(Stdio::null())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .process_group(0);
    let child = match command.spawn() {
      Ok(child) => child,
      Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied) => {
        return Ok(None);
      }
      Err(error) => return Err(error),
    };
    let mut child = CollectionChild(child);
    let mut stdout = child.0.stdout.take().expect("collector stdout was piped");
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
      self.control.check()?;
      if !eof {
        match stdout.read(&mut buffer) {
          Ok(0) => eof = true,
          Ok(length) => {
            if output.len() + length > COMMAND_OUTPUT_LIMIT {
              return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Device information collector output exceeds its limit",
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
        return Ok(Some(Output {
          status,
          stdout: output,
          stderr: Vec::new(),
        }));
      }
      thread::sleep(Duration::from_millis(10));
    }
  }
}

fn resolve_executable(path: &Path, require_root: bool) -> Option<PathBuf> {
  let resolved = fs::canonicalize(path).ok()?;
  let metadata = fs::metadata(&resolved).ok()?;
  if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 && (!require_root || root_owned_path(&resolved)) {
    Some(resolved)
  } else {
    None
  }
}

/// Known command directories, canonicalized and restricted to trusted root
/// ownership when used for privileged execution.
pub fn command_path(require_root: bool) -> io::Result<std::ffi::OsString> {
  let directories = TOOL_DIRECTORIES.iter().filter_map(|directory| {
    let resolved = fs::canonicalize(directory).ok()?;
    if resolved.is_dir() && (!require_root || root_owned_path(&resolved)) {
      Some(resolved)
    } else {
      None
    }
  });
  std::env::join_paths(directories).map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
}

fn root_owned_path(path: &Path) -> bool {
  path.ancestors().all(|part| {
    fs::symlink_metadata(part).is_ok_and(|metadata| {
      metadata.uid() == 0 && metadata.permissions().mode() & 0o022 == 0 && !metadata.file_type().is_symlink()
    })
  })
}

struct CollectionChild(Child);

impl Drop for CollectionChild {
  fn drop(&mut self) {
    // Also stop descendants retaining the pipe after the direct child exits.
    let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
    let _ = self.0.wait();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn root_command_path_contains_only_trusted_canonical_directories() {
    let path = command_path(true).unwrap();
    let directories: Vec<_> = std::env::split_paths(&path).collect();
    assert!(!directories.is_empty());
    for directory in directories {
      assert!(directory.is_dir());
      assert!(root_owned_path(&directory));
      assert_eq!(directory, fs::canonicalize(&directory).unwrap());
    }
  }

  #[test]
  fn executable_resolution_returns_target_not_mutable_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let link = dir.path().join("tool");
    std::os::unix::fs::symlink("/bin/sh", &link).unwrap();
    let resolved = resolve_executable(&link, false).unwrap();
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink("/bin/echo", &link).unwrap();
    assert_eq!(resolved, fs::canonicalize("/bin/sh").unwrap());
    assert_ne!(resolved, fs::canonicalize(&link).unwrap());
  }

  #[test]
  fn cancellation_stops_running_collector() {
    let started = Instant::now();
    let control = || {
      if started.elapsed() >= Duration::from_millis(100) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
      } else {
        Ok(())
      }
    };
    let error = CollectorCommands::new(&control)
      .run_path(Path::new("/bin/sh"), &["-c", "sleep 30 & wait"])
      .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    assert!(started.elapsed() < Duration::from_secs(2));
  }

  #[test]
  fn collector_has_private_environment_and_cwd() {
    let budget = CollectionBudget::new(Duration::from_secs(2));
    let output = CollectorCommands::new(&budget)
      .run_path(
        Path::new("/bin/sh"),
        &[
          "-c",
          "printf '%s|%s|%s' \"$LC_ALL\" \"$PWD\" \"${SENTINEL_OUTPUT_JSON-unset}\"",
        ],
      )
      .unwrap()
      .unwrap();
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "C|/|unset");
  }

  #[test]
  fn deadline_stops_collector_instead_of_hanging() {
    let budget = CollectionBudget::new(Duration::from_millis(100));
    let started = Instant::now();
    let error = CollectorCommands::new(&budget)
      .run_path(Path::new("/bin/sh"), &["-c", "sleep 30"])
      .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
  }

  #[test]
  fn collector_output_is_bounded() {
    let budget = CollectionBudget::new(Duration::from_secs(2));
    let error = CollectorCommands::new(&budget)
      .run_path(Path::new("/bin/sh"), &["-c", "yes x"])
      .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
  }

  #[test]
  fn cancellation_is_not_an_optional_missing_product() {
    let control = || Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
    let error = CollectorCommands::new(&control).run("not-installed", &[]).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
  }
}
