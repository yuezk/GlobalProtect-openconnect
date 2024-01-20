pub mod auth;
pub mod credential;
pub mod gateway;
pub mod gp_params;
pub mod portal;
pub mod process;
pub mod service;
pub mod utils;

#[cfg(feature = "clap")]
pub mod clap;

#[cfg(debug_assertions)]
pub const GP_API_KEY: &[u8; 32] = &[0; 32];

pub const GP_USER_AGENT: &str = "PAN GlobalProtect";
pub const GP_SERVICE_LOCK_FILE: &str = "/var/run/gpservice.lock";

#[cfg(not(debug_assertions))]
pub const GP_SERVICE_BINARY: &str = "/usr/bin/gpservice";
#[cfg(not(debug_assertions))]
pub const GP_GUI_BINARY: &str = "/usr/bin/gpgui";
#[cfg(not(debug_assertions))]
pub(crate) const GP_AUTH_BINARY: &str = "/usr/bin/gpauth";

#[cfg(debug_assertions)]
pub const GP_SERVICE_BINARY: &str = dotenvy_macro::dotenv!("GP_SERVICE_BINARY");
#[cfg(debug_assertions)]
pub const GP_GUI_BINARY: &str = dotenvy_macro::dotenv!("GP_GUI_BINARY");
#[cfg(debug_assertions)]
pub(crate) const GP_AUTH_BINARY: &str = dotenvy_macro::dotenv!("GP_AUTH_BINARY");
