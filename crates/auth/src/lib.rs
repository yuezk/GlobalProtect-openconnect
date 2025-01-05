use anyhow::bail;
use gpapi::{
  gp_params::GpParams,
  portal::{prelogin, Prelogin},
};

#[cfg(feature = "browser-auth")]
mod browser;

#[cfg(feature = "browser-auth")]
pub use browser::*;

#[cfg(feature = "webview-auth")]
mod webview;
#[cfg(feature = "webview-auth")]
pub use webview::*;

pub async fn auth_prelogin(server: &str, gp_params: &GpParams) -> anyhow::Result<String> {
  match prelogin(server, gp_params).await? {
    Prelogin::Saml(prelogin) => Ok(prelogin.saml_request().to_string()),
    Prelogin::Standard(_) => bail!("Received non-SAML prelogin response"),
  }
}
