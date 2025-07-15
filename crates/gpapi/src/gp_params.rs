use std::collections::HashMap;

use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{utils::request::create_identity, GP_USER_AGENT};

#[derive(Debug, Serialize, Deserialize, Clone, Type, Default)]
pub enum ClientOs {
  #[cfg_attr(not(target_os = "macos"), default)]
  Linux,
  Windows,
  #[cfg_attr(target_os = "macos", default)]
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

#[derive(Debug, Serialize, Deserialize, Type, Default, Clone)]
pub struct GpParams {
  is_gateway: bool,
  user_agent: String,
  client_os: ClientOs,
  os_version: Option<String>,
  client_version: Option<String>,
  computer: String,
  ignore_tls_errors: bool,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
  // Used for MFA
  input_str: Option<String>,
  otp: Option<String>,
}

impl GpParams {
  pub fn builder() -> GpParamsBuilder {
    GpParamsBuilder::new()
  }

  pub(crate) fn is_gateway(&self) -> bool {
    self.is_gateway
  }

  pub fn set_is_gateway(&mut self, is_gateway: bool) {
    self.is_gateway = is_gateway;
  }

  pub(crate) fn user_agent(&self) -> &str {
    &self.user_agent
  }

  pub(crate) fn computer(&self) -> &str {
    &self.computer
  }

  pub fn ignore_tls_errors(&self) -> bool {
    self.ignore_tls_errors
  }

  pub fn client_os(&self) -> &str {
    self.client_os.as_str()
  }

  pub fn os_version(&self) -> Option<&str> {
    self.os_version.as_deref()
  }

  pub fn client_version(&self) -> Option<&str> {
    self.client_version.as_deref()
  }

  pub fn set_input_str(&mut self, input_str: &str) {
    self.input_str = Some(input_str.to_string());
  }

  pub fn set_otp(&mut self, otp: &str) {
    self.otp = Some(otp.to_string());
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
    params.insert("clientVer", "4100");
    params.insert("clientos", client_os);
    params.insert("computer", &self.computer);

    // MFA
    params.insert("inputStr", self.input_str.as_deref().unwrap_or_default());
    if let Some(otp) = &self.otp {
      params.insert("passwd", otp);
    }

    if let Some(os_version) = &self.os_version {
      params.insert("os-version", os_version);
    }

    // Include clientgpversion to satisfy server version requirements
    if let Some(client_version) = &self.client_version {
      params.insert("clientgpversion", client_version);
    }

    params
  }
}

pub struct GpParamsBuilder {
  is_gateway: bool,
  user_agent: String,
  client_os: ClientOs,
  os_version: Option<String>,
  client_version: Option<String>,
  computer: String,
  ignore_tls_errors: bool,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
}

impl GpParamsBuilder {
  pub fn new() -> Self {
    let computer = whoami::fallible::hostname().unwrap_or_else(|_| String::from("localhost"));

    Self {
      is_gateway: false,
      user_agent: GP_USER_AGENT.to_string(),
      client_os: ClientOs::Linux,
      os_version: Default::default(),
      client_version: Default::default(),
      computer,
      ignore_tls_errors: false,
      certificate: Default::default(),
      sslkey: Default::default(),
      key_password: Default::default(),
    }
  }

  pub fn is_gateway(&mut self, is_gateway: bool) -> &mut Self {
    self.is_gateway = is_gateway;
    self
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
    self.computer = computer.to_string();
    self
  }

  pub fn ignore_tls_errors(&mut self, ignore_tls_errors: bool) -> &mut Self {
    self.ignore_tls_errors = ignore_tls_errors;
    self
  }

  pub fn certificate<T: Into<Option<String>>>(&mut self, certificate: T) -> &mut Self {
    self.certificate = certificate.into();
    self
  }

  pub fn sslkey<T: Into<Option<String>>>(&mut self, sslkey: T) -> &mut Self {
    self.sslkey = sslkey.into();
    self
  }

  pub fn key_password<T: Into<Option<String>>>(&mut self, password: T) -> &mut Self {
    self.key_password = password.into();
    self
  }

  pub fn build(&self) -> GpParams {
    GpParams {
      is_gateway: self.is_gateway,
      user_agent: self.user_agent.clone(),
      client_os: self.client_os.clone(),
      os_version: self.os_version.clone(),
      client_version: self.client_version.clone(),
      computer: self.computer.clone(),
      ignore_tls_errors: self.ignore_tls_errors,
      certificate: self.certificate.clone(),
      sslkey: self.sslkey.clone(),
      key_password: self.key_password.clone(),
      input_str: Default::default(),
      otp: Default::default(),
    }
  }
}

impl Default for GpParamsBuilder {
  fn default() -> Self {
    Self::new()
  }
}

impl TryFrom<&GpParams> for Client {
  type Error = anyhow::Error;

  fn try_from(value: &GpParams) -> Result<Self, Self::Error> {
    let mut builder = Client::builder()
      .danger_accept_invalid_certs(value.ignore_tls_errors)
      .user_agent(&value.user_agent);

    if let Some(cert) = value.certificate.as_deref() {
      info!("Using client certificate authentication...");
      let identity = create_identity(cert, value.sslkey.as_deref(), value.key_password.as_deref())?;
      builder = builder.identity(identity);
    }

    let client = builder.build()?;
    Ok(client)
  }
}
