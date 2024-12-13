mod auth_messenger;
mod common;

pub mod auth_window;

#[cfg_attr(not(target_os = "macos"), path = "unix.rs")]
mod platform_impl;
