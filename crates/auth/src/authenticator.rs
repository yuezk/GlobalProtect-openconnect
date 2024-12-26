use std::borrow::Cow;

use anyhow::bail;
use gpapi::{
  gp_params::GpParams,
  portal::{prelogin, Prelogin},
};

pub struct Authenticator<'a> {
  server: &'a str,
  auth_request: Option<&'a str>,
  pub(crate) gp_params: &'a GpParams,

  #[cfg(feature = "webview-auth")]
  pub(crate) clean: bool,
  #[cfg(feature = "webview-auth")]
  pub(crate) is_retrying: tokio::sync::RwLock<bool>,
}

impl<'a> Authenticator<'a> {
  pub fn new(server: &'a str, gp_params: &'a GpParams) -> Self {
    Self {
      server,
      gp_params,
      auth_request: None,

      #[cfg(feature = "webview-auth")]
      clean: false,
      #[cfg(feature = "webview-auth")]
      is_retrying: Default::default(),
    }
  }

  pub fn with_auth_request(mut self, auth_request: &'a str) -> Self {
    if !auth_request.is_empty() {
      self.auth_request = Some(auth_request);
    }
    self
  }

  pub(crate) async fn initial_auth_request(&self) -> anyhow::Result<Cow<'a, str>> {
    if let Some(auth_request) = self.auth_request {
      return Ok(Cow::Borrowed(auth_request));
    }

    let auth_request = self.portal_prelogin().await?;
    Ok(Cow::Owned(auth_request))
  }

  pub(crate) async fn portal_prelogin(&self) -> anyhow::Result<String> {
    auth_prelogin(self.server, self.gp_params).await
  }
}

pub async fn auth_prelogin(server: &str, gp_params: &GpParams) -> anyhow::Result<String> {
  match prelogin(server, gp_params).await? {
    Prelogin::Saml(prelogin) => Ok(prelogin.saml_request().to_string()),
    Prelogin::Standard(_) => bail!("Received non-SAML prelogin response"),
  }
}
