pub mod auth;
pub mod credential;
pub mod error;
pub mod gateway;
pub mod gp_params;
pub mod portal;

#[cfg(unix)]
pub mod process;

pub mod service;
pub mod utils;

#[cfg(feature = "logger")]
pub mod logger;

#[cfg(feature = "clap")]
pub mod clap;

#[cfg(debug_assertions)]
pub const GP_API_KEY: &[u8; 32] = &[0; 32];
