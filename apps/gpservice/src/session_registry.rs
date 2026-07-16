use std::{
  collections::HashMap,
  sync::{Arc, Mutex, Weak},
  time::{Duration, Instant},
};

use gpapi::service::transport::SessionCredential;
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ws_connection::ConnectionControl;

const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_PENDING_SESSIONS: usize = 16;

pub struct HandshakePermit {
  pub session_id: Uuid,
  pub observed_generation: u64,
  pub secret: Zeroizing<[u8; 32]>,
  token: Uuid,
  registry: Weak<SessionRegistry>,
}

pub struct PreparedAttach {
  pub generation: u64,
}

#[derive(Debug, Error)]
pub enum SessionError {
  #[error("credential belongs to another service instance")]
  ServiceRestarted { service_instance_id: Uuid },
  #[error("session is unauthorized")]
  Unauthorized,
  #[error("too many pending sessions")]
  TooManyPending,
  #[error("another reconnect completed first")]
  ReconnectSuperseded,
  #[error("session registry is unavailable")]
  Unavailable,
}

enum SessionStatus {
  Pending {
    activation_deadline: Instant,
    handshake_token: Option<Uuid>,
  },
  Active {
    generation: u64,
    connection: Option<ConnectionControl>,
    reconnect_token: Option<Uuid>,
  },
}

struct SessionRecord {
  secret: Zeroizing<[u8; 32]>,
  product_version: String,
  status: SessionStatus,
}

pub struct SessionRegistry {
  service_instance_id: Uuid,
  sessions: Mutex<HashMap<Uuid, SessionRecord>>,
}

impl SessionRegistry {
  pub fn new(service_instance_id: Uuid) -> Self {
    Self {
      service_instance_id,
      sessions: Mutex::new(HashMap::new()),
    }
  }

  pub fn issue(&self, product_version: &str) -> Result<Arc<SessionCredential>, SessionError> {
    let mut sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let now = Instant::now();
    sessions.retain(|_, record| {
      !matches!(
        record.status,
        SessionStatus::Pending {
          activation_deadline, ..
        } if now > activation_deadline
      )
    });
    let pending = sessions
      .values()
      .filter(|record| matches!(record.status, SessionStatus::Pending { .. }))
      .count();
    if pending >= MAX_PENDING_SESSIONS {
      return Err(SessionError::TooManyPending);
    }

    let credential = Arc::new(
      SessionCredential::generate(self.service_instance_id, product_version).map_err(|_| SessionError::Unavailable)?,
    );
    sessions.insert(
      credential.session_id(),
      SessionRecord {
        secret: Zeroizing::new(*credential.secret()),
        product_version: credential.product_version().to_owned(),
        status: SessionStatus::Pending {
          activation_deadline: Instant::now() + ACTIVATION_TIMEOUT,
          handshake_token: None,
        },
      },
    );
    Ok(credential)
  }

  pub fn begin_handshake(
    self: &Arc<Self>,
    service_instance_id: Uuid,
    session_id: Uuid,
    product_version: &str,
  ) -> Result<HandshakePermit, SessionError> {
    if service_instance_id != self.service_instance_id {
      return Err(SessionError::ServiceRestarted {
        service_instance_id: self.service_instance_id,
      });
    }

    let mut sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let Some(record) = sessions.get_mut(&session_id) else {
      return Err(SessionError::Unauthorized);
    };
    if record.product_version != product_version {
      return Err(SessionError::Unauthorized);
    }

    let token = Uuid::new_v4();
    let observed_generation = match &mut record.status {
      SessionStatus::Pending {
        activation_deadline,
        handshake_token,
      } => {
        if Instant::now() > *activation_deadline {
          sessions.remove(&session_id);
          return Err(SessionError::Unauthorized);
        }
        if handshake_token.is_some() {
          return Err(SessionError::ReconnectSuperseded);
        }
        *handshake_token = Some(token);
        0
      }
      SessionStatus::Active {
        generation,
        reconnect_token,
        ..
      } => {
        if reconnect_token.is_some() {
          return Err(SessionError::ReconnectSuperseded);
        }
        *reconnect_token = Some(token);
        *generation
      }
    };

    let record = sessions.get(&session_id).ok_or(SessionError::Unauthorized)?;
    Ok(HandshakePermit {
      session_id,
      observed_generation,
      secret: Zeroizing::new(*record.secret),
      token,
      registry: Arc::downgrade(self),
    })
  }

  pub fn prepare_attach(&self, permit: &HandshakePermit) -> Result<PreparedAttach, SessionError> {
    let sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get(&permit.session_id).ok_or(SessionError::Unauthorized)?;
    let generation = match &record.status {
      SessionStatus::Pending { handshake_token, .. }
        if permit.observed_generation == 0 && *handshake_token == Some(permit.token) =>
      {
        1
      }
      SessionStatus::Active {
        generation,
        reconnect_token,
        ..
      } if *generation == permit.observed_generation && *reconnect_token == Some(permit.token) => {
        generation.checked_add(1).ok_or(SessionError::Unavailable)?
      }
      _ => return Err(SessionError::ReconnectSuperseded),
    };
    Ok(PreparedAttach { generation })
  }

  pub fn commit_attach(
    &self,
    permit: &HandshakePermit,
    connection: ConnectionControl,
  ) -> Result<Option<ConnectionControl>, SessionError> {
    let mut sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get_mut(&permit.session_id).ok_or(SessionError::Unauthorized)?;

    match &mut record.status {
      SessionStatus::Pending { handshake_token, .. }
        if permit.observed_generation == 0 && *handshake_token == Some(permit.token) =>
      {
        record.status = SessionStatus::Active {
          generation: 1,
          connection: Some(connection),
          reconnect_token: None,
        };
        Ok(None)
      }
      SessionStatus::Active {
        generation,
        connection: current,
        reconnect_token,
      } if *generation == permit.observed_generation && *reconnect_token == Some(permit.token) => {
        *generation = generation.checked_add(1).ok_or(SessionError::Unavailable)?;
        *reconnect_token = None;
        Ok(current.replace(connection))
      }
      _ => Err(SessionError::ReconnectSuperseded),
    }
  }

  fn cancel_handshake(&self, session_id: Uuid, observed_generation: u64, token: Uuid) {
    let Ok(mut sessions) = self.sessions.lock() else {
      return;
    };
    let Some(record) = sessions.get_mut(&session_id) else {
      return;
    };
    match &mut record.status {
      SessionStatus::Pending { handshake_token, .. } if observed_generation == 0 && *handshake_token == Some(token) => {
        sessions.remove(&session_id);
      }
      SessionStatus::Active {
        generation,
        reconnect_token,
        ..
      } if *generation == observed_generation && *reconnect_token == Some(token) => {
        *reconnect_token = None;
      }
      _ => {}
    }
  }

  pub fn is_current(&self, session_id: Uuid, generation: u64) -> bool {
    let Ok(sessions) = self.sessions.lock() else {
      return false;
    };
    matches!(
      sessions.get(&session_id).map(|record| &record.status),
      Some(SessionStatus::Active { generation: current, .. }) if *current == generation
    )
  }

  pub fn disconnect_if_current(&self, session_id: Uuid, generation: u64) {
    let Ok(mut sessions) = self.sessions.lock() else {
      return;
    };
    if let Some(SessionRecord {
      status: SessionStatus::Active {
        generation: current,
        connection,
        ..
      },
      ..
    }) = sessions.get_mut(&session_id)
      && *current == generation
    {
      connection.take();
    }
  }

  pub fn revoke(&self, session_id: Uuid) {
    let connection = self
      .sessions
      .lock()
      .ok()
      .and_then(|mut sessions| sessions.remove(&session_id))
      .and_then(|record| match record.status {
        SessionStatus::Active { connection, .. } => connection,
        SessionStatus::Pending { .. } => None,
      });
    if let Some(connection) = connection {
      connection.close(gpapi::service::transport::CloseReason::Unauthorized);
    }
  }
}

impl Drop for HandshakePermit {
  fn drop(&mut self) {
    if let Some(registry) = self.registry.upgrade() {
      registry.cancel_handshake(self.session_id, self.observed_generation, self.token);
    }
  }
}

#[cfg(test)]
mod tests {
  use tokio::sync::mpsc;

  use super::*;
  use crate::ws_connection::ConnectionCommand;

  fn control() -> ConnectionControl {
    let (tx, _rx) = mpsc::channel::<ConnectionCommand>(1);
    ConnectionControl::new(Uuid::new_v4(), tx).0
  }

  #[test]
  fn reconnect_uses_generation_compare_and_swap() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("2.6.4").unwrap();
    let first = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    assert_eq!(registry.prepare_attach(&first).unwrap().generation, 1);
    registry.commit_attach(&first, control()).unwrap();

    let candidate_a = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    assert!(matches!(
      registry.begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4"),
      Err(SessionError::ReconnectSuperseded)
    ));
    assert_eq!(registry.prepare_attach(&candidate_a).unwrap().generation, 2);
    registry.commit_attach(&candidate_a, control()).unwrap();
  }

  #[test]
  fn failed_initial_handshake_only_revokes_its_own_reservation() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("2.6.4").unwrap();
    let first = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    drop(first);
    assert!(matches!(
      registry.begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4"),
      Err(SessionError::Unauthorized)
    ));
  }

  #[test]
  fn failed_reconnect_keeps_active_connection_authoritative() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("2.6.4").unwrap();
    let first = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    registry.commit_attach(&first, control()).unwrap();

    let reconnect = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    drop(reconnect);
    assert!(registry.is_current(credential.session_id(), 1));
    assert!(
      registry
        .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
        .is_ok()
    );
  }
}
