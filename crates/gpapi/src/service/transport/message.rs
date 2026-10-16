use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::hip::HipApprovalStatus;
use crate::service::{event::WsEvent, request::WsRequest, vpn_env::VpnEnv, vpnc_script::VpncScriptMetadata};

use super::{MAX_PLAINTEXT, MAX_SERVICE_MESSAGE, ServiceErrorCode, TransportError};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase", deny_unknown_fields)]
pub enum ClientMessage {
  Attach { last_event_revision: Option<u64> },
  Request { id: Uuid, request: WsRequest },
  Resync { last_event_revision: u64 },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase", deny_unknown_fields)]
pub enum ServerMessage {
  TunnelStatistics {
    snapshot: crate::session::tunnel::TunnelStatisticsSnapshot,
  },
  Attached {
    connection_id: Uuid,
    current_revision: u64,
    snapshot: VpnEnv,
  },
  Reply {
    id: Uuid,
    result: ServiceResult,
  },
  Event {
    revision: u64,
    event: WsEvent,
  },
  Snapshot {
    current_revision: u64,
    snapshot: VpnEnv,
  },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase", deny_unknown_fields)]
pub enum ServiceResult {
  TunnelStatisticsSnapshot(crate::session::tunnel::TunnelStatisticsSnapshot),
  Accepted,
  StoredHipReport {
    report_id: String,
  },
  HipUploadProgress {
    upload_id: String,
  },
  HipPreviewChunk {
    preview_id: String,
    offset: usize,
    xml: String,
    complete: bool,
  },
  HipApprovalStatus(HipApprovalStatus),
  NoSubmittedHipReport,
  VpncScriptMetadata(Option<VpncScriptMetadata>),
  Rejected(ServiceRejection),
}

impl ServiceResult {
  pub fn rejected(code: ServiceErrorCode, message: impl Into<String>) -> Self {
    Self::Rejected(ServiceRejection::new(code, message))
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "ServiceRejectionDto")]
pub struct ServiceRejection {
  code: ServiceErrorCode,
  message: String,
}

impl ServiceRejection {
  fn new(code: ServiceErrorCode, message: impl Into<String>) -> Self {
    let mut message = message.into();
    if message.len() > MAX_SERVICE_MESSAGE {
      let mut boundary = MAX_SERVICE_MESSAGE;
      while !message.is_char_boundary(boundary) {
        boundary -= 1;
      }
      message.truncate(boundary);
    }
    Self { code, message }
  }

  pub fn code(&self) -> ServiceErrorCode {
    self.code
  }

  pub fn message(&self) -> &str {
    &self.message
  }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceRejectionDto {
  code: ServiceErrorCode,
  message: String,
}

impl TryFrom<ServiceRejectionDto> for ServiceRejection {
  type Error = &'static str;

  fn try_from(value: ServiceRejectionDto) -> Result<Self, Self::Error> {
    if value.message.len() > MAX_SERVICE_MESSAGE {
      return Err("service rejection message is too long");
    }
    Ok(Self {
      code: value.code,
      message: value.message,
    })
  }
}

pub(crate) fn encode<T: Serialize>(message: &T) -> Result<Vec<u8>, TransportError> {
  let encoded = serde_json::to_vec(message)?;
  if encoded.len() > MAX_PLAINTEXT {
    return Err(TransportError::MessageTooLarge);
  }
  Ok(encoded)
}

pub(crate) fn decode<T: for<'de> Deserialize<'de>>(encoded: &[u8]) -> Result<T, TransportError> {
  if encoded.len() > MAX_PLAINTEXT {
    return Err(TransportError::MessageTooLarge);
  }
  Ok(serde_json::from_slice(encoded)?)
}

#[cfg(test)]
mod tests {
  use crate::{
    gateway::Gateway,
    service::{
      request::{ConnectRequest, MAX_CLIENT_IDENTITY_DATA, WsRequest},
      vpn_state::ConnectInfo,
    },
  };

  use super::*;

  #[test]
  fn rejects_oversized_messages() {
    assert!(encode(&"x".repeat(MAX_PLAINTEXT)).is_err());
    assert!(decode::<String>(&vec![b'x'; MAX_PLAINTEXT + 1]).is_err());
  }

  #[test]
  fn rejection_messages_are_utf8_safely_bounded() {
    let ServiceResult::Rejected(rejection) = ServiceResult::rejected(ServiceErrorCode::Internal, "界".repeat(100))
    else {
      panic!("expected rejection");
    };
    assert!(rejection.message().len() <= MAX_SERVICE_MESSAGE);

    let oversized = format!(
      r#"{{"type":"rejected","data":{{"code":"internal","message":"{}"}}}}"#,
      "x".repeat(MAX_SERVICE_MESSAGE + 1)
    );
    assert!(serde_json::from_str::<ServiceResult>(&oversized).is_err());
  }

  #[test]
  fn maximum_client_identity_fits_one_transport_message() {
    let gateway = Gateway::new("Gateway".to_string(), "vpn.example.com".to_string());
    let info = ConnectInfo::new("portal.example.com".to_string(), gateway.clone(), vec![gateway]);
    let certificate_size = MAX_CLIENT_IDENTITY_DATA / 2;
    let plan = crate::session::ConnectionPlan::new(
      vec![crate::session::AuthenticatedGateway {
        gateway: info.gateway().clone(),
        binding: Some(crate::session::network::test_binding()),
        authentication: crate::session::GatewayAuthentication::new(
          "authcookie=token&user=user".into(),
          "tunnel".into(),
        )
        .unwrap(),
      }],
      vec![],
      crate::session::InternalSessionPolicy::default(),
    )
    .unwrap();
    let request = ConnectRequest::new(info, plan)
      .with_certificate_data(Some(vec![b'c'; certificate_size]))
      .with_sslkey_data(Some(vec![b'k'; MAX_CLIENT_IDENTITY_DATA - certificate_size]));
    let message = ClientMessage::Request {
      id: Uuid::new_v4(),
      request: WsRequest::Connect(Box::new(request)),
    };

    let encoded = encode(&message).unwrap();
    assert!(encoded.len() <= MAX_PLAINTEXT);
  }

  #[test]
  fn full_configuration_and_large_metadata_fit_complete_snapshot_envelopes_at_the_budget() {
    use crate::{
      os_profile::HostIdentity,
      service::{
        vpn_env::HostInfo,
        vpn_state::{ConnectedInfo, VpnState},
      },
      session::tunnel::{MAX_CONFIGURATION_BYTES, TunnelConfiguration, TunnelPhase, TunnelState, TunnelTransport},
    };
    let mut configuration = TunnelConfiguration::default();
    configuration.included_routes.items = vec!["r".repeat(1024); 15];
    configuration.included_routes.items.push(String::new());
    configuration.included_routes.total_count = 16;
    let padding = MAX_CONFIGURATION_BYTES - serde_json::to_vec(&configuration).unwrap().len();
    assert!(padding <= 1024);
    *configuration.included_routes.items.last_mut().unwrap() = "r".repeat(padding);
    assert_eq!(
      serde_json::to_vec(&configuration).unwrap().len(),
      MAX_CONFIGURATION_BYTES
    );
    let gateway = Gateway::new("Gateway".repeat(20), "vpn.example.com".into());
    let info = ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![gateway; 32]);
    let snapshot = VpnEnv {
      tunnel_state: Some(TunnelState {
        tunnel_id: Uuid::new_v4(),
        sequence: u64::MAX,
        phase: TunnelPhase::Connected,
        configuration_revision: u64::MAX,
        configuration: Some(configuration),
        transport: TunnelTransport::IpsecEsp,
        elapsed_ms: u64::MAX,
      }),
      vpn_state: VpnState::Connected(Box::new(ConnectedInfo::new(info, None))),
      vpnc_script: Some("s".repeat(1024)),
      auth_executable: "a".repeat(1024),
      host_info: HostInfo {
        host_identity: HostIdentity::collect(),
      },
    };
    for mut message in [
      ServerMessage::Attached {
        connection_id: Uuid::new_v4(),
        current_revision: u64::MAX,
        snapshot: snapshot.clone(),
      },
      ServerMessage::Snapshot {
        current_revision: u64::MAX,
        snapshot,
      },
    ] {
      let remaining = MAX_PLAINTEXT - encode(&message).unwrap().len();
      let snapshot = match &mut message {
        ServerMessage::Attached { snapshot, .. } | ServerMessage::Snapshot { snapshot, .. } => snapshot,
        _ => unreachable!(),
      };
      // Existing host/connection metadata has no declared maximum. Fill the
      // remaining envelope budget, then assert its actual boundary explicitly.
      snapshot.auth_executable.push_str(&"a".repeat(remaining));
      assert_eq!(encode(&message).unwrap().len(), MAX_PLAINTEXT);
      let snapshot = match &mut message {
        ServerMessage::Attached { snapshot, .. } | ServerMessage::Snapshot { snapshot, .. } => snapshot,
        _ => unreachable!(),
      };
      snapshot.auth_executable.push('a');
      assert!(matches!(encode(&message), Err(TransportError::MessageTooLarge)));
    }
  }
}
