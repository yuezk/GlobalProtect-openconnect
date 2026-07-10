use std::path::PathBuf;

pub const GP_USER_AGENT: &str = "PAN GlobalProtect";
pub const GP_CLIENT_VERSION_LINUX: &str = "6.3.3-619";
pub const GP_CLIENT_VERSION_WINDOWS: &str = "6.3.3-650";
pub const GP_CLIENT_VERSION_MACOS: &str = "6.3.3-915";
pub const GP_SERVICE_LOCK_FILE: &str = "/var/run/gpservice.lock";
pub const GP_CALLBACK_PORT_FILENAME: &str = "gpcallback.port";
pub const GP_CALLBACK_LOG_FILENAME: &str = "gpcallback.log";

pub fn gp_callback_port_file() -> PathBuf {
  gp_callback_file(GP_CALLBACK_PORT_FILENAME)
}

pub fn gp_callback_log_file() -> PathBuf {
  gp_callback_file(GP_CALLBACK_LOG_FILENAME)
}

fn gp_callback_file(filename: &str) -> PathBuf {
  if let Some(runtime_dir) = non_empty_env("XDG_RUNTIME_DIR") {
    return PathBuf::from(runtime_dir).join(filename);
  }
  let owner = non_empty_env("USER")
    .or_else(|| non_empty_env("LOGNAME"))
    .unwrap_or_else(|| "unknown".to_string());

  let (stem, suffix) = filename.rsplit_once('.').unwrap_or((filename, ""));
  let filename = if suffix.is_empty() {
    format!("{}.{}", stem, safe_filename_component(&owner))
  } else {
    format!("{}.{}.{}", stem, safe_filename_component(&owner), suffix)
  };
  std::env::temp_dir().join(filename)
}

fn non_empty_env(key: &str) -> Option<String> {
  std::env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn safe_filename_component(value: &str) -> String {
  value
    .chars()
    .map(|ch| {
      if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
        ch
      } else {
        '_'
      }
    })
    .collect()
}

// Release binaries - macOS (Apple Silicon Homebrew)
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))]
pub const GP_CLIENT_BINARY: &str = "/opt/homebrew/bin/gpclient";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))]
pub const GP_SERVICE_BINARY: &str = "/opt/homebrew/bin/gpservice";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))]
pub const GP_GUI_BINARY: &str = "/opt/homebrew/bin/gpgui";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))]
pub const GP_GUI_HELPER_BINARY: &str = "/opt/homebrew/bin/gpgui-helper";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "aarch64"))]
pub const GP_AUTH_BINARY: &str = "/opt/homebrew/bin/gpauth";

// Release binaries - macOS (Intel Homebrew)
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "x86_64"))]
pub const GP_CLIENT_BINARY: &str = "/usr/local/bin/gpclient";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "x86_64"))]
pub const GP_SERVICE_BINARY: &str = "/usr/local/bin/gpservice";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "x86_64"))]
pub const GP_GUI_BINARY: &str = "/usr/local/bin/gpgui";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "x86_64"))]
pub const GP_GUI_HELPER_BINARY: &str = "/usr/local/bin/gpgui-helper";
#[cfg(all(not(debug_assertions), target_os = "macos", target_arch = "x86_64"))]
pub const GP_AUTH_BINARY: &str = "/usr/local/bin/gpauth";

// Release binaries - BSD package layout
#[cfg(all(not(debug_assertions), any(target_os = "freebsd", target_os = "openbsd")))]
pub const GP_CLIENT_BINARY: &str = "/usr/local/bin/gpclient";
#[cfg(all(not(debug_assertions), any(target_os = "freebsd", target_os = "openbsd")))]
pub const GP_SERVICE_BINARY: &str = "/usr/local/bin/gpservice";
#[cfg(all(not(debug_assertions), any(target_os = "freebsd", target_os = "openbsd")))]
pub const GP_GUI_BINARY: &str = "/usr/local/bin/gpgui";
#[cfg(all(not(debug_assertions), any(target_os = "freebsd", target_os = "openbsd")))]
pub const GP_GUI_HELPER_BINARY: &str = "/usr/local/bin/gpgui-helper";
#[cfg(all(not(debug_assertions), any(target_os = "freebsd", target_os = "openbsd")))]
pub const GP_AUTH_BINARY: &str = "/usr/local/bin/gpauth";

// Release binaries - Linux
#[cfg(all(
  not(debug_assertions),
  not(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))
))]
pub const GP_CLIENT_BINARY: &str = "/usr/bin/gpclient";
#[cfg(all(
  not(debug_assertions),
  not(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))
))]
pub const GP_SERVICE_BINARY: &str = "/usr/bin/gpservice";
#[cfg(all(
  not(debug_assertions),
  not(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))
))]
pub const GP_GUI_BINARY: &str = "/usr/bin/gpgui";
#[cfg(all(
  not(debug_assertions),
  not(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))
))]
pub const GP_GUI_HELPER_BINARY: &str = "/usr/bin/gpgui-helper";
#[cfg(all(
  not(debug_assertions),
  not(any(target_os = "macos", target_os = "freebsd", target_os = "openbsd"))
))]
pub const GP_AUTH_BINARY: &str = "/usr/bin/gpauth";

// Debug binaries are set in build.rs via environment variables
#[cfg(debug_assertions)]
pub const GP_CLIENT_BINARY: &str = env!("GP_CLIENT_BINARY");
#[cfg(debug_assertions)]
pub const GP_SERVICE_BINARY: &str = env!("GP_SERVICE_BINARY");
#[cfg(debug_assertions)]
pub const GP_GUI_BINARY: &str = env!("GP_GUI_BINARY");
#[cfg(debug_assertions)]
pub const GP_GUI_HELPER_BINARY: &str = env!("GP_GUI_HELPER_BINARY");
#[cfg(debug_assertions)]
pub const GP_AUTH_BINARY: &str = env!("GP_AUTH_BINARY");
