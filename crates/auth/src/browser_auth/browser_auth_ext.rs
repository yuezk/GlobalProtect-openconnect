use std::future::Future;

use gpapi::auth::SamlAuthData;

use crate::{browser_auth::browser_auth_impl::BrowserAuthenticatorImpl, Authenticator};

pub trait BrowserAuthenticator {
  fn browser_authenticate(&self, browser: Option<&str>) -> impl Future<Output = anyhow::Result<SamlAuthData>> + Send;
}

impl BrowserAuthenticator for Authenticator<'_> {
  async fn browser_authenticate(&self, browser: Option<&str>) -> anyhow::Result<SamlAuthData> {
    let auth_request = self.initial_auth_request().await?;
    let browser_auth = if let Some(browser) = browser {
      BrowserAuthenticatorImpl::new_with_browser(&auth_request, browser)
    } else {
      BrowserAuthenticatorImpl::new(&auth_request)
    };

    browser_auth.authenticate().await
  }
}
