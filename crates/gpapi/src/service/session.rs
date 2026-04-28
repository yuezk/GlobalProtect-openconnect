use serde::{Deserialize, Serialize};
use specta::Type;

use crate::gp_params::ClientOs;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionWarning {
  pub prior_secs: u32,
  pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SessionRequestArgs {
  cookie: String,
  user_agent: Option<String>,
  os: Option<ClientOs>,
  os_version: Option<String>,
  client_version: Option<String>,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
  disable_ipv6: bool,
}

impl SessionRequestArgs {
  pub fn new(cookie: String) -> Self {
    Self {
      cookie,
      user_agent: None,
      os: None,
      os_version: None,
      client_version: None,
      certificate: None,
      sslkey: None,
      key_password: None,
      disable_ipv6: false,
    }
  }

  pub fn with_user_agent<T: Into<Option<String>>>(mut self, user_agent: T) -> Self {
    self.user_agent = user_agent.into();
    self
  }

  pub fn with_os<T: Into<Option<ClientOs>>>(mut self, os: T) -> Self {
    self.os = os.into();
    self
  }

  pub fn with_os_version<T: Into<Option<String>>>(mut self, os_version: T) -> Self {
    self.os_version = os_version.into();
    self
  }

  pub fn with_client_version<T: Into<Option<String>>>(mut self, client_version: T) -> Self {
    self.client_version = client_version.into();
    self
  }

  pub fn with_certificate<T: Into<Option<String>>>(mut self, certificate: T) -> Self {
    self.certificate = certificate.into();
    self
  }

  pub fn with_sslkey<T: Into<Option<String>>>(mut self, sslkey: T) -> Self {
    self.sslkey = sslkey.into();
    self
  }

  pub fn with_key_password<T: Into<Option<String>>>(mut self, key_password: T) -> Self {
    self.key_password = key_password.into();
    self
  }

  pub fn with_disable_ipv6(mut self, disable_ipv6: bool) -> Self {
    self.disable_ipv6 = disable_ipv6;
    self
  }

  pub fn cookie(&self) -> &str {
    &self.cookie
  }

  pub fn user_agent(&self) -> Option<String> {
    self.user_agent.clone()
  }

  pub fn os(&self) -> Option<ClientOs> {
    self.os.clone()
  }

  pub fn os_version(&self) -> Option<String> {
    self.os_version.clone()
  }

  pub fn client_version(&self) -> Option<String> {
    self.client_version.clone()
  }

  pub fn certificate(&self) -> Option<String> {
    self.certificate.clone()
  }

  pub fn sslkey(&self) -> Option<String> {
    self.sslkey.clone()
  }

  pub fn key_password(&self) -> Option<String> {
    self.key_password.clone()
  }

  pub fn disable_ipv6(&self) -> bool {
    self.disable_ipv6
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
  pub lifetime_secs: Option<u32>,
  pub user_expires: Option<u32>,
  pub expires_in_human: Option<String>,
  pub lifetime_warning: Option<SessionWarning>,
  pub inactivity_warning: Option<SessionWarning>,
  pub admin_logout_message: Option<String>,
  pub allow_extend_session: bool,
}
