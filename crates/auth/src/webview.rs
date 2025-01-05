mod auth_messenger;
mod webview_auth;

#[cfg_attr(not(target_os = "macos"), path = "webview/unix.rs")]
#[cfg_attr(target_os = "macos", path = "webview/macos.rs")]
mod platform_impl;

pub use webview_auth::WebviewAuthenticator;
