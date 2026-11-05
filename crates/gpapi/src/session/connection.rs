use std::{fmt, time::Duration};

use anyhow::ensure;
use serde::{Deserialize, Serialize};
use specta::Type;
use zeroize::Zeroize;

use crate::gateway::Gateway;

/// Network location and session mode are independent: an internal gateway can
/// still require a tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SessionMode {
  Tunnel,
  NonTunnel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MaintenanceState {
  Authenticated,
  Healthy,
  Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySessionSummary {
  pub gateway: Gateway,
  pub mode: SessionMode,
  pub client_ip: Option<String>,
  pub client_ipv6: Option<String>,
  pub maintenance: MaintenanceState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum SessionEndReason {
  InvalidCookie,
  NetworkChanged,
  AllMembersFailed,
  AuthorizationRevoked,
}

#[derive(Clone, Serialize, Deserialize, Type)]
#[serde(try_from = "GatewayAuthenticationFields")]
pub struct GatewayAuthentication {
  cookie: String,
  connection_type: String,
}

#[derive(Deserialize)]
struct GatewayAuthenticationFields {
  cookie: String,
  connection_type: String,
}

impl TryFrom<GatewayAuthenticationFields> for GatewayAuthentication {
  type Error = anyhow::Error;

  fn try_from(fields: GatewayAuthenticationFields) -> Result<Self, Self::Error> {
    Self::new(fields.cookie, fields.connection_type)
  }
}

impl GatewayAuthentication {
  pub fn new(cookie: String, connection_type: String) -> anyhow::Result<Self> {
    let authentication = Self {
      cookie,
      connection_type,
    };
    ensure!(
      !authentication.cookie.is_empty() && authentication.cookie.len() <= 64 * 1024,
      "Gateway authentication cookie is missing or too large"
    );
    let params = url::form_urlencoded::parse(authentication.cookie.as_bytes()).collect::<Vec<_>>();
    for key in ["authcookie", "user"] {
      let mut values = params.iter().filter(|(name, _)| name == key).map(|(_, value)| value);
      ensure!(
        values.next().is_some_and(|value| !value.is_empty()) && values.next().is_none(),
        "Gateway authentication requires one nonempty {key}"
      );
    }
    ensure!(
      !authentication.connection_type.trim().is_empty()
        && authentication.connection_type != "(null)"
        && authentication.connection_type != "-1",
      "Gateway connection type is missing"
    );
    Ok(authentication)
  }

  pub fn cookie(&self) -> &str {
    &self.cookie
  }

  pub fn connection_type(&self) -> &str {
    &self.connection_type
  }

  pub fn mode(&self) -> SessionMode {
    if self.connection_type == "tunnel" {
      SessionMode::Tunnel
    } else {
      SessionMode::NonTunnel
    }
  }
}

impl fmt::Debug for GatewayAuthentication {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("GatewayAuthentication")
      .field("cookie", &"<redacted>")
      .field("mode", &self.mode())
      .finish()
  }
}

impl Drop for GatewayAuthentication {
  fn drop(&mut self) {
    self.cookie.zeroize();
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", try_from = "InternalSessionPolicyFields")]
pub struct InternalSessionPolicy {
  hip_interval_secs: u32,
  collect_hip_data: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InternalSessionPolicyFields {
  hip_interval_secs: u32,
  collect_hip_data: bool,
}

impl TryFrom<InternalSessionPolicyFields> for InternalSessionPolicy {
  type Error = anyhow::Error;

  fn try_from(fields: InternalSessionPolicyFields) -> Result<Self, Self::Error> {
    Self::new(fields.hip_interval_secs, fields.collect_hip_data)
  }
}

impl Default for InternalSessionPolicy {
  fn default() -> Self {
    Self {
      hip_interval_secs: 3600,
      collect_hip_data: true,
    }
  }
}

impl InternalSessionPolicy {
  pub fn new(hip_interval_secs: u32, collect_hip_data: bool) -> anyhow::Result<Self> {
    ensure!(hip_interval_secs > 0, "HIP report interval must be positive");
    ensure!(
      hip_interval_secs.checked_mul(1000).is_some(),
      "HIP report interval exceeds the supported range"
    );
    Ok(Self {
      hip_interval_secs,
      collect_hip_data,
    })
  }

  pub fn hip_interval(&self) -> Duration {
    Duration::from_secs(u64::from(self.hip_interval_secs))
  }

  pub fn collect_hip_data(&self) -> bool {
    self.collect_hip_data
  }
}
