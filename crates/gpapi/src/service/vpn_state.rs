use serde::{Deserialize, Serialize};
use specta::Type;

use crate::gateway::Gateway;

#[derive(Debug, Deserialize, Serialize, Type, Clone)]
pub struct ConnectInfo {
  portal: String,
  gateway: Gateway,
  gateways: Vec<Gateway>,
}

impl ConnectInfo {
  pub fn new(portal: String, gateway: Gateway, gateways: Vec<Gateway>) -> Self {
    Self {
      portal,
      gateway,
      gateways,
    }
  }

  pub fn gateway(&self) -> &Gateway {
    &self.gateway
  }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum VpnState {
  Disconnected,
  Connecting(Box<ConnectInfo>),
  Connected(Box<ConnectInfo>),
  Disconnecting,
}
