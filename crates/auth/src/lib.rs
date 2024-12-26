mod authenticator;
pub use authenticator::auth_prelogin;
pub use authenticator::Authenticator;

#[cfg(feature = "browser-auth")]
mod browser_auth;
#[cfg(feature = "browser-auth")]
pub use browser_auth::BrowserAuthenticator;

#[cfg(feature = "webview-auth")]
mod webview_auth;
#[cfg(feature = "webview-auth")]
pub use webview_auth::WebviewAuthenticator;
