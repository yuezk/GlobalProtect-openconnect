use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{gateway::Gateway, gp_params::ClientOs};

use super::vpn_state::ConnectInfo;

#[derive(Debug, Deserialize, Serialize)]
pub struct LaunchGuiRequest {
  user: String,
  envs: HashMap<String, String>,
}

impl LaunchGuiRequest {
  pub fn new(user: String, envs: HashMap<String, String>) -> Self {
    Self { user, envs }
  }

  pub fn user(&self) -> &str {
    &self.user
  }

  pub fn envs(&self) -> &HashMap<String, String> {
    &self.envs
  }
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct ConnectArgs {
  cookie: String,
  vpnc_script: Option<String>,
  user_agent: Option<String>,
  os: Option<ClientOs>,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
  csd_uid: u32,
  csd_wrapper: Option<String>,
  reconnect_timeout: u32,
  mtu: u32,
  disable_ipv6: bool,
  no_dtls: bool,
}

impl ConnectArgs {
  pub fn new(cookie: String) -> Self {
    Self {
      cookie,
      vpnc_script: None,
      user_agent: None,
      os: None,
      certificate: None,
      sslkey: None,
      key_password: None,
      csd_uid: 0,
      csd_wrapper: None,
      reconnect_timeout: 300,
      mtu: 0,
      disable_ipv6: false,
      no_dtls: false,
    }
  }

  pub fn cookie(&self) -> &str {
    &self.cookie
  }

  pub fn vpnc_script(&self) -> Option<String> {
    self.vpnc_script.clone()
  }

  pub fn user_agent(&self) -> Option<String> {
    self.user_agent.clone()
  }

  pub fn openconnect_os(&self) -> Option<String> {
    self.os.as_ref().map(|os| os.to_openconnect_os().to_string())
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

  pub fn csd_uid(&self) -> u32 {
    self.csd_uid
  }

  pub fn csd_wrapper(&self) -> Option<String> {
    self.csd_wrapper.clone()
  }

  pub fn reconnect_timeout(&self) -> u32 {
    self.reconnect_timeout
  }

  pub fn mtu(&self) -> u32 {
    self.mtu
  }

  pub fn disable_ipv6(&self) -> bool {
    self.disable_ipv6
  }

  pub fn no_dtls(&self) -> bool {
    self.no_dtls
  }
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct ConnectRequest {
  info: ConnectInfo,
  args: ConnectArgs,
}

impl ConnectRequest {
  pub fn new(info: ConnectInfo, cookie: String) -> Self {
    Self {
      info,
      args: ConnectArgs::new(cookie),
    }
  }

  pub fn with_vpnc_script<T: Into<Option<String>>>(mut self, vpnc_script: T) -> Self {
    self.args.vpnc_script = vpnc_script.into();
    self
  }

  pub fn with_csd_uid(mut self, csd_uid: u32) -> Self {
    self.args.csd_uid = csd_uid;
    self
  }

  pub fn with_csd_wrapper<T: Into<Option<String>>>(mut self, csd_wrapper: T) -> Self {
    self.args.csd_wrapper = csd_wrapper.into();
    self
  }

  pub fn with_user_agent<T: Into<Option<String>>>(mut self, user_agent: T) -> Self {
    self.args.user_agent = user_agent.into();
    self
  }

  pub fn with_os<T: Into<Option<ClientOs>>>(mut self, os: T) -> Self {
    self.args.os = os.into();
    self
  }

  pub fn with_certificate<T: Into<Option<String>>>(mut self, certificate: T) -> Self {
    self.args.certificate = certificate.into();
    self
  }

  pub fn with_sslkey<T: Into<Option<String>>>(mut self, sslkey: T) -> Self {
    self.args.sslkey = sslkey.into();
    self
  }

  pub fn with_key_password<T: Into<Option<String>>>(mut self, key_password: T) -> Self {
    self.args.key_password = key_password.into();
    self
  }

  pub fn with_reconnect_timeout(mut self, reconnect_timeout: u32) -> Self {
    self.args.reconnect_timeout = reconnect_timeout;
    self
  }

  pub fn with_mtu(mut self, mtu: u32) -> Self {
    self.args.mtu = mtu;
    self
  }

  pub fn with_disable_ipv6(mut self, disable_ipv6: bool) -> Self {
    self.args.disable_ipv6 = disable_ipv6;
    self
  }

  pub fn with_no_dtls(mut self, no_dtls: bool) -> Self {
    self.args.no_dtls = no_dtls;
    self
  }

  pub fn gateway(&self) -> &Gateway {
    self.info.gateway()
  }

  pub fn info(&self) -> &ConnectInfo {
    &self.info
  }

  pub fn args(&self) -> &ConnectArgs {
    &self.args
  }
}

#[derive(Debug, Deserialize, Serialize, Type)]
pub struct DisconnectRequest;

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateLogLevelRequest(pub String);

/// Requests that can be sent to the service
#[derive(Debug, Deserialize, Serialize)]
pub enum WsRequest {
  Connect(Box<ConnectRequest>),
  Disconnect(DisconnectRequest),
  UpdateLogLevel(UpdateLogLevelRequest),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateGuiRequest {
  pub path: String,
  pub checksum: String,
}
