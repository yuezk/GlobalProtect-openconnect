#[cfg(unix)]
mod command_runner;
#[cfg(windows)]
#[path = "windows/command_runner.rs"]
mod command_runner;
#[cfg(unix)]
mod command_traits;
#[cfg(windows)]
#[path = "windows/command_traits.rs"]
mod command_traits;
#[cfg(unix)]
mod desktop_child;
#[cfg(unix)]
mod desktop_session_env;
#[cfg(unix)]
pub(crate) mod gui_helper_launcher;

pub use command_traits::CommandExt;

pub mod auth_launcher;
#[cfg(unix)]
pub mod desktop_launcher;
#[cfg(unix)]
pub mod gui_launcher;
#[cfg(any(target_os = "linux", all(test, unix)))]
pub mod gui_switch;
#[cfg(any(target_os = "linux", all(test, unix)))]
mod nsh_runtime;
#[cfg(unix)]
pub mod users;

#[cfg(unix)]
pub mod collection;
#[cfg(windows)]
#[path = "windows/collection.rs"]
pub mod collection;

pub mod rendering_recovery;
#[cfg(windows)]
#[path = "windows/user_process.rs"]
pub mod user_process;

#[cfg(windows)]
#[path = "windows/desktop_auth.rs"]
mod desktop_auth;

#[cfg(windows)]
#[path = "windows/auth_bootstrap.rs"]
pub mod auth_bootstrap;

#[cfg(windows)]
#[path = "windows/console_input.rs"]
mod console_input;
