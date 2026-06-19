mod auth_messenger;
mod webview_auth;

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
#[path = "webview/gtk.rs"]
mod platform_impl;

#[cfg(target_os = "macos")]
#[path = "webview/macos.rs"]
mod platform_impl;

#[cfg(not(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
)))]
compile_error!("webview-auth is only supported on macOS and GTK/WebKitGTK Unix targets");

pub use webview_auth::WebviewAuthenticator;
