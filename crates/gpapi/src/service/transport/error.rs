use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum TransportError {
  #[error("invalid credential frame")]
  InvalidCredential,
  #[error("invalid transport state")]
  InvalidState,
  #[error("message exceeds transport limit")]
  MessageTooLarge,
  #[error("malformed transport message")]
  MalformedMessage,
  #[error("Noise operation failed")]
  Noise(#[from] snow::Error),
  #[error("transport serialization failed")]
  Serialization(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum HandshakeRejection {
  VersionMismatch { client: String, service: String },
  ServiceRestarted { service_instance_id: Uuid },
  ReconnectSuperseded,
  Unauthorized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ServiceErrorCode {
  InvalidRequest,
  Busy,
  ChecksumFailed,
  InstallFailed,
  Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
  VersionMismatch,
  Unauthorized,
  SessionReplaced,
  HandshakeFailed,
  MalformedMessage,
  MessageTooLarge,
  AuthenticationFailed,
  HandshakeTimeout,
  ReconnectSuperseded,
  InternalError,
  ServiceRestarted,
  HeartbeatTimeout,
}

impl CloseReason {
  pub const fn code(self) -> u16 {
    match self {
      Self::VersionMismatch => 4001,
      Self::Unauthorized => 4002,
      Self::SessionReplaced => 4003,
      Self::HandshakeFailed => 4004,
      Self::MalformedMessage => 4005,
      Self::MessageTooLarge => 4006,
      Self::AuthenticationFailed => 4007,
      Self::HandshakeTimeout => 4008,
      Self::ReconnectSuperseded => 4009,
      Self::InternalError => 4010,
      Self::ServiceRestarted => 4011,
      Self::HeartbeatTimeout => 4012,
    }
  }

  pub const fn reason(self) -> &'static str {
    match self {
      Self::VersionMismatch => "VersionMismatch",
      Self::Unauthorized => "Unauthorized",
      Self::SessionReplaced => "SessionReplaced",
      Self::HandshakeFailed => "HandshakeFailed",
      Self::MalformedMessage => "MalformedMessage",
      Self::MessageTooLarge => "MessageTooLarge",
      Self::AuthenticationFailed => "AuthenticationFailed",
      Self::HandshakeTimeout => "HandshakeTimeout",
      Self::ReconnectSuperseded => "ReconnectSuperseded",
      Self::InternalError => "InternalError",
      Self::ServiceRestarted => "ServiceRestarted",
      Self::HeartbeatTimeout => "HeartbeatTimeout",
    }
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashSet;

  use super::*;

  #[test]
  fn close_codes_are_unique_application_codes() {
    let reasons = [
      CloseReason::VersionMismatch,
      CloseReason::Unauthorized,
      CloseReason::SessionReplaced,
      CloseReason::HandshakeFailed,
      CloseReason::MalformedMessage,
      CloseReason::MessageTooLarge,
      CloseReason::AuthenticationFailed,
      CloseReason::HandshakeTimeout,
      CloseReason::ReconnectSuperseded,
      CloseReason::InternalError,
      CloseReason::ServiceRestarted,
      CloseReason::HeartbeatTimeout,
    ];
    let codes: HashSet<_> = reasons.iter().map(|reason| reason.code()).collect();
    assert_eq!(codes.len(), reasons.len());
    assert!(codes.iter().all(|code| (4000..5000).contains(code)));
  }
}
