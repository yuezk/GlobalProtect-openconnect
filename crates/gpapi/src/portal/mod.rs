mod config;
mod prelogin;

pub use config::*;
pub use prelogin::*;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum PortalError {
  #[error("Portal prelogin error: {0}")]
  PreloginError(String),
  #[error("Portal config error: {0}")]
  ConfigError(String),
}
