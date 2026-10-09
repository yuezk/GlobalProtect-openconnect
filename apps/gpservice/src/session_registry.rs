use std::{
  collections::HashMap,
  sync::{Arc, Mutex, Weak},
  time::{Duration, Instant},
};

use gpapi::service::transport::{ServiceErrorCode, ServiceResult, SessionCredential};
use thiserror::Error;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::ws_connection::ConnectionControl;

const ACTIVATION_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_PENDING_SESSIONS: usize = 16;
const HIP_PREVIEW_CHUNK_BYTES: usize = 16 * 1024;

pub struct HandshakePermit {
  pub session_id: Uuid,
  pub desktop_uid: Option<u32>,
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
  #[error("invalid HIP report upload")]
  InvalidHipUpload,
}

pub enum HipUploadResult {
  Progress(Uuid),
  Stored(Uuid),
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
  non_tunnel: Option<AttachmentAttempt>,
  closed_attachment: Option<Uuid>,
  secret: Zeroizing<[u8; 32]>,
  product_version: String,
  desktop_uid: Option<u32>,
  edited_hip_report: Option<(Uuid, Arc<str>)>,
  edited_hip_upload: Option<(Uuid, String)>,
  hip_preview: Option<(Uuid, Arc<str>)>,
  last_successful_hip_report: Option<Arc<str>>,
  status: SessionStatus,
}

/// Only live preparation/execution is retained, never acceptance history.
struct AttachmentAttempt {
  generation: u64,
  connection_id: Uuid,
  drain: crate::vpn_task::AttemptDrain,
}

pub struct SessionRegistry {
  service_instance_id: Uuid,
  sessions: Mutex<HashMap<Uuid, SessionRecord>>,
}

fn unavailable_connection() -> ServiceResult {
  ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Connection attachment is unavailable")
}

fn current_connection(record: &SessionRecord, expected: u64) -> Result<Uuid, ServiceResult> {
  match &record.status {
    SessionStatus::Active {
      generation,
      connection: Some(connection),
      ..
    } if *generation == expected && !connection.is_closed() => Ok(connection.connection_id()),
    _ => Err(unavailable_connection()),
  }
}

impl SessionRegistry {
  pub(crate) fn reserve_connection(
    &self,
    session_id: Uuid,
    generation: u64,
    non_tunnel: bool,
    reserve: impl FnOnce() -> Result<crate::vpn_task::ConnectReservation, ServiceResult>,
  ) -> Result<crate::vpn_task::ConnectReservation, ServiceResult> {
    let mut sessions = self.sessions.lock().map_err(|_| unavailable_connection())?;
    let record = sessions.get_mut(&session_id).ok_or_else(unavailable_connection)?;
    let connection_id = current_connection(record, generation)?;
    if non_tunnel && record.closed_attachment == Some(connection_id) {
      return Err(ServiceResult::rejected(
        ServiceErrorCode::InvalidRequest,
        "Non-tunnel attachment is closing",
      ));
    }
    if record
      .non_tunnel
      .as_ref()
      .is_some_and(|attempt| attempt.drain.is_complete())
    {
      record.non_tunnel = None;
    }
    let reservation = reserve()?;
    if non_tunnel {
      record.non_tunnel = Some(AttachmentAttempt {
        generation,
        connection_id,
        drain: reservation.drain(),
      });
    }
    Ok(reservation)
  }

  pub(crate) fn commit_connection(
    &self,
    session_id: Uuid,
    generation: u64,
    non_tunnel: bool,
    commit: impl FnOnce() -> ServiceResult,
  ) -> ServiceResult {
    let Ok(sessions) = self.sessions.lock() else {
      return unavailable_connection();
    };
    let Some(record) = sessions.get(&session_id) else {
      return unavailable_connection();
    };
    if non_tunnel {
      let connection_id = match current_connection(record, generation) {
        Ok(id) => id,
        Err(result) => return result,
      };
      if record.closed_attachment == Some(connection_id) {
        return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Non-tunnel attachment is closing");
      }
    } else if !matches!(record.status, SessionStatus::Active { .. }) {
      return unavailable_connection();
    }
    // Non-tunnel attachment invalidation takes this same lock. An authorized
    // tunnel reservation survives attachment loss as before; revoked sessions
    // cannot commit either mode. Queue insertion transfers cleanup ownership.
    commit()
  }

  pub(crate) fn stop_non_tunnel_attachment(
    &self,
    session_id: Uuid,
    generation: u64,
    connection_id: Uuid,
  ) -> Result<Option<crate::vpn_task::AttemptDrain>, ServiceResult> {
    let mut sessions = self.sessions.lock().map_err(|_| unavailable_connection())?;
    let record = sessions.get_mut(&session_id).ok_or_else(unavailable_connection)?;
    let current_id = current_connection(record, generation)?;
    if current_id == connection_id {
      record.closed_attachment = Some(connection_id);
    }
    if let Some(attempt) = &record.non_tunnel {
      if attempt.connection_id == connection_id {
        attempt.drain.cancel();
        return Ok(Some(attempt.drain.clone()));
      }
    }
    // No retained live work belongs to that attachment. Never stop a successor.
    Ok(None)
  }

  pub fn new(service_instance_id: Uuid) -> Self {
    Self {
      service_instance_id,
      sessions: Mutex::new(HashMap::new()),
    }
  }

  pub fn issue(&self, product_version: &str, desktop_uid: Option<u32>) -> Result<Arc<SessionCredential>, SessionError> {
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

    let credential =
      SessionCredential::generate(self.service_instance_id, product_version).map_err(|_| SessionError::Unavailable)?;
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    let credential = {
      let anchor = crate::device_anchor::resolve().map_err(|error| {
        log::warn!("GUI device anchor unavailable: {error:#}");
        SessionError::Unavailable
      })?;
      credential
        .with_device_anchor(anchor)
        .map_err(|_| SessionError::Unavailable)?
    };
    let credential = Arc::new(credential);
    sessions.insert(
      credential.session_id(),
      SessionRecord {
        non_tunnel: None,
        closed_attachment: None,
        secret: Zeroizing::new(*credential.secret()),
        product_version: credential.product_version().to_owned(),
        desktop_uid,
        edited_hip_report: None,
        edited_hip_upload: None,
        hip_preview: None,
        last_successful_hip_report: None,
        status: SessionStatus::Pending {
          activation_deadline: Instant::now() + ACTIVATION_TIMEOUT,
          handshake_token: None,
        },
      },
    );
    Ok(credential)
  }

  #[cfg(target_os = "macos")]
  pub fn is_empty(&self) -> bool {
    let Ok(mut sessions) = self.sessions.lock() else {
      return false;
    };
    let now = Instant::now();
    sessions.retain(|_, record| {
      !matches!(
        record.status,
        SessionStatus::Pending {
          activation_deadline, ..
        } if now > activation_deadline
      )
    });
    sessions.is_empty()
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
      desktop_uid: record.desktop_uid,
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
        if let Some(attempt) = &record.non_tunnel {
          attempt.drain.cancel();
        }
        record.closed_attachment = None;
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

  pub fn append_edited_hip_report_chunk(
    &self,
    session_id: Uuid,
    upload_id: Option<Uuid>,
    offset: usize,
    chunk: String,
    complete: bool,
  ) -> Result<HipUploadResult, SessionError> {
    let mut sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get_mut(&session_id).ok_or(SessionError::Unauthorized)?;
    if !matches!(record.status, SessionStatus::Active { .. }) {
      return Err(SessionError::Unauthorized);
    }
    if upload_id.is_none() {
      if offset != 0 {
        return Err(SessionError::InvalidHipUpload);
      }
      record.edited_hip_upload = Some((Uuid::new_v4(), String::new()));
    }
    let (current_id, xml) = record
      .edited_hip_upload
      .as_mut()
      .ok_or(SessionError::InvalidHipUpload)?;
    if upload_id.is_some_and(|id| id != *current_id) || offset != xml.len() || chunk.len() > 8 * 1024 {
      return Err(SessionError::InvalidHipUpload);
    }
    if xml.len() + chunk.len() > gphip::MAX_EDITED_REPORT_BYTES {
      record.edited_hip_upload = None;
      return Err(SessionError::InvalidHipUpload);
    }
    xml.push_str(&chunk);
    if !complete {
      return Ok(HipUploadResult::Progress(*current_id));
    }
    if gphip::validate_edited_report(xml).is_err() {
      record.edited_hip_upload = None;
      return Err(SessionError::InvalidHipUpload);
    }
    let (_, xml) = record.edited_hip_upload.take().ok_or(SessionError::InvalidHipUpload)?;
    let report_id = Uuid::new_v4();
    record.edited_hip_report = Some((report_id, Arc::from(xml)));
    Ok(HipUploadResult::Stored(report_id))
  }

  pub fn edited_hip_report(&self, session_id: Uuid, report_id: Uuid) -> Result<Arc<str>, SessionError> {
    let sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get(&session_id).ok_or(SessionError::Unauthorized)?;
    record
      .edited_hip_report
      .as_ref()
      .filter(|(id, _)| *id == report_id)
      .map(|(_, xml)| Arc::clone(xml))
      .ok_or(SessionError::Unauthorized)
  }

  pub fn store_hip_preview(&self, session_id: Uuid, xml: String) -> Result<Uuid, SessionError> {
    let mut sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get_mut(&session_id).ok_or(SessionError::Unauthorized)?;
    if !matches!(record.status, SessionStatus::Active { .. }) {
      return Err(SessionError::Unauthorized);
    }
    let preview_id = Uuid::new_v4();
    record.hip_preview = Some((preview_id, Arc::from(xml)));
    Ok(preview_id)
  }

  pub fn hip_preview_chunk(
    &self,
    session_id: Uuid,
    preview_id: Uuid,
    offset: usize,
  ) -> Result<(String, bool), SessionError> {
    let sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get(&session_id).ok_or(SessionError::Unauthorized)?;
    let (_, xml) = record
      .hip_preview
      .as_ref()
      .filter(|(id, _)| *id == preview_id)
      .ok_or(SessionError::Unauthorized)?;
    if offset > xml.len() || !xml.is_char_boundary(offset) {
      return Err(SessionError::Unauthorized);
    }
    let mut end = (offset + HIP_PREVIEW_CHUNK_BYTES).min(xml.len());
    while !xml.is_char_boundary(end) {
      end -= 1;
    }
    Ok((xml[offset..end].to_owned(), end == xml.len()))
  }

  pub fn store_successful_hip_report(&self, session_id: Uuid, xml: &str) -> Result<(), SessionError> {
    if xml.len() > 1024 * 1024 {
      return Err(SessionError::Unavailable);
    }
    let mut sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get_mut(&session_id).ok_or(SessionError::Unauthorized)?;
    if !matches!(record.status, SessionStatus::Active { .. }) {
      return Err(SessionError::Unauthorized);
    }
    record.last_successful_hip_report = Some(Arc::from(xml));
    Ok(())
  }

  pub fn last_successful_hip_report(&self, session_id: Uuid) -> Result<Option<Arc<str>>, SessionError> {
    let sessions = self.sessions.lock().map_err(|_| SessionError::Unavailable)?;
    let record = sessions.get(&session_id).ok_or(SessionError::Unauthorized)?;
    Ok(record.last_successful_hip_report.as_ref().map(Arc::clone))
  }

  pub fn clear_successful_hip_report(&self, session_id: Uuid) {
    if let Ok(mut sessions) = self.sessions.lock()
      && let Some(record) = sessions.get_mut(&session_id)
    {
      record.last_successful_hip_report = None;
    }
  }

  pub fn disconnect_if_current(&self, session_id: Uuid, generation: u64) {
    let Ok(mut sessions) = self.sessions.lock() else {
      return;
    };
    if let Some(record) = sessions.get_mut(&session_id) {
      if let Some(attempt) = &record.non_tunnel {
        if attempt.generation == generation {
          attempt.drain.cancel();
        }
      }
    }
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
      .and_then(|record| {
        if let Some(attempt) = &record.non_tunnel {
          attempt.drain.cancel();
        }
        match record.status {
          SessionStatus::Active { connection, .. } => connection,
          SessionStatus::Pending { .. } => None,
        }
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

  #[tokio::test]
  async fn attachment_replacement_cancels_preparation_and_retains_drain_until_release() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("fixture", None).unwrap();
    let session = credential.session_id();
    let first = registry
      .begin_handshake(credential.service_instance_id(), session, "fixture")
      .unwrap();
    let (tx, _rx) = mpsc::channel(4);
    let first_control = ConnectionControl::new(Uuid::new_v4(), tx).0;
    let old_id = first_control.connection_id();
    registry.commit_attach(&first, first_control).unwrap();
    let (state, _state_rx) = tokio::sync::watch::channel(gpapi::service::vpn_state::VpnState::Disconnected);
    let (_task, lifecycle) = crate::vpn_task::VpnTask::new(state, false, registry.clone());
    let reservation = registry
      .reserve_connection(session, 1, true, || lifecycle.reserve_connect())
      .unwrap();
    let drain = reservation.drain();
    let cancel = reservation.cancellation();

    let next = registry
      .begin_handshake(credential.service_instance_id(), session, "fixture")
      .unwrap();
    let (tx, _next_rx) = mpsc::channel(4);
    registry
      .commit_attach(&next, ConnectionControl::new(Uuid::new_v4(), tx).0)
      .unwrap();
    assert!(cancel.is_cancelled());
    assert!(!drain.is_complete());
    assert!(
      registry
        .reserve_connection(session, 2, true, || lifecycle.reserve_connect())
        .is_err()
    );
    assert!(matches!(
      registry.commit_connection(session, 1, true, || panic!("stale start committed")),
      ServiceResult::Rejected(_)
    ));
    drop(reservation);
    drain.wait().await;

    let next_reservation = registry
      .reserve_connection(session, 2, true, || lifecycle.reserve_connect())
      .unwrap();
    assert!(
      registry
        .stop_non_tunnel_attachment(session, 2, old_id)
        .unwrap()
        .is_none()
    );
    assert!(!next_reservation.cancellation().is_cancelled());
    registry.disconnect_if_current(session, 1);
    assert!(!next_reservation.cancellation().is_cancelled());
    registry.disconnect_if_current(session, 2);
    assert!(next_reservation.cancellation().is_cancelled());
  }

  #[tokio::test]
  async fn scoped_stop_closes_attachment_to_delayed_starts_without_receipt_history() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("fixture", None).unwrap();
    let session = credential.session_id();
    let permit = registry
      .begin_handshake(credential.service_instance_id(), session, "fixture")
      .unwrap();
    let (tx, _rx) = mpsc::channel(4);
    let connection = ConnectionControl::new(Uuid::new_v4(), tx).0;
    let id = connection.connection_id();
    registry.commit_attach(&permit, connection).unwrap();
    let (state, _state_rx) = tokio::sync::watch::channel(gpapi::service::vpn_state::VpnState::Disconnected);
    let (_task, lifecycle) = crate::vpn_task::VpnTask::new(state, false, registry.clone());
    let reservation = registry
      .reserve_connection(session, 1, true, || lifecycle.reserve_connect())
      .unwrap();
    let drain = registry.stop_non_tunnel_attachment(session, 1, id).unwrap().unwrap();
    assert!(reservation.cancellation().is_cancelled());
    assert!(matches!(
      registry.commit_connection(session, 1, true, || panic!("closing start committed")),
      ServiceResult::Rejected(_)
    ));
    assert!(!drain.is_complete());
    drop(reservation);
    drain.wait().await;
    assert!(
      registry
        .reserve_connection(session, 1, true, || panic!("closed attachment reserved"))
        .is_err()
    );
    assert!(
      registry
        .reserve_connection(session, 1, false, || lifecycle.reserve_connect())
        .is_ok()
    );
  }

  #[tokio::test]
  async fn tunnel_reservation_survives_attachment_loss_but_non_tunnel_revocation_cancels() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("fixture", None).unwrap();
    let session = credential.session_id();
    let permit = registry
      .begin_handshake(credential.service_instance_id(), session, "fixture")
      .unwrap();
    let (tx, _rx) = mpsc::channel(4);
    registry
      .commit_attach(&permit, ConnectionControl::new(Uuid::new_v4(), tx).0)
      .unwrap();
    let (state, _state_rx) = tokio::sync::watch::channel(gpapi::service::vpn_state::VpnState::Disconnected);
    let (_task, lifecycle) = crate::vpn_task::VpnTask::new(state, false, registry.clone());
    let tunnel = registry
      .reserve_connection(session, 1, false, || lifecycle.reserve_connect())
      .unwrap();
    registry.disconnect_if_current(session, 1);
    assert!(!tunnel.cancellation().is_cancelled());
    assert_eq!(
      registry.commit_connection(session, 1, false, || ServiceResult::Accepted),
      ServiceResult::Accepted
    );
    drop(tunnel);
    let permit = registry
      .begin_handshake(credential.service_instance_id(), session, "fixture")
      .unwrap();
    let (tx, _next_rx) = mpsc::channel(4);
    registry
      .commit_attach(&permit, ConnectionControl::new(Uuid::new_v4(), tx).0)
      .unwrap();
    let session_work = registry
      .reserve_connection(session, 2, true, || lifecycle.reserve_connect())
      .unwrap();
    registry.revoke(session);
    assert!(session_work.cancellation().is_cancelled());
    assert!(!session_work.drain().is_complete());
  }

  #[test]
  fn credential_retains_verified_desktop_user_across_reconnect() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("2.6.4", Some(1000)).unwrap();
    let first = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    assert_eq!(first.desktop_uid, Some(1000));
    registry.commit_attach(&first, control()).unwrap();

    let reconnect = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    assert_eq!(reconnect.desktop_uid, Some(1000));
  }

  #[test]
  fn edited_hip_report_is_session_bound_and_replaced_without_affecting_pinned_content() {
    fn upload(registry: &SessionRegistry, session_id: Uuid, xml: &str) -> Uuid {
      let mut upload_id = None;
      let mut offset = 0;
      while offset < xml.len() {
        let mut end = (offset + 8 * 1024).min(xml.len());
        while !xml.is_char_boundary(end) {
          end -= 1;
        }
        let complete = end == xml.len();
        match registry
          .append_edited_hip_report_chunk(session_id, upload_id, offset, xml[offset..end].into(), complete)
          .unwrap()
        {
          HipUploadResult::Progress(id) => upload_id = Some(id),
          HipUploadResult::Stored(id) => return id,
        }
        offset = end;
      }
      panic!("upload did not complete")
    }

    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let first_credential = registry.issue("2.6.4", Some(1000)).unwrap();
    let second_credential = registry.issue("2.6.4", Some(1001)).unwrap();
    for credential in [&first_credential, &second_credential] {
      let permit = registry
        .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
        .unwrap();
      registry.commit_attach(&permit, control()).unwrap();
    }

    let valid = gphip::generate_report(&gphip::ReportInput {
      profile: gpapi::os_profile::OsProfile::builder(gpapi::os_profile::ClientOs::Linux).build(),
      context: gphip::ReportContext::Preview,
    })
    .unwrap();
    let first_id = upload(&registry, first_credential.session_id(), &valid);
    let pinned = registry
      .edited_hip_report(first_credential.session_id(), first_id)
      .unwrap();
    assert!(matches!(
      registry.edited_hip_report(second_credential.session_id(), first_id),
      Err(SessionError::Unauthorized)
    ));
    let second_id = upload(&registry, first_credential.session_id(), &valid);
    assert!(matches!(
      registry.edited_hip_report(first_credential.session_id(), first_id),
      Err(SessionError::Unauthorized)
    ));
    assert_eq!(
      &*registry
        .edited_hip_report(first_credential.session_id(), second_id)
        .unwrap(),
      valid
    );
    assert_eq!(&*pinned, valid);
    registry.revoke(first_credential.session_id());
    assert!(matches!(
      registry.edited_hip_report(first_credential.session_id(), second_id),
      Err(SessionError::Unauthorized)
    ));
  }

  #[test]
  fn submitted_report_changes_only_after_success_callback_and_is_session_bound() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("2.6.4", Some(1000)).unwrap();
    let permit = registry
      .begin_handshake(credential.service_instance_id(), credential.session_id(), "2.6.4")
      .unwrap();
    registry.commit_attach(&permit, control()).unwrap();
    assert!(
      registry
        .last_successful_hip_report(credential.session_id())
        .unwrap()
        .is_none()
    );
    registry
      .store_successful_hip_report(credential.session_id(), "first")
      .unwrap();
    assert_eq!(
      registry
        .last_successful_hip_report(credential.session_id())
        .unwrap()
        .as_deref(),
      Some("first")
    );
    let preview_id = registry
      .store_hip_preview(credential.session_id(), "é".repeat(8193))
      .unwrap();
    let (first_chunk, complete) = registry
      .hip_preview_chunk(credential.session_id(), preview_id, 0)
      .unwrap();
    assert!(!complete);
    assert!(first_chunk.len() <= HIP_PREVIEW_CHUNK_BYTES);
    assert!(
      registry
        .hip_preview_chunk(credential.session_id(), preview_id, 1)
        .is_err()
    );
    assert_eq!(
      registry
        .last_successful_hip_report(credential.session_id())
        .unwrap()
        .as_deref(),
      Some("first")
    );
    registry.clear_successful_hip_report(credential.session_id());
    assert!(
      registry
        .last_successful_hip_report(credential.session_id())
        .unwrap()
        .is_none()
    );
  }

  #[test]
  fn reconnect_uses_generation_compare_and_swap() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue("2.6.4", None).unwrap();
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
    let credential = registry.issue("2.6.4", None).unwrap();
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
    let credential = registry.issue("2.6.4", None).unwrap();
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
