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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuthenticatedGateway {
  pub gateway: Gateway,
  pub authentication: GatewayAuthentication,
  pub binding: Option<super::network::GatewayBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GatewayFailure {
  AuthenticationRejected,
  Unreachable,
  TunnelFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct GatewayFailureSummary {
  pub gateway: Gateway,
  pub failure: GatewayFailure,
}

/// Credentials remain in the interactive authentication owner; only issued
/// gateway cookies cross the runtime/service boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(try_from = "ConnectionPlanFields")]
pub struct ConnectionPlan {
  members: Vec<AuthenticatedGateway>,
  failures: Vec<GatewayFailureSummary>,
  policy: InternalSessionPolicy,
  internal_detection: Option<super::network::InternalHostDetection>,
}

#[derive(Deserialize)]
struct ConnectionPlanFields {
  members: Vec<AuthenticatedGateway>,
  failures: Vec<GatewayFailureSummary>,
  policy: InternalSessionPolicy,
  internal_detection: Option<super::network::InternalHostDetection>,
}

impl TryFrom<ConnectionPlanFields> for ConnectionPlan {
  type Error = anyhow::Error;

  fn try_from(fields: ConnectionPlanFields) -> Result<Self, Self::Error> {
    Ok(Self::new(fields.members, fields.failures, fields.policy)?.with_internal_detection(fields.internal_detection))
  }
}

impl ConnectionPlan {
  pub fn new(
    members: Vec<AuthenticatedGateway>,
    failures: Vec<GatewayFailureSummary>,
    policy: InternalSessionPolicy,
  ) -> anyhow::Result<Self> {
    ensure!(!members.is_empty(), "Connection plan has no authenticated gateways");
    ensure!(
      members.len() <= 64 && failures.len() <= 64,
      "Connection plan exceeds its gateway limit"
    );
    let mode = members[0].authentication.mode();
    ensure!(
      members.iter().all(|member| member.authentication.mode() == mode),
      "Mixed tunnel and non-tunnel sessions are unsupported"
    );
    ensure!(
      mode != SessionMode::NonTunnel
        || members
          .iter()
          .all(|member| member.gateway.kind() == crate::gateway::GatewayKind::Internal),
      "Non-tunnel sessions require freshly discovered internal gateways"
    );
    ensure!(
      members.len() == 1
        || members
          .iter()
          .all(|member| member.gateway.kind() == crate::gateway::GatewayKind::Internal),
      "Multiple authenticated gateways require an internal connection"
    );
    let mut hosts = std::collections::HashSet::new();
    for member in &members {
      if mode == SessionMode::NonTunnel {
        let binding = member
          .binding
          .as_ref()
          .ok_or_else(|| anyhow::anyhow!("Non-tunnel session has no physical binding"))?;
        binding.validate_server(member.gateway.server())?;
      }
      let host = crate::utils::normalize_server(member.gateway.server())?;
      ensure!(hosts.insert(host), "Connection plan has duplicate gateways");
    }
    Ok(Self {
      members,
      failures,
      policy,
      internal_detection: None,
    })
  }

  pub fn members(&self) -> &[AuthenticatedGateway] {
    &self.members
  }

  pub fn failures(&self) -> &[GatewayFailureSummary] {
    &self.failures
  }

  pub fn policy(&self) -> InternalSessionPolicy {
    self.policy
  }

  pub fn mode(&self) -> SessionMode {
    self.members[0].authentication.mode()
  }

  pub fn with_internal_detection(mut self, detection: Option<super::network::InternalHostDetection>) -> Self {
    self.internal_detection = detection;
    self
  }

  pub fn internal_detection(&self) -> Option<&super::network::InternalHostDetection> {
    self.internal_detection.as_ref()
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn deserialization_enforces_authentication_invariants() {
    for (cookie, mode) in [
      ("", "tunnel"),
      ("not-a-cookie", "tunnel"),
      ("user=alice", "tunnel"),
      ("authcookie=token", "tunnel"),
      ("authcookie=token&user=alice&user=bob", "tunnel"),
      ("authcookie=token&user=alice", ""),
    ] {
      let serialized = serde_json::json!({"cookie":cookie, "connection_type":mode});
      assert!(serde_json::from_value::<GatewayAuthentication>(serialized).is_err());
    }
    let auth = GatewayAuthentication::new("authcookie=token&user=alice".into(), "non-tunnel".into()).unwrap();
    let decoded: GatewayAuthentication = serde_json::from_str(&serde_json::to_string(&auth).unwrap()).unwrap();
    assert_eq!(decoded.mode(), SessionMode::NonTunnel);
  }

  #[test]
  fn deserialization_enforces_schedule_invariants() {
    for interval in [0, u32::MAX] {
      let serialized = serde_json::json!({"hipIntervalSecs":interval,"collectHipData":false});
      assert!(serde_json::from_value::<InternalSessionPolicy>(serialized).is_err());
    }
    let serialized = serde_json::to_string(&InternalSessionPolicy::default()).unwrap();
    let decoded: InternalSessionPolicy = serde_json::from_str(&serialized).unwrap();
    assert_eq!(decoded.hip_interval(), Duration::from_secs(3600));
  }
}
