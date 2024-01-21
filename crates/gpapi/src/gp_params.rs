use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::GP_USER_AGENT;

#[derive(Debug, Serialize, Deserialize, Clone, Type, Default)]
pub enum ClientOs {
  #[default]
  Linux,
  Windows,
  Mac,
}

impl From<&str> for ClientOs {
  fn from(os: &str) -> Self {
    match os {
      "Linux" => ClientOs::Linux,
      "Windows" => ClientOs::Windows,
      "Mac" => ClientOs::Mac,
      _ => ClientOs::Linux,
    }
  }
}

impl ClientOs {
  pub fn as_str(&self) -> &str {
    match self {
      ClientOs::Linux => "Linux",
      ClientOs::Windows => "Windows",
      ClientOs::Mac => "Mac",
    }
  }

  pub fn to_openconnect_os(&self) -> &str {
    match self {
      ClientOs::Linux => "linux",
      ClientOs::Windows => "win",
      ClientOs::Mac => "mac-intel",
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Default)]
pub struct GpParams {
  user_agent: String,
  client_os: ClientOs,
  os_version: Option<String>,
  client_version: Option<String>,
  computer: Option<String>,
  ignore_tls_errors: bool,
}

impl GpParams {
  pub fn builder() -> GpParamsBuilder {
    GpParamsBuilder::new()
  }

  pub(crate) fn user_agent(&self) -> &str {
    &self.user_agent
  }

  pub(crate) fn computer(&self) -> &str {
    match self.computer {
      Some(ref computer) => computer,
      None => self.client_os.as_str(),
    }
  }

  pub fn ignore_tls_errors(&self) -> bool {
    self.ignore_tls_errors
  }

  pub(crate) fn to_params(&self) -> HashMap<&str, &str> {
    let mut params: HashMap<&str, &str> = HashMap::new();
    let client_os = self.client_os.as_str();

    // Common params
    params.insert("prot", "https:");
    params.insert("jnlpReady", "jnlpReady");
    params.insert("ok", "Login");
    params.insert("direct", "yes");
    params.insert("ipv6-support", "yes");
    params.insert("inputStr", "");
    params.insert("clientVer", "4100");

    params.insert("clientos", client_os);

    if let Some(computer) = &self.computer {
      params.insert("computer", computer);
    } else {
      params.insert("computer", client_os);
    }

    if let Some(os_version) = &self.os_version {
      params.insert("os-version", os_version);
    }

    if let Some(client_version) = &self.client_version {
      params.insert("clientgpversion", client_version);
    }

    params
  }
}

pub struct GpParamsBuilder {
  user_agent: String,
  client_os: ClientOs,
  os_version: Option<String>,
  client_version: Option<String>,
  computer: Option<String>,
  ignore_tls_errors: bool,
}

impl GpParamsBuilder {
  pub fn new() -> Self {
    Self {
      user_agent: GP_USER_AGENT.to_string(),
      client_os: ClientOs::Linux,
      os_version: Default::default(),
      client_version: Default::default(),
      computer: Default::default(),
      ignore_tls_errors: false,
    }
  }

  pub fn user_agent(&mut self, user_agent: &str) -> &mut Self {
    self.user_agent = user_agent.to_string();
    self
  }

  pub fn client_os(&mut self, client_os: ClientOs) -> &mut Self {
    self.client_os = client_os;
    self
  }

  pub fn os_version<T: Into<Option<String>>>(&mut self, os_version: T) -> &mut Self {
    self.os_version = os_version.into();
    self
  }

  pub fn client_version<T: Into<Option<String>>>(&mut self, client_version: T) -> &mut Self {
    self.client_version = client_version.into();
    self
  }

  pub fn computer(&mut self, computer: &str) -> &mut Self {
    self.computer = Some(computer.to_string());
    self
  }

  pub fn ignore_tls_errors(&mut self, ignore_tls_errors: bool) -> &mut Self {
    self.ignore_tls_errors = ignore_tls_errors;
    self
  }

  pub fn build(&self) -> GpParams {
    GpParams {
      user_agent: self.user_agent.clone(),
      client_os: self.client_os.clone(),
      os_version: self.os_version.clone(),
      client_version: self.client_version.clone(),
      computer: self.computer.clone(),
      ignore_tls_errors: self.ignore_tls_errors,
    }
  }
}

impl Default for GpParamsBuilder {
  fn default() -> Self {
    Self::new()
  }
}
