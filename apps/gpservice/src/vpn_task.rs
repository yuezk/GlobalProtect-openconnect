use std::sync::{
  Arc, Mutex,
  atomic::{AtomicBool, Ordering},
};

use gpapi::{
  service::{
    transport::{ServiceErrorCode, ServiceResult},
    vpn_state::{ConnectInfo, ConnectedInfo, VpnState},
  },
  session::{SessionInfo, SessionWarning},
};
use log::{info, warn};
use openconnect::VpnSessionInfo;
use tokio::sync::{Notify, mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::Zeroizing;

#[cfg(test)]
use gpapi::service::request::ConnectRequest;

#[cfg(test)]
mod ownership_tests;

mod non_tunnel;
mod preparation;
pub(crate) use preparation::{PreparedConnection, prepare_connection};
mod runtime;

#[derive(Clone)]
pub(crate) struct LifecycleHandle {
  events: mpsc::Sender<LifecycleEvent>,
  admissions: Arc<Mutex<Admissions>>,
}

impl LifecycleHandle {
  pub fn reserve_connect(&self) -> Result<ConnectReservation, ServiceResult> {
    let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
    if admissions.closed {
      return Err(ServiceResult::rejected(
        ServiceErrorCode::Internal,
        "VPN task is unavailable",
      ));
    }
    if admissions.current.is_some() {
      return Err(ServiceResult::rejected(
        ServiceErrorCode::Busy,
        "A VPN connection is already in progress",
      ));
    }
    let attempt = admissions.next_attempt;
    admissions.next_attempt += 1;
    let cancellation = CancellationToken::new();
    admissions.current = Some(Admission {
      attempt,
      phase: AdmissionPhase::Preparing,
      stop_requested: false,
      cancellation: cancellation.clone(),
      completion: Arc::new(AttemptCompletion::default()),
    });
    Ok(ConnectReservation {
      attempt,
      lifecycle: self.clone(),
      cancellation,
      transferred: false,
    })
  }

  #[cfg(test)]
  fn submit_test_connection(
    &self,
    request: ConnectRequest,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
    session_id: Option<Uuid>,
  ) -> ServiceResult {
    match self.reserve_connect() {
      Ok(reservation) => reservation.commit(
        PreparedConnection::lifecycle_fixture(request),
        desktop_uid,
        edited_report,
        session_id,
      ),
      Err(result) => result,
    }
  }

  pub fn request_disconnect(&self) -> ServiceResult {
    let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
    let Some(admission) = admissions.current.as_mut() else {
      return ServiceResult::Accepted;
    };
    if admission.stop_requested {
      return ServiceResult::Accepted;
    }

    admission.stop_requested = true;
    admission.cancellation.cancel();
    admission.phase = AdmissionPhase::Stopping;
    let _ = self.events.try_send(LifecycleEvent::Stop {
      attempt: admission.attempt,
    });
    ServiceResult::Accepted
  }

  pub async fn disconnect_and_wait(&self) -> bool {
    let completion = {
      let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
      let Some(admission) = admissions.current.as_mut() else {
        return false;
      };
      if !admission.stop_requested {
        let _ = self.events.try_send(LifecycleEvent::Stop {
          attempt: admission.attempt,
        });
        admission.stop_requested = true;
        admission.cancellation.cancel();
        admission.phase = AdmissionPhase::Stopping;
      }
      Arc::clone(&admission.completion)
    };

    loop {
      let notified = completion.notify.notified();
      if completion.completed.load(Ordering::Acquire) {
        return true;
      }
      notified.await;
    }
  }

  #[cfg(test)]
  fn has_attempt(&self, attempt: u64) -> bool {
    self
      .admissions
      .lock()
      .expect("VPN lifecycle admissions mutex poisoned")
      .current
      .as_ref()
      .is_some_and(|admission| admission.attempt == attempt)
  }

  fn is_stop_requested(&self, attempt: u64) -> bool {
    self
      .admissions
      .lock()
      .expect("VPN lifecycle admissions mutex poisoned")
      .current
      .as_ref()
      .is_some_and(|admission| admission.attempt == attempt && admission.stop_requested)
  }

  fn close_admission(&self) {
    let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
    admissions.closed = true;
    if let Some(admission) = &mut admissions.current {
      admission.stop_requested = true;
      admission.phase = AdmissionPhase::Stopping;
      admission.cancellation.cancel();
    }
  }

  fn attempt_cancellation(&self, attempt: u64) -> CancellationToken {
    let admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
    if let Some(admission) = admissions
      .current
      .as_ref()
      .filter(|admission| admission.attempt == attempt)
    {
      return admission.cancellation.clone();
    }
    // A stale queued owner must still drain its transferred sessions.
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    cancellation
  }

  fn activate_attempt(&self, attempt: u64) -> bool {
    let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
    let Some(admission) = admissions.current.as_mut() else {
      return false;
    };
    if admission.attempt != attempt
      || admission.stop_requested
      || admission.cancellation.is_cancelled()
      || admission.phase != AdmissionPhase::Pending
    {
      return false;
    }
    admission.phase = AdmissionPhase::Active;
    true
  }

  fn clear_attempt(&self, attempt: u64) {
    let completion = {
      let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
      let Some(admission) = admissions.current.as_ref() else {
        return;
      };
      if admission.attempt != attempt {
        return;
      }
      let completion = Arc::clone(&admission.completion);
      admissions.current = None;
      completion
    };
    completion.completed.store(true, Ordering::Release);
    completion.notify.notify_waiters();
  }

  #[cfg(test)]
  pub(crate) fn for_tests() -> Self {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let registry = Arc::new(crate::session_registry::SessionRegistry::new(Uuid::new_v4()));
    let (_task, lifecycle) = VpnTask::new(vpn_state_tx, false, registry);
    lifecycle
  }
}

/// A pre-admission reservation owns no server authentication. Dropping it after
/// joined preparation releases admission and wakes shutdown; commit transfers
/// cleanup ownership atomically with successful queue insertion.
pub(crate) struct ConnectReservation {
  attempt: u64,
  lifecycle: LifecycleHandle,
  cancellation: CancellationToken,
  transferred: bool,
}

#[derive(Clone)]
pub(crate) struct AttemptDrain {
  cancellation: CancellationToken,
  completion: Arc<AttemptCompletion>,
}

impl AttemptDrain {
  pub fn cancel(&self) {
    self.cancellation.cancel();
  }

  pub fn is_complete(&self) -> bool {
    self.completion.completed.load(Ordering::Acquire)
  }

  pub async fn wait(&self) {
    loop {
      let notified = self.completion.notify.notified();
      if self.is_complete() {
        return;
      }
      notified.await;
    }
  }
}

impl ConnectReservation {
  pub fn drain(&self) -> AttemptDrain {
    let admissions = self
      .lifecycle
      .admissions
      .lock()
      .expect("VPN lifecycle admissions mutex poisoned");
    let admission = admissions
      .current
      .as_ref()
      .filter(|admission| admission.attempt == self.attempt)
      .expect("reservation retains its admission");
    AttemptDrain {
      cancellation: self.cancellation.clone(),
      completion: Arc::clone(&admission.completion),
    }
  }
  pub fn cancellation(&self) -> CancellationToken {
    self.cancellation.clone()
  }

  pub fn commit(
    mut self,
    connection: PreparedConnection,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
    session_id: Option<Uuid>,
  ) -> ServiceResult {
    let result = {
      let mut admissions = self
        .lifecycle
        .admissions
        .lock()
        .expect("VPN lifecycle admissions mutex poisoned");
      if admissions.closed {
        return ServiceResult::rejected(ServiceErrorCode::Internal, "VPN task is unavailable");
      }
      let Some(admission) = admissions
        .current
        .as_mut()
        .filter(|admission| admission.attempt == self.attempt)
      else {
        return ServiceResult::rejected(
          ServiceErrorCode::InvalidRequest,
          "Connection preparation was superseded",
        );
      };
      if admission.stop_requested || self.cancellation.is_cancelled() {
        return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Connection preparation was cancelled");
      }
      match self.lifecycle.events.try_send(LifecycleEvent::Start {
        attempt: self.attempt,
        connection,
        desktop_uid,
        edited_report,
        session_id,
      }) {
        Ok(()) => {
          admission.phase = AdmissionPhase::Pending;
          self.transferred = true;
          ServiceResult::Accepted
        }
        Err(mpsc::error::TrySendError::Full(_)) => {
          ServiceResult::rejected(ServiceErrorCode::Busy, "VPN lifecycle queue is full")
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
          ServiceResult::rejected(ServiceErrorCode::Internal, "VPN task is unavailable")
        }
      }
    };
    result
  }
}

impl Drop for ConnectReservation {
  fn drop(&mut self) {
    if !self.transferred {
      self.cancellation.cancel();
      self.lifecycle.clear_attempt(self.attempt);
      // A full queue already wakes the actor; a closed queue has no actor to wake.
      let _ = self.lifecycle.events.try_send(LifecycleEvent::PreparationReleased);
    }
  }
}

#[derive(Default)]
struct Admissions {
  closed: bool,
  next_attempt: u64,
  current: Option<Admission>,
}

#[derive(Clone)]
struct Admission {
  attempt: u64,
  phase: AdmissionPhase,
  stop_requested: bool,
  cancellation: CancellationToken,
  completion: Arc<AttemptCompletion>,
}

#[derive(Default)]
struct AttemptCompletion {
  completed: AtomicBool,
  notify: Notify,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AdmissionPhase {
  Preparing,
  Pending,
  Active,
  Stopping,
}

enum LifecycleEvent {
  Start {
    attempt: u64,
    connection: PreparedConnection,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
    session_id: Option<Uuid>,
  },
  Stop {
    attempt: u64,
  },
  Established {
    attempt: u64,
    session: VpnSessionInfo,
  },
  MembersChanged {
    attempt: u64,
    members: Vec<gpapi::session::GatewaySessionSummary>,
    failures: Vec<gpapi::session::GatewayFailureSummary>,
  },
  Ended {
    attempt: u64,
  },
  Failed {
    attempt: u64,
    reason: String,
  },
  PreparationReleased,
}

struct ActiveAttempt {
  attempt: u64,
  session_id: Option<Uuid>,
  info: ConnectInfo,
  allow_extend_session: bool,
  cancellation: CancellationToken,
  task: tokio::task::JoinHandle<()>,
  failure: Option<String>,
}

pub(crate) struct VpnTask {
  events: mpsc::Receiver<LifecycleEvent>,
  lifecycle: LifecycleHandle,
  vpn_state_tx: watch::Sender<VpnState>,
  brokered_macos: bool,
  registry: Arc<crate::session_registry::SessionRegistry>,
  active: Option<ActiveAttempt>,
  cancel_token: CancellationToken,
}

impl VpnTask {
  pub fn new(
    vpn_state_tx: watch::Sender<VpnState>,
    brokered_macos: bool,
    registry: Arc<crate::session_registry::SessionRegistry>,
  ) -> (Self, LifecycleHandle) {
    let (events_tx, events) = mpsc::channel(4);
    let lifecycle = LifecycleHandle {
      events: events_tx,
      admissions: Arc::new(Mutex::new(Admissions::default())),
    };
    let task = Self {
      events,
      lifecycle: lifecycle.clone(),
      vpn_state_tx,
      brokered_macos,
      registry,
      active: None,
      cancel_token: CancellationToken::new(),
    };
    (task, lifecycle)
  }

  pub fn cancel_token(&self) -> CancellationToken {
    self.cancel_token.clone()
  }

  pub async fn start(&mut self, server_cancel_token: CancellationToken) {
    let cancel_token = self.cancel_token.clone();
    let mut shutting_down = false;

    loop {
      if shutting_down {
        if self.active.is_none() && !self.has_admission() {
          break;
        }
        let Some(event) = self.events.recv().await else { break };
        self.handle_event(event).await;
        continue;
      }

      tokio::select! {
        event = self.events.recv() => {
          let Some(event) = event else { break };
          self.handle_event(event).await;
        }
        _ = cancel_token.cancelled() => {
          info!("VPN task cancelled");
          shutting_down = true;
          self.lifecycle.close_admission();
          if let Some(active) = self.active.as_ref() {
            self.stop_attempt(active.attempt);
          }
        }
      }
    }

    self.lifecycle.close_admission();
    server_cancel_token.cancel();
    info!("VPN task stopped");
  }

  fn has_admission(&self) -> bool {
    self
      .lifecycle
      .admissions
      .lock()
      .expect("VPN lifecycle admissions mutex poisoned")
      .current
      .is_some()
  }

  async fn handle_event(&mut self, event: LifecycleEvent) {
    match event {
      LifecycleEvent::Start {
        attempt,
        connection,
        desktop_uid,
        edited_report,
        session_id,
      } => self.start_attempt(attempt, connection, desktop_uid, edited_report, session_id),
      LifecycleEvent::Stop { attempt } => self.stop_attempt(attempt),
      LifecycleEvent::Established { attempt, session } => self.establish_attempt(attempt, session),
      LifecycleEvent::MembersChanged {
        attempt,
        members,
        failures,
      } => self.publish_members(attempt, members, failures),
      LifecycleEvent::Ended { attempt } => self.end_attempt(attempt).await,
      LifecycleEvent::Failed { attempt, reason } => {
        if let Some(active) = self.active.as_mut().filter(|active| active.attempt == attempt) {
          active.failure = Some(reason);
        }
      }
      LifecycleEvent::PreparationReleased => {}
    }
  }

  fn start_attempt(
    &mut self,
    attempt: u64,
    connection: PreparedConnection,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
    session_id: Option<Uuid>,
  ) {
    let activated = self.lifecycle.activate_attempt(attempt);
    let info = connection.request.info().clone();
    let allow_extend_session = connection.request.args().allow_extend_session();
    let cancellation = self.lifecycle.attempt_cancellation(attempt);
    if !activated {
      cancellation.cancel();
    }
    let task = runtime::spawn(
      runtime::AttemptInput {
        attempt,
        connection,
        desktop_uid,
        edited_report,
        session_id,
        brokered_macos: self.brokered_macos,
        registry: self.registry.clone(),
        lifecycle: self.lifecycle.clone(),
      },
      cancellation.clone(),
      self.lifecycle.events.clone(),
    );
    self.active = Some(ActiveAttempt {
      attempt,
      session_id,
      info: info.clone(),
      allow_extend_session,
      cancellation,
      task,
      failure: None,
    });
    if activated {
      self.send_state(VpnState::Connecting(Box::new(info)));
    }
  }

  fn stop_attempt(&mut self, attempt: u64) {
    let Some(active) = self.active.as_ref() else {
      return;
    };
    if active.attempt != attempt || !self.lifecycle.is_stop_requested(attempt) {
      return;
    }

    info!("Disconnecting VPN...");
    self.send_state(VpnState::Disconnecting);
    active.cancellation.cancel();
  }

  fn establish_attempt(&mut self, attempt: u64, vpn_session_info: VpnSessionInfo) {
    let Some(active) = self.active.as_ref() else {
      return;
    };
    if active.attempt != attempt {
      return;
    }
    if active.cancellation.is_cancelled() {
      return;
    }

    let session_info = SessionInfo::from_vpn_session_fields(
      vpn_session_info.lifetime_secs,
      vpn_session_info.user_expires,
      vpn_session_info.lifetime_warning.map(|warning| SessionWarning {
        prior_secs: warning.prior_secs,
        message: warning.message,
      }),
      active.allow_extend_session,
    );
    self.publish_connected(attempt, active.info.clone(), session_info);
  }

  fn publish_members(
    &self,
    attempt: u64,
    members: Vec<gpapi::session::GatewaySessionSummary>,
    failures: Vec<gpapi::session::GatewayFailureSummary>,
  ) {
    let Some(active) = &self.active else {
      return;
    };
    if active.attempt != attempt || active.cancellation.is_cancelled() {
      return;
    }
    if !members
      .iter()
      .any(|member| member.maintenance == gpapi::session::MaintenanceState::Healthy)
    {
      if matches!(&*self.vpn_state_tx.borrow(), VpnState::Connected(_)) {
        self.send_state(VpnState::Connecting(Box::new(active.info.clone())));
      }
      return;
    }
    let admissions = self
      .lifecycle
      .admissions
      .lock()
      .expect("VPN lifecycle admissions mutex poisoned");
    if admissions
      .current
      .as_ref()
      .is_none_or(|admission| admission.attempt != attempt || admission.stop_requested)
    {
      return;
    }
    let info = match &*self.vpn_state_tx.borrow() {
      VpnState::Connected(connected) => (**connected).clone(),
      _ => ConnectedInfo::new(active.info.clone(), None),
    };
    self.send_state(VpnState::Connected(Box::new(info.with_members(members, failures))));
  }

  fn publish_connected(&self, attempt: u64, info: ConnectInfo, session_info: SessionInfo) {
    let connected = VpnState::Connected(Box::new(ConnectedInfo::new(info, Some(session_info))));
    let admissions = self
      .lifecycle
      .admissions
      .lock()
      .expect("VPN lifecycle admissions mutex poisoned");
    if admissions.current.as_ref().is_some_and(|admission| {
      admission.attempt == attempt && admission.phase == AdmissionPhase::Active && !admission.stop_requested
    }) {
      self.send_state(connected);
    }
  }

  async fn end_attempt(&mut self, attempt: u64) {
    if self.active.as_ref().is_none_or(|active| active.attempt != attempt) {
      return;
    }
    let active = self.active.take().expect("active attempt checked above");
    if let Err(error) = active.task.await {
      warn!("Connection attempt worker failed: {error}");
    }
    if let Some(session_id) = active.session_id {
      self.registry.clear_successful_hip_report(session_id);
    }
    if let Some(reason) = active.failure {
      self.send_state(VpnState::Failed(reason));
      self.lifecycle.clear_attempt(attempt);
    } else {
      self.finish_attempt(attempt);
    }
  }

  fn finish_attempt(&self, attempt: u64) {
    self.send_state(VpnState::Disconnected);
    self.lifecycle.clear_attempt(attempt);
  }

  fn send_state(&self, state: VpnState) {
    let _ = self.vpn_state_tx.send(state);
  }
}

fn prepare_identity(
  args: &gpapi::service::request::ConnectArgs,
  protected_snapshot_required: bool,
) -> anyhow::Result<Option<gpapi::utils::request::ClientIdentity>> {
  let certificate_path = args.certificate();
  let key_path = args.sslkey();
  let certificate = args.certificate_data()?.map(Zeroizing::new);
  let key = args.sslkey_data()?.map(Zeroizing::new);
  let password = args.key_password().map(Zeroizing::new);
  if certificate_path.is_some() || key_path.is_some() {
    anyhow::ensure!(
      certificate.is_none() && key.is_none(),
      "Client identity cannot mix paths and protected data"
    );
    anyhow::ensure!(
      !protected_snapshot_required,
      "Client identity must be a protected snapshot"
    );
    let certificate_path = certificate_path
      .as_deref()
      .ok_or_else(|| anyhow::anyhow!("Client identity key requires a certificate path"))?;
    return gpapi::utils::request::ClientIdentity::load(
      certificate_path,
      key_path.as_deref(),
      password.as_deref().map(String::as_str),
    )
    .map(Some);
  }
  let size = certificate
    .as_ref()
    .map_or(0, |data| data.len())
    .checked_add(key.as_ref().map_or(0, |data| data.len()));
  anyhow::ensure!(
    size.is_some_and(|size| size <= gpapi::service::request::MAX_CLIENT_IDENTITY_DATA),
    "Client identity exceeds its size limit"
  );
  match (certificate, key) {
    (None, None) => Ok(None),
    (Some(certificate), key) => Ok(Some(gpapi::utils::request::ClientIdentity::from_data(
      certificate,
      key,
      password.as_deref().map(String::as_str),
    )?)),
    (None, Some(_)) => anyhow::bail!("Client identity key requires certificate data"),
  }
}

#[cfg(test)]
pub(crate) fn test_connect_request(info: ConnectInfo) -> ConnectRequest {
  let endpoint = info
    .gateway()
    .server()
    .trim_start_matches("https://")
    .parse::<std::net::SocketAddr>()
    .unwrap_or_else(|_| "192.0.2.10:443".parse().unwrap());
  let source = if endpoint.ip().is_loopback() {
    endpoint.ip()
  } else {
    "192.0.2.5".parse().unwrap()
  };
  let interface = if endpoint.ip().is_loopback() {
    if cfg!(target_os = "linux") { "lo" } else { "lo0" }
  } else {
    "fixture"
  };
  let binding = gpapi::session::network::GatewayBinding::new(
    endpoint,
    std::net::SocketAddr::new(source, 0),
    interface.into(),
    1,
    gpapi::session::ClientAddresses {
      ipv4: match source {
        std::net::IpAddr::V4(ip) => Some(ip),
        _ => None,
      },
      ipv6: match source {
        std::net::IpAddr::V6(ip) => Some(ip),
        _ => None,
      },
    },
    "0".repeat(64),
  )
  .unwrap();
  let plan = gpapi::session::ConnectionPlan::new(
    vec![gpapi::session::AuthenticatedGateway {
      gateway: info.gateway().clone(),
      binding: Some(binding),
      authentication: gpapi::session::GatewayAuthentication::new("authcookie=token&user=user".into(), "tunnel".into())
        .unwrap(),
    }],
    vec![],
    gpapi::session::InternalSessionPolicy::default(),
  )
  .unwrap();
  ConnectRequest::new(info, plan)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::session_registry::SessionRegistry;

  fn registry() -> Arc<SessionRegistry> {
    Arc::new(SessionRegistry::new(Uuid::new_v4()))
  }

  fn identity_request() -> ConnectRequest {
    test_connect_request(ConnectInfo::new(
      "portal.example.com".into(),
      gpapi::gateway::Gateway::new("gateway".into(), "gateway.example.com".into()),
      vec![],
    ))
  }

  #[test]
  fn protected_identity_size_limit_covers_pkcs12_and_combined_pem_before_import() {
    let limit = gpapi::service::request::MAX_CLIENT_IDENTITY_DATA;
    for request in [
      identity_request().with_certificate_data(Some(vec![0; limit + 1])),
      identity_request()
        .with_certificate_data(Some(vec![0; limit]))
        .with_sslkey_data(Some(vec![0])),
    ] {
      assert_eq!(
        prepare_identity(request.args(), true).unwrap_err().to_string(),
        "Client identity exceeds its size limit"
      );
    }
    assert_eq!(
      prepare_identity(identity_request().with_sslkey_data(Some(vec![0])).args(), true)
        .unwrap_err()
        .to_string(),
      "Client identity key requires certificate data"
    );
  }

  fn identity_material() -> (openssl::pkey::PKey<openssl::pkey::Private>, openssl::x509::X509) {
    use openssl::{
      asn1::Asn1Time,
      hash::MessageDigest,
      pkey::PKey,
      rsa::Rsa,
      x509::{X509, X509NameBuilder},
    };
    let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
    let mut name = X509NameBuilder::new().unwrap();
    name.append_entry_by_text("CN", "service-identity-fixture").unwrap();
    let name = name.build();
    let mut certificate = X509::builder().unwrap();
    certificate.set_version(2).unwrap();
    certificate.set_subject_name(&name).unwrap();
    certificate.set_issuer_name(&name).unwrap();
    certificate.set_pubkey(&key).unwrap();
    certificate
      .set_not_before(&Asn1Time::days_from_now(0).unwrap())
      .unwrap();
    certificate.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
    certificate.sign(&key, MessageDigest::sha256()).unwrap();
    (key, certificate.build())
  }

  #[test]
  fn tunnel_identity_paths_allow_large_files_and_retain_loaded_material() {
    let (key, certificate) = identity_material();
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("identity.pem");
    let mut pem = certificate.to_pem().unwrap();
    pem.extend(key.private_key_to_pem_pkcs8().unwrap());
    pem.resize(gpapi::service::request::MAX_CLIENT_IDENTITY_DATA + 1, b'\n');
    std::fs::write(&path, pem).unwrap();
    let request = identity_request().with_certificate(Some(path.to_str().unwrap().into()));
    let identity = prepare_identity(request.args(), false).unwrap().unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(identity.certificate_data(), certificate.to_pem().unwrap());
    let files = identity.write_files(None).unwrap();
    assert_eq!(
      std::fs::read(files.certificate()).unwrap(),
      certificate.to_pem().unwrap()
    );
    assert!(files.key().is_some());
  }

  #[test]
  fn protected_identity_rejects_paths_and_all_handoffs_reject_mixed_material() {
    let path = identity_request().with_certificate(Some("/identity.pem".into()));
    assert_eq!(
      prepare_identity(path.args(), true).unwrap_err().to_string(),
      "Client identity must be a protected snapshot"
    );
    let mixed = path.with_certificate_data(Some(vec![0]));
    for protected_snapshot_required in [false, true] {
      assert_eq!(
        prepare_identity(mixed.args(), protected_snapshot_required)
          .unwrap_err()
          .to_string(),
        "Client identity cannot mix paths and protected data"
      );
    }
  }

  #[test]
  fn protected_pkcs12_import_retains_password_and_single_file_material() {
    use openssl::pkcs12::Pkcs12;
    let (key, certificate) = identity_material();
    let password = "service-fixture-password";
    let p12 = Pkcs12::builder()
      .name("fixture")
      .pkey(&key)
      .cert(&certificate)
      .build2(password)
      .unwrap()
      .to_der()
      .unwrap();
    let request = identity_request()
      .with_certificate_data(Some(p12.clone()))
      .with_key_password(Some(password.into()));
    let wire = serde_json::to_vec(&request).unwrap();
    let request: ConnectRequest = serde_json::from_slice(&wire).unwrap();
    let identity = prepare_identity(request.args(), true).unwrap().unwrap();
    assert_eq!(identity.certificate_data(), p12);
    assert!(identity.key_data().is_none());
    assert_eq!(identity.key_password(), Some(password));
    let files = identity.write_files(None).unwrap();
    assert!(files.certificate().ends_with(".p12"));
    assert!(files.key().is_none());
    assert_eq!(std::fs::read(files.certificate()).unwrap(), p12);
    assert!(!format!("{request:?}").contains(password));
  }

  #[test]
  fn shutdown_cancels_preparation_despite_a_full_queue_and_rejects_commit() {
    let (state, _state_rx) = watch::channel(VpnState::Disconnected);
    let (_task, lifecycle) = VpnTask::new(state, false, registry());
    let reservation = lifecycle.reserve_connect().unwrap();
    let cancellation = reservation.cancellation();
    while lifecycle.events.try_send(LifecycleEvent::PreparationReleased).is_ok() {}
    lifecycle.close_admission();
    assert!(cancellation.is_cancelled());
    assert!(lifecycle.reserve_connect().is_err());
    let request = test_connect_request(ConnectInfo::new(
      "portal.example.com".into(),
      gpapi::gateway::Gateway::new("vpn".into(), "vpn.example.com".into()),
      vec![],
    ));
    assert!(matches!(
      reservation.commit(PreparedConnection::lifecycle_fixture(request), None, None, None,),
      ServiceResult::Rejected(..)
    ));
    assert!(!lifecycle.has_attempt(0));
    assert!(lifecycle.reserve_connect().is_err());
  }

  #[test]
  fn disconnect_cancels_preparation_even_when_stop_notification_queue_is_full() {
    let (state, _state_rx) = watch::channel(VpnState::Disconnected);
    let (_task, lifecycle) = VpnTask::new(state, false, registry());
    let reservation = lifecycle.reserve_connect().unwrap();
    while lifecycle.events.try_send(LifecycleEvent::PreparationReleased).is_ok() {}
    assert_eq!(lifecycle.request_disconnect(), ServiceResult::Accepted);
    assert!(reservation.cancellation().is_cancelled());
    assert!(!reservation.drain().is_complete());
    assert!(lifecycle.reserve_connect().is_err());
  }

  #[tokio::test]
  async fn non_tunnel_connected_requires_healthy_hip_and_disappears_when_none_remain() {
    let (state, state_rx) = watch::channel(VpnState::Disconnected);
    let (mut task, lifecycle) = VpnTask::new(state, false, registry());
    let gateway = gpapi::gateway::Gateway::new("vpn".into(), "vpn.example.com".into());
    let info = ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![]);
    let reservation = lifecycle.reserve_connect().unwrap();
    let cancellation = reservation.cancellation();
    assert_eq!(
      reservation.commit(
        PreparedConnection::lifecycle_fixture(test_connect_request(info.clone())),
        None,
        None,
        None
      ),
      ServiceResult::Accepted
    );
    let _ = task.events.recv().await.unwrap();
    assert!(lifecycle.activate_attempt(0));
    task.active = Some(ActiveAttempt {
      attempt: 0,
      session_id: None,
      info: info.clone(),
      allow_extend_session: false,
      cancellation,
      task: tokio::spawn(async {}),
      failure: None,
    });
    task.send_state(VpnState::Connecting(Box::new(info)));
    let mut member = gpapi::session::GatewaySessionSummary {
      gateway,
      mode: gpapi::session::SessionMode::NonTunnel,
      client_ip: Some("192.0.2.1".into()),
      client_ipv6: None,
      maintenance: gpapi::session::MaintenanceState::Authenticated,
    };
    task.publish_members(0, vec![member.clone()], vec![]);
    assert!(matches!(*state_rx.borrow(), VpnState::Connecting(_)));
    member.maintenance = gpapi::session::MaintenanceState::Healthy;
    task.publish_members(0, vec![member.clone()], vec![]);
    assert!(matches!(*state_rx.borrow(), VpnState::Connected(_)));
    member.maintenance = gpapi::session::MaintenanceState::Rejected;
    task.publish_members(0, vec![member], vec![]);
    assert!(!matches!(*state_rx.borrow(), VpnState::Connected(_)));
    task.end_attempt(0).await;
  }

  #[tokio::test]
  async fn disconnect_completion_waits_for_the_owned_attempt_task() {
    let (state, state_rx) = watch::channel(VpnState::Disconnected);
    let (mut task, lifecycle) = VpnTask::new(state, false, registry());
    let info = ConnectInfo::new(
      "portal.example.com".into(),
      gpapi::gateway::Gateway::new("vpn".into(), "vpn.example.com".into()),
      vec![],
    );
    assert_eq!(
      lifecycle.submit_test_connection(test_connect_request(info.clone()), None, None, None),
      ServiceResult::Accepted
    );
    let _ = task.events.try_recv().unwrap();
    assert!(lifecycle.activate_attempt(0));
    let cancellation = CancellationToken::new();
    let worker_cancel = cancellation.clone();
    let events = lifecycle.events.clone();
    let (release, wait_release) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
      worker_cancel.cancelled().await;
      events.send(LifecycleEvent::Ended { attempt: 0 }).await.unwrap();
      // An Ended notification alone cannot release service admission. The
      // actor must also join the owned task before admitting another attempt.
      wait_release.await.unwrap();
    });
    task.active = Some(ActiveAttempt {
      attempt: 0,
      session_id: None,
      info,
      allow_extend_session: false,
      cancellation: cancellation.clone(),
      task: worker,
      failure: None,
    });
    assert_eq!(lifecycle.request_disconnect(), ServiceResult::Accepted);
    let stop = task.events.recv().await.unwrap();
    task.handle_event(stop).await;
    assert!(cancellation.is_cancelled());
    assert!(matches!(*state_rx.borrow(), VpnState::Disconnecting));
    let ended = task.events.recv().await.unwrap();
    let owner = tokio::spawn(async move {
      task.handle_event(ended).await;
      task
    });
    tokio::task::yield_now().await;
    assert!(!owner.is_finished());
    assert!(lifecycle.has_attempt(0));
    assert!(matches!(*state_rx.borrow(), VpnState::Disconnecting));
    release.send(()).unwrap();
    let task = owner.await.unwrap();
    assert!(task.active.is_none());
    assert!(!lifecycle.has_attempt(0));
    assert!(matches!(*state_rx.borrow(), VpnState::Disconnected));
  }

  #[tokio::test]
  async fn approval_revocation_suppresses_a_queued_established_event() {
    let (state, state_rx) = watch::channel(VpnState::Disconnected);
    let (mut task, lifecycle) = VpnTask::new(state, false, registry());
    let info = ConnectInfo::new(
      "portal.example.com".into(),
      gpapi::gateway::Gateway::new("vpn".into(), "vpn.example.com".into()),
      vec![],
    );
    assert_eq!(
      lifecycle.submit_test_connection(test_connect_request(info.clone()), None, None, None),
      ServiceResult::Accepted
    );
    let _ = task.events.try_recv().unwrap();
    assert!(lifecycle.activate_attempt(0));
    let cancellation = CancellationToken::new();
    task.active = Some(ActiveAttempt {
      attempt: 0,
      session_id: None,
      info,
      allow_extend_session: true,
      cancellation: cancellation.clone(),
      task: tokio::spawn(async {}),
      failure: None,
    });
    task.send_state(VpnState::Connecting(Box::new(
      task.active.as_ref().unwrap().info.clone(),
    )));
    runtime::stop_for_revoked_approval(&lifecycle, &cancellation);
    assert!(lifecycle.is_stop_requested(0));
    assert!(cancellation.is_cancelled());
    task
      .handle_event(LifecycleEvent::Established {
        attempt: 0,
        session: VpnSessionInfo::default(),
      })
      .await;
    assert!(matches!(*state_rx.borrow(), VpnState::Connecting(_)));
    task.end_attempt(0).await;
  }

  #[tokio::test]
  async fn disconnect_before_start_drains_the_pending_owner() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (mut task, lifecycle) = VpnTask::new(vpn_state_tx, false, registry());
    let request = test_connect_request(ConnectInfo::new(
      "portal.example.com".to_string(),
      gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
      vec![],
    ));

    assert!(matches!(
      lifecycle.submit_test_connection(request, None, None, None),
      ServiceResult::Accepted
    ));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    let start = task.events.try_recv().unwrap();
    task.handle_event(start).await;

    assert!(lifecycle.has_attempt(0));
    assert!(task.active.as_ref().unwrap().cancellation.is_cancelled());
    while task.active.is_some() {
      let event = tokio::time::timeout(std::time::Duration::from_secs(1), task.events.recv())
        .await
        .unwrap()
        .unwrap();
      task.handle_event(event).await;
    }
    assert!(!lifecycle.has_attempt(0));
  }

  #[test]
  fn late_established_event_is_ignored_after_disconnect_is_accepted() {
    let (vpn_state_tx, vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let state_rx = vpn_state_rx.clone();
    let (task, lifecycle) = VpnTask::new(vpn_state_tx, false, registry());
    let info = ConnectInfo::new(
      "portal.example.com".to_string(),
      gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
      vec![],
    );
    let request = test_connect_request(info.clone());

    assert!(matches!(
      lifecycle.submit_test_connection(request, None, None, None),
      ServiceResult::Accepted
    ));
    assert!(lifecycle.activate_attempt(0));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    task.publish_connected(0, info, SessionInfo::default());

    assert!(matches!(*state_rx.borrow(), VpnState::Disconnected));
  }

  #[tokio::test]
  async fn disconnect_waiter_completes_for_its_attempt_before_a_later_connect() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (task, lifecycle) = VpnTask::new(vpn_state_tx, false, registry());
    let request = || {
      test_connect_request(ConnectInfo::new(
        "portal.example.com".to_string(),
        gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
        vec![],
      ))
    };

    assert!(matches!(
      lifecycle.submit_test_connection(request(), None, None, None),
      ServiceResult::Accepted
    ));
    let waiter_lifecycle = lifecycle.clone();
    let waiter = tokio::spawn(async move { waiter_lifecycle.disconnect_and_wait().await });
    while !lifecycle.is_stop_requested(0) {
      tokio::task::yield_now().await;
    }

    task.finish_attempt(0);
    assert!(matches!(
      lifecycle.submit_test_connection(request(), None, None, None),
      ServiceResult::Accepted
    ));
    assert!(
      tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
        .await
        .unwrap()
        .unwrap()
    );
  }

  #[test]
  fn repeated_disconnects_enqueue_one_stop_for_an_attempt() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (mut task, lifecycle) = VpnTask::new(vpn_state_tx, false, registry());
    let request = test_connect_request(ConnectInfo::new(
      "portal.example.com".to_string(),
      gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
      vec![],
    ));

    assert!(matches!(
      lifecycle.submit_test_connection(request, None, None, None),
      ServiceResult::Accepted
    ));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    assert!(matches!(task.events.try_recv(), Ok(LifecycleEvent::Start { .. })));
    assert!(matches!(task.events.try_recv(), Ok(LifecycleEvent::Stop { .. })));
    assert!(matches!(task.events.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
  }

  #[test]
  fn second_connect_is_rejected_while_an_attempt_is_admitted() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (_task, lifecycle) = VpnTask::new(vpn_state_tx, false, registry());
    let request = || {
      test_connect_request(ConnectInfo::new(
        "portal.example.com".to_string(),
        gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
        vec![],
      ))
    };

    assert!(matches!(
      lifecycle.submit_test_connection(request(), None, None, None),
      ServiceResult::Accepted
    ));
    assert!(matches!(
      lifecycle.submit_test_connection(request(), None, None, None),
      ServiceResult::Rejected(rejection) if rejection.code() == ServiceErrorCode::Busy
    ));
  }

  #[test]
  fn maps_openconnect_session_metadata_to_service_session_info() {
    let info = SessionInfo::from_vpn_session_fields(
      Some(43_200),
      None,
      Some(SessionWarning {
        prior_secs: 1_800,
        message: "Session expires soon".to_string(),
      }),
      true,
    );

    assert_eq!(info.lifetime_secs, Some(43_200));
    assert_eq!(info.expires_in_human.as_deref(), Some("12h"));
    assert_eq!(info.lifetime_warning.unwrap().prior_secs, 1_800);
    assert!(info.allow_extend_session);
  }

  #[test]
  fn direct_request_session_metadata_keeps_extension_disabled() {
    let info = SessionInfo::from_vpn_session_fields(
      Some(43_200),
      None,
      Some(SessionWarning {
        prior_secs: 1_800,
        message: "Session expires soon".to_string(),
      }),
      false,
    );

    assert_eq!(info.lifetime_secs, Some(43_200));
    assert!(!info.allow_extend_session);
  }
}
