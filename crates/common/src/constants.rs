pub const GP_USER_AGENT: &str = "PAN GlobalProtect";
pub const GP_CLIENT_VERSION_LINUX: &str = "6.3.3-619";
pub const GP_CLIENT_VERSION_WINDOWS: &str = "6.3.3-650";
pub const GP_CLIENT_VERSION_MACOS: &str = "6.3.3-915";
pub const GP_SERVICE_LOCK_FILE: &str = "/var/run/gpservice.lock";
#[cfg(target_os = "linux")]
pub const GP_VPNC_SCRIPT_INSTALLER_BINARY: &str = "/usr/libexec/gpclient/gp-vpnc-script-installer";
#[cfg(target_os = "linux")]
pub const INSTALLED_VPNC_SCRIPT: &str = "/var/lib/gpclient/scripts/vpnc-script";
#[cfg(target_os = "linux")]
pub const INSTALLED_VPNC_SCRIPT_METADATA: &str = "/var/lib/gpclient/scripts/vpnc-script.metadata";
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub const GP_VPNC_SCRIPT_INSTALLER_BINARY: &str = "/usr/local/libexec/gpclient/gp-vpnc-script-installer";
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub const INSTALLED_VPNC_SCRIPT: &str = "/var/db/gpclient/scripts/vpnc-script";
#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
pub const INSTALLED_VPNC_SCRIPT_METADATA: &str = "/var/db/gpclient/scripts/vpnc-script.metadata";
pub const MAX_VPNC_SCRIPT_SIZE: usize = 1024 * 1024;

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
