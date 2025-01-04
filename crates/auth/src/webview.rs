mod auth_messenger;
mod response_reader;
mod webview_auth;

#[cfg_attr(not(target_os = "macos"), path = "webview/unix.rs")]
#[cfg_attr(target_os = "macos", path = "webview/macos.rs")]
mod platform_impl;

#[cfg(target_os = "macos")]
mod navigation_delegate;

pub use webview_auth::WebviewAuthenticator;
pub use webview_auth::WebviewAuthenticatorBuilder;
