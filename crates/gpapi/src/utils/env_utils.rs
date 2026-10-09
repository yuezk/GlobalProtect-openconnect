use std::env;
use std::ffi::OsStr;

use log::info;

pub fn patch_gui_runtime_env(hidpi: bool) {
  if is_wayland_session(
    env::var_os("WAYLAND_DISPLAY").as_deref(),
    env::var("XDG_SESSION_TYPE").ok().as_deref(),
  ) {
    info!("Wayland session detected, enabling software GL");
    set_env_if_missing("LIBGL_ALWAYS_SOFTWARE", "1");
  }

  if hidpi {
    info!("Setting GDK_SCALE=2 and GDK_DPI_SCALE=0.5");
    unsafe {
      std::env::set_var("GDK_SCALE", "2");
      std::env::set_var("GDK_DPI_SCALE", "0.5");
    };
  }
}

fn is_wayland_session(wayland_display: Option<&OsStr>, xdg_session_type: Option<&str>) -> bool {
  if wayland_display.is_some() {
    return true;
  }

  matches!(xdg_session_type, Some("wayland"))
}

fn set_env_if_missing(key: &str, value: &str) {
  if env::var_os(key).is_some() {
    return;
  }

  info!("Setting {}={}", key, value);
  unsafe { env::set_var(key, value) };
}

#[cfg(test)]
mod tests {
  use std::ffi::OsStr;

  use super::is_wayland_session;

  #[test]
  fn detects_wayland_from_wayland_display() {
    assert!(is_wayland_session(Some(OsStr::new("wayland-1")), None));
  }

  #[test]
  fn detects_wayland_from_session_type() {
    assert!(is_wayland_session(None, Some("wayland")));
  }

  #[test]
  fn ignores_non_wayland_sessions() {
    assert!(!is_wayland_session(None, None));
    assert!(!is_wayland_session(None, Some("x11")));
    assert!(!is_wayland_session(None, Some("tty")));
  }
}
