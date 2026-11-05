use super::*;
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AuthenticatedGateway {
  pub gateway: Gateway,
  pub authentication: GatewayAuthentication,
  pub binding: Option<crate::session::desktop::network::GatewayBinding>,
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
  internal_detection: Option<crate::session::InternalHostDetection>,
}

#[derive(Deserialize)]
struct ConnectionPlanFields {
  members: Vec<AuthenticatedGateway>,
  failures: Vec<GatewayFailureSummary>,
  policy: InternalSessionPolicy,
  internal_detection: Option<crate::session::InternalHostDetection>,
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

  pub fn with_internal_detection(mut self, detection: Option<crate::session::InternalHostDetection>) -> Self {
    self.internal_detection = detection;
    self
  }

  pub fn internal_detection(&self) -> Option<&crate::session::InternalHostDetection> {
    self.internal_detection.as_ref()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

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
