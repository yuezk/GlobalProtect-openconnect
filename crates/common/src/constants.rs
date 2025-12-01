pub const GP_USER_AGENT: &str = "PAN GlobalProtect";
pub const GP_SERVICE_LOCK_FILE: &str = "/var/run/gpservice.lock";
pub const GP_CALLBACK_PORT_FILENAME: &str = "gpcallback.port";

// Release binaries - macOS (Apple Silicon Homebrew)
#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub const GP_CLIENT_BINARY: &str = "/opt/homebrew/bin/gpclient";
#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub const GP_SERVICE_BINARY: &str = "/opt/homebrew/bin/gpservice";
#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub const GP_GUI_BINARY: &str = "/opt/homebrew/bin/gpgui";
#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub const GP_GUI_HELPER_BINARY: &str = "/opt/homebrew/bin/gpgui-helper";
#[cfg(all(not(debug_assertions), target_os = "macos"))]
pub const GP_AUTH_BINARY: &str = "/opt/homebrew/bin/gpauth";

// Release binaries - Linux
#[cfg(all(not(debug_assertions), not(target_os = "macos")))]
pub const GP_CLIENT_BINARY: &str = "/usr/bin/gpclient";
#[cfg(all(not(debug_assertions), not(target_os = "macos")))]
pub const GP_SERVICE_BINARY: &str = "/usr/bin/gpservice";
#[cfg(all(not(debug_assertions), not(target_os = "macos")))]
pub const GP_GUI_BINARY: &str = "/usr/bin/gpgui";
#[cfg(all(not(debug_assertions), not(target_os = "macos")))]
pub const GP_GUI_HELPER_BINARY: &str = "/usr/bin/gpgui-helper";
#[cfg(all(not(debug_assertions), not(target_os = "macos")))]
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
