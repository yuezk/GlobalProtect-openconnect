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
  csd_uid: u32,
  csd_wrapper: Option<String>,
  mtu: u32,
  os: Option<ClientOs>,
}

impl ConnectArgs {
  pub fn new(cookie: String) -> Self {
    Self {
      cookie,
      vpnc_script: None,
      user_agent: None,
      os: None,
      csd_uid: 0,
      csd_wrapper: None,
      mtu: 0,
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

  pub fn csd_uid(&self) -> u32 {
    self.csd_uid
  }

  pub fn csd_wrapper(&self) -> Option<String> {
    self.csd_wrapper.clone()
  }

  pub fn mtu(&self) -> u32 {
    self.mtu
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

  pub fn with_mtu(mut self, mtu: u32) -> Self {
    self.args.mtu = mtu;
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

/// Requests that can be sent to the service
#[derive(Debug, Deserialize, Serialize)]
pub enum WsRequest {
  Connect(Box<ConnectRequest>),
  Disconnect(DisconnectRequest),
}

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateGuiRequest {
  pub path: String,
  pub checksum: String,
}
