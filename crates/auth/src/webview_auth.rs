mod auth_messenger;
mod auth_response;
mod auth_settings;
mod webview_auth_ext;

#[cfg_attr(not(target_os = "macos"), path = "webview_auth/unix.rs")]
#[cfg_attr(target_os = "macos", path = "webview_auth/macos.rs")]
mod platform_impl;

#[cfg(target_os = "macos")]
mod navigation_delegate;

pub use webview_auth_ext::WebviewAuthenticator;
