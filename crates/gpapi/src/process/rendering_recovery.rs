use std::process::{Command, ExitStatus};

const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
const DIAGNOSTIC: &[u8] = b"Error 71 (Protocol error) dispatching to Wayland display";

#[derive(Debug, thiserror::Error)]
#[error("WebKit Wayland rendering process exited with {status}")]
pub struct RenderingCrash {
  pub status: ExitStatus,
}

#[derive(Debug, thiserror::Error)]
#[error("Rendering recovery failed: {0:#}. Collect diagnostics for a bug report")]
pub struct RecoveryFailed(#[source] anyhow::Error);

#[derive(Default)]
pub struct RecoveryState {
  first_failure: Option<String>,
}

impl RecoveryState {
  pub fn dmabuf_disabled(&self) -> bool {
    self.first_failure.is_some()
  }

  pub fn recover(&mut self, error: &anyhow::Error) -> bool {
    if self.first_failure.is_none() && error.downcast_ref::<RenderingCrash>().is_some() {
      self.first_failure = Some(error.to_string());
      true
    } else {
      false
    }
  }

  pub fn terminal(&self, error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<crate::auth::AuthenticationCancelled>().is_some()
      || error.downcast_ref::<RecoveryFailed>().is_some()
    {
      return error;
    }
    match &self.first_failure {
      Some(first) => RecoveryFailed(error.context(format!("First attempt: {first}"))).into(),
      None => error,
    }
  }

  pub fn finish<T>(&self, result: anyhow::Result<T>) -> anyhow::Result<T> {
    result.map_err(|error| self.terminal(error))
  }
}

pub(crate) fn is_crash_exit(status: ExitStatus) -> bool {
  use std::os::unix::process::ExitStatusExt;
  !status.success() && !matches!(status.signal(), Some(1 | 2 | 9 | 15))
}

pub fn eligible(command: &Command) -> bool {
  eligible_with_inheritance(command, true)
}

pub(crate) fn eligible_with_inheritance(command: &Command, inherits: bool) -> bool {
  cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))
    && effective_env(command, "WAYLAND_DISPLAY", inherits).is_some_and(|value| !value.is_empty())
    && effective_env(command, DMABUF, inherits).is_none()
}

fn effective_env(command: &Command, key: &str, inherits: bool) -> Option<std::ffi::OsString> {
  for (name, value) in command.get_envs() {
    if name == key {
      return value.map(ToOwned::to_owned);
    }
  }
  inherits.then(|| std::env::var_os(key)).flatten()
}

/// Observes diagnostic lines without retaining arbitrary application output.
#[derive(Default)]
pub(crate) struct StderrDiagnostic {
  line: Vec<u8>,
  overflow: bool,
  matched: bool,
}

impl StderrDiagnostic {
  pub(crate) fn push(&mut self, bytes: &[u8]) {
    for &byte in bytes {
      if byte == b'\n' {
        self.check_line();
        self.line.clear();
        self.overflow = false;
      } else if self.line.len() < 1024 {
        self.line.push(byte);
      } else {
        self.overflow = true;
      }
    }
  }

  fn check_line(&mut self) {
    let line = self.line.trim_ascii_end();
    let message = line.strip_suffix(b".").unwrap_or(line);
    if !self.overflow
      && (line.starts_with(b"Gdk-Message:")
        || line.starts_with(b"(gpgui:")
        || line.starts_with(b"(gpauth:")
        || line.starts_with(b"(gpgui-helper:"))
      && line.windows(b"Gdk-Message:".len()).any(|part| part == b"Gdk-Message:")
      && message.ends_with(DIAGNOSTIC)
    {
      self.matched = true;
    }
  }

  pub(crate) fn matched(&mut self) -> bool {
    self.check_line();
    self.matched
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn recognizes_split_diagnostic_without_matching_quoted_warning() {
    let mut observer = StderrDiagnostic::default();
    observer.push(b"warning: Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.\n");
    assert!(!observer.matched());
    observer.push(b"Gdk-Message: 12:00:00: Error 71 (Protocol error) dispat");
    observer.push(b"ching to Wayland display.\n");
    assert!(observer.matched());
  }

  #[test]
  fn recognizes_real_gtk_diagnostic_with_prefix_and_line_endings() {
    for prefix in [
      "Gdk-Message:",
      "(gpauth:42): Gdk-Message:",
      "(gpgui:42): Gdk-Message:",
      "(gpgui-helper:42): Gdk-Message:",
    ] {
      for punctuation in ["", "."] {
        for ending in ["\n", "\r\n", "", " \t\n"] {
          let line = format!(
            "{prefix} 10:49:22.617: Error 71 (Protocol error) dispatching to Wayland display{punctuation}{ending}"
          );
          let mut observer = StderrDiagnostic::default();
          for chunk in line.as_bytes().chunks(7) {
            observer.push(chunk);
          }
          assert!(observer.matched(), "unrecognized GTK diagnostic: {line:?}");
        }
      }
    }
  }

  #[test]
  fn rejects_quoted_or_extended_gtk_diagnostic() {
    for line in [
      "warning: Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.\n",
      "Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display. Retry suggested\n",
      "Gdk-Message: Error 70 (Communication error) dispatching to Wayland display.\n",
    ] {
      let mut observer = StderrDiagnostic::default();
      observer.push(line.as_bytes());
      assert!(!observer.matched(), "incorrectly recognized: {line:?}");
    }
  }

  #[test]
  fn oversized_lines_do_not_grow_or_match() {
    let mut observer = StderrDiagnostic::default();
    observer.push(&vec![b'x'; 100_000]);
    assert_eq!(observer.line.len(), 1024);
    observer.push(DIAGNOSTIC);
    assert!(!observer.matched());
  }
  #[test]
  fn respects_explicit_overrides_and_x11_environment() {
    let mut command = Command::new("fixture");
    command.env_clear().env("WAYLAND_DISPLAY", "wayland-0");
    let platform = cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"));
    assert_eq!(eligible_with_inheritance(&command, false), platform);
    for value in ["", "0", "1"] {
      command.env(DMABUF, value);
      assert!(!eligible_with_inheritance(&command, false));
    }
    command
      .env_remove(DMABUF)
      .env_remove("WAYLAND_DISPLAY")
      .env("DISPLAY", ":0");
    assert!(!eligible_with_inheritance(&command, false));
  }

  #[test]
  fn recovery_is_once_and_preserves_cancellation_and_both_failures() {
    use std::os::unix::process::ExitStatusExt;
    let mut recovery = RecoveryState::default();
    let first = RenderingCrash {
      status: ExitStatus::from_raw(71 << 8),
    }
    .into();
    assert!(recovery.recover(&first));
    assert!(!recovery.recover(&first));
    assert!(
      recovery
        .terminal(crate::auth::AuthenticationCancelled.into())
        .downcast_ref::<crate::auth::AuthenticationCancelled>()
        .is_some()
    );
    let error = recovery.terminal(anyhow::anyhow!("second exit 72"));
    assert!(error.downcast_ref::<RecoveryFailed>().is_some());
    assert!(error.to_string().contains("71"));
    assert!(error.to_string().contains("second exit 72"));
  }
  #[test]
  fn explicit_termination_is_not_a_rendering_crash() {
    use std::os::unix::process::ExitStatusExt;
    for signal in [1, 2, 9, 15] {
      assert!(!is_crash_exit(ExitStatus::from_raw(signal)));
    }
    assert!(is_crash_exit(ExitStatus::from_raw(6)));
    assert!(is_crash_exit(ExitStatus::from_raw(71 << 8)));
    assert!(!is_crash_exit(ExitStatus::from_raw(0)));
  }
}
