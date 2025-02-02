mod auth_messenger;
mod webview_auth;

#[cfg_attr(not(any(target_os = "macos", target_os = "windows")), path = "webview/unix.rs")]
#[cfg_attr(target_os = "macos", path = "webview/macos.rs")]
#[cfg_attr(windows, path = "webview/windows.rs")]
mod platform_impl;

pub use webview_auth::WebviewAuthenticator;
