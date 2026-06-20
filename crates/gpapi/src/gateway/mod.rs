mod login;
mod parse_gateways;
pub mod session;

pub use login::*;
pub(crate) use parse_gateways::*;
pub use session::*;

use serde::{Deserialize, Serialize};
use specta::Type;

use std::fmt::Display;

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
pub(crate) struct PriorityRule {
  pub(crate) name: String,
  pub(crate) priority: u32,
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Gateway {
  pub(crate) name: String,
  pub(crate) address: String,
  #[serde(default)]
  pub(crate) kind: GatewayKind,
  pub(crate) priority: u32,
  pub(crate) priority_rules: Vec<PriorityRule>,
}

#[derive(Debug, Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GatewayKind {
  Internal,
  External,
}

impl GatewayKind {
  pub fn as_login_param(self) -> &'static str {
    match self {
      GatewayKind::Internal => "yes",
      GatewayKind::External => "no",
    }
  }
}

impl Default for GatewayKind {
  fn default() -> Self {
    Self::External
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GatewaySelection {
  Auto,
  Manual,
}

impl GatewaySelection {
  pub fn as_login_param(self) -> &'static str {
    match self {
      GatewaySelection::Auto => "auto",
      GatewaySelection::Manual => "manual",
    }
  }
}

impl Default for GatewaySelection {
  fn default() -> Self {
    Self::Auto
  }
}

#[derive(Debug, Clone)]
pub struct GatewayLoginContext {
  host: String,
  name: String,
  kind: GatewayKind,
  selection: GatewaySelection,
  connect_method: Option<String>,
  client_ip: Option<String>,
}

impl GatewayLoginContext {
  pub fn new(gateway: &Gateway, selection: GatewaySelection) -> Self {
    let name = if gateway.name().is_empty() {
      gateway.server()
    } else {
      gateway.name()
    };

    Self {
      host: gateway.server().to_string(),
      name: name.to_string(),
      kind: gateway.kind(),
      selection,
      connect_method: None,
      client_ip: None,
    }
  }

  pub fn with_connect_method(mut self, connect_method: Option<&str>) -> Self {
    self.connect_method = connect_method.filter(|s| !s.is_empty()).map(|s| s.to_string());
    self
  }

  pub fn with_client_ip(mut self, client_ip: Option<String>) -> Self {
    self.client_ip = client_ip.filter(|s| !s.is_empty());
    self
  }

  pub(crate) fn host(&self) -> &str {
    &self.host
  }

  pub(crate) fn name(&self) -> &str {
    &self.name
  }

  pub(crate) fn kind(&self) -> GatewayKind {
    self.kind
  }

  pub(crate) fn selection(&self) -> GatewaySelection {
    self.selection
  }

  pub(crate) fn connect_method(&self) -> Option<&str> {
    self.connect_method.as_deref()
  }

  pub(crate) fn client_ip(&self) -> Option<&str> {
    self.client_ip.as_deref()
  }
}

impl Display for Gateway {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{} ({})", self.name, self.address)
  }
}

impl Gateway {
  pub fn new(name: String, address: String) -> Self {
    Self {
      name,
      address,
      kind: GatewayKind::External,
      priority: 0,
      priority_rules: vec![],
    }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn server(&self) -> &str {
    &self.address
  }

  pub fn kind(&self) -> GatewayKind {
    self.kind
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn deserializes_gateway_without_kind_as_external() {
    let gateway: Gateway = serde_json::from_str(
      r#"{
        "name": "US East",
        "address": "us-east.example.com",
        "priority": 1,
        "priorityRules": []
      }"#,
    )
    .unwrap();

    assert_eq!(gateway.kind(), GatewayKind::External);
  }
}
