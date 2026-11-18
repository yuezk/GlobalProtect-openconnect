use std::process::{Command, ExitStatus};

const DMABUF: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
pub const MAIN_WEBVIEW_INITIALIZED: &str = "GP Connect main webview initialized";
const GBM_DIAGNOSTIC: &[u8] = b"Failed to create GBM buffer";
pub(crate) const AUTH_STDERR_PREFIX: &[u8] = b"gpauth stderr: ";
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
  shared_memory: bool,
}

impl RecoveryState {
  pub fn dmabuf_disabled(&self) -> bool {
    self.first_failure.is_some() && !self.shared_memory
  }

  pub fn shared_memory(&self) -> bool {
    self.shared_memory
  }

  pub fn recover(&mut self, error: &anyhow::Error) -> bool {
    if self.first_failure.is_some() {
      return false;
    }
    let gbm_failure = error.downcast_ref::<GbmBufferFailure>().is_some();
    if !gbm_failure && error.downcast_ref::<RenderingCrash>().is_none() {
      return false;
    }
    if gbm_failure && let Err(error) = crate::gui::set_compatibility_graphics(true) {
      log::warn!("Could not save compatibility graphics setting: {error:#}");
      return false;
    }
    self.first_failure = Some(error.to_string());
    self.shared_memory = gbm_failure;
    true
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

#[cfg(unix)]
pub(crate) fn is_crash_exit(status: ExitStatus) -> bool {
  use std::os::unix::process::ExitStatusExt;
  !status.success() && !matches!(status.signal(), Some(1 | 2 | 9 | 15))
}

#[cfg(windows)]
pub(crate) fn is_crash_exit(status: ExitStatus) -> bool {
  status.code().is_some_and(|code| (code as u32) >= 0xC0000000)
}

pub fn eligible(command: &Command) -> bool {
  eligible_with_inheritance(command, true)
}

pub(crate) fn eligible_with_inheritance(command: &Command, inherits: bool) -> bool {
  cfg!(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))
    && effective_env(command, "WAYLAND_DISPLAY", inherits).is_some_and(|value| !value.is_empty())
    && effective_env(command, DMABUF, inherits).is_none()
}

pub(crate) fn allows_shared_memory_override(command: &Command, inherits: bool) -> bool {
  crate::gui::COMPATIBILITY_GRAPHICS_SUPPORTED
    && effective_env(command, DMABUF, inherits).is_none()
    && effective_env(command, crate::gui::FORCE_SHARED_MEMORY, inherits).is_none()
}

#[derive(Debug, thiserror::Error)]
#[error("WebKit failed to create a GBM buffer")]
pub(crate) struct GbmBufferFailure;

fn effective_env(command: &Command, key: &str, inherits: bool) -> Option<std::ffi::OsString> {
  for (name, value) in command.get_envs() {
    if name == key {
      return value.map(ToOwned::to_owned);
    }
  }
  inherits.then(|| std::env::var_os(key)).flatten()
}

pub(crate) fn is_gbm_diagnostic(line: &[u8]) -> bool {
  !line.starts_with(AUTH_STDERR_PREFIX)
    && line
      .windows(GBM_DIAGNOSTIC.len())
      .any(|message| message == GBM_DIAGNOSTIC)
}

/// Observes diagnostic lines without retaining arbitrary application output.
#[derive(Default)]
pub(crate) struct StderrDiagnostic {
  line: Vec<u8>,
  overflow: bool,
  matched: bool,
  gbm_failed: bool,
  main_webview_initialized: bool,
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
    if !self.overflow {
      if line == MAIN_WEBVIEW_INITIALIZED.as_bytes() {
        self.main_webview_initialized = true;
      }
      if !self.main_webview_initialized && is_gbm_diagnostic(line) {
        self.gbm_failed = true;
      }
    }
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

  pub(crate) fn should_retry_gbm(&self, can_retry_wayland: bool) -> bool {
    self.gbm_failed && !(can_retry_wayland && self.matched)
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
  fn detects_gbm_failure_before_main_webview_initializes() {
    for line in [
      b"Failed to create GBM buffer of size 280x404: Invalid argument\n".as_slice(),
      b"(WebKitWebProcess:42): Warning: Failed to create GBM buffer\n",
      b"gpauth: Failed to create GBM buffer\n",
    ] {
      let mut observer = StderrDiagnostic::default();
      for bytes in line.chunks(7) {
        observer.push(bytes);
      }
      assert!(observer.gbm_failed);
      assert!(!observer.matched());
    }
  }

  #[test]
  fn ignores_gbm_failure_after_startup_and_from_auth_stderr() {
    let mut observer = StderrDiagnostic::default();
    observer
      .push(b"gpauth stderr: (WebKitWebProcess:42): Failed to create GBM buffer of size 900x648: Invalid argument\n");
    assert!(!observer.gbm_failed);
    observer.push(format!("{MAIN_WEBVIEW_INITIALIZED}\n").as_bytes());
    observer.push(b"Failed to create GBM buffer of size 840x640: Invalid argument\n");
    assert!(!observer.gbm_failed);
  }

  #[test]
  fn preserves_wayland_retry_when_both_diagnostics_are_observed() {
    let mut observer = StderrDiagnostic::default();
    observer.push(b"Failed to create GBM buffer of size 280x404: Invalid argument\n");
    observer.push(b"Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.\n");
    assert!(observer.matched());
    assert!(!observer.should_retry_gbm(true));
    assert!(observer.should_retry_gbm(false));
  }

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

  #[cfg(unix)]
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
  #[cfg(unix)]
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
