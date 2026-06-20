use anyhow::bail;
use gpapi::{
  gp_params::GpParams,
  portal::{Prelogin, PreloginOptions, prelogin},
};

#[cfg(feature = "browser-auth")]
mod browser;

#[cfg(feature = "browser-auth")]
pub use browser::*;

#[cfg(feature = "webview-auth")]
mod webview;
#[cfg(feature = "webview-auth")]
pub use webview::*;

pub async fn auth_prelogin(
  server: &str,
  gp_params: &GpParams,
  external_browser_requested: bool,
) -> anyhow::Result<String> {
  // gpauth only performs the initial portal/gateway prelogin. Portal policy is
  // not available here, so gateway prelogin stays on the embedded value.
  let options = PreloginOptions::default().external_browser_requested(external_browser_requested);
  match prelogin(server, gp_params, options).await? {
    Prelogin::Saml(prelogin) => Ok(prelogin.saml_request().to_string()),
    Prelogin::Standard(_) => bail!("Received non-SAML prelogin response"),
  }
}
