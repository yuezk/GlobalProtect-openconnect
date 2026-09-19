use std::{
  io::Write,
  os::unix::fs::PermissionsExt,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
  thread,
};

use gpapi::{
  service::{
    request::{ConnectRequest, MAX_CLIENT_IDENTITY_DATA},
    transport::{ServiceErrorCode, ServiceResult},
    vpn_state::{ConnectInfo, ConnectedInfo, VpnState},
  },
  session::{SessionInfo, SessionWarning},
};
use log::{info, warn};
use openconnect::{Vpn, VpnSessionInfo};
use tokio::sync::{Notify, mpsc, watch};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

#[derive(Clone)]
pub(crate) struct LifecycleHandle {
  events: mpsc::Sender<LifecycleEvent>,
  admissions: Arc<Mutex<Admissions>>,
}

impl LifecycleHandle {
  pub fn submit_connect(&self, request: ConnectRequest) -> ServiceResult {
    let attempt = {
      let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
      if admissions.current.is_some() {
        return ServiceResult::rejected(ServiceErrorCode::Busy, "A VPN connection is already in progress");
      }
      let attempt = admissions.next_attempt;
      admissions.next_attempt += 1;
      admissions.current = Some(Admission {
        attempt,
        phase: AdmissionPhase::Pending,
        stop_requested: false,
        completion: Arc::new(AttemptCompletion::default()),
      });
      attempt
    };

    match self.events.try_send(LifecycleEvent::Start { attempt, request }) {
      Ok(()) => ServiceResult::Accepted,
      Err(mpsc::error::TrySendError::Full(_)) => {
        self.clear_attempt(attempt);
        ServiceResult::rejected(ServiceErrorCode::Busy, "VPN lifecycle queue is full")
      }
      Err(mpsc::error::TrySendError::Closed(_)) => {
        self.clear_attempt(attempt);
        ServiceResult::rejected(ServiceErrorCode::Internal, "VPN task is unavailable")
      }
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

    match self.events.try_send(LifecycleEvent::Stop {
      attempt: admission.attempt,
    }) {
      Ok(()) => {
        admission.stop_requested = true;
        admission.phase = AdmissionPhase::Stopping;
        ServiceResult::Accepted
      }
      Err(mpsc::error::TrySendError::Full(_)) => {
        ServiceResult::rejected(ServiceErrorCode::Busy, "VPN lifecycle queue is full")
      }
      Err(mpsc::error::TrySendError::Closed(_)) => {
        ServiceResult::rejected(ServiceErrorCode::Internal, "VPN task is unavailable")
      }
    }
  }

  pub async fn disconnect_and_wait(&self) -> bool {
    let completion = {
      let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
      let Some(admission) = admissions.current.as_mut() else {
        return false;
      };
      if !admission.stop_requested {
        if self
          .events
          .try_send(LifecycleEvent::Stop {
            attempt: admission.attempt,
          })
          .is_err()
        {
          return false;
        }
        admission.stop_requested = true;
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

  fn activate_attempt(&self, attempt: u64) -> bool {
    let mut admissions = self.admissions.lock().expect("VPN lifecycle admissions mutex poisoned");
    let Some(admission) = admissions.current.as_mut() else {
      return false;
    };
    if admission.attempt != attempt || admission.stop_requested || admission.phase != AdmissionPhase::Pending {
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
    let (_task, lifecycle) = VpnTask::new(vpn_state_tx, false, None);
    lifecycle
  }
}

#[derive(Default)]
struct Admissions {
  next_attempt: u64,
  current: Option<Admission>,
}

#[derive(Clone)]
struct Admission {
  attempt: u64,
  phase: AdmissionPhase,
  stop_requested: bool,
  completion: Arc<AttemptCompletion>,
}

#[derive(Default)]
struct AttemptCompletion {
  completed: AtomicBool,
  notify: Notify,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum AdmissionPhase {
  Pending,
  Active,
  Stopping,
}

enum LifecycleEvent {
  Start { attempt: u64, request: ConnectRequest },
  Stop { attempt: u64 },
  Established { attempt: u64, session: VpnSessionInfo },
  Ended { attempt: u64 },
}

struct ActiveAttempt {
  attempt: u64,
  vpn: Arc<Vpn>,
  info: ConnectInfo,
  allow_extend_session: bool,
  _identity_files: Vec<tempfile::NamedTempFile>,
}

pub(crate) struct VpnTask {
  events: mpsc::Receiver<LifecycleEvent>,
  lifecycle: LifecycleHandle,
  vpn_state_tx: watch::Sender<VpnState>,
  brokered_macos: bool,
  trusted_csd_uid: Option<u32>,
  active: Option<ActiveAttempt>,
  cancel_token: CancellationToken,
}

impl VpnTask {
  pub fn new(
    vpn_state_tx: watch::Sender<VpnState>,
    brokered_macos: bool,
    trusted_csd_uid: Option<u32>,
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
      trusted_csd_uid,
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
        self.handle_event(event);
        continue;
      }

      tokio::select! {
        event = self.events.recv() => {
          let Some(event) = event else { break };
          self.handle_event(event);
        }
        _ = cancel_token.cancelled() => {
          info!("VPN task cancelled");
          shutting_down = true;
          self.lifecycle.request_disconnect();
        }
      }
    }

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

  fn handle_event(&mut self, event: LifecycleEvent) {
    match event {
      LifecycleEvent::Start { attempt, request } => self.start_attempt(attempt, request),
      LifecycleEvent::Stop { attempt } => self.stop_attempt(attempt),
      LifecycleEvent::Established { attempt, session } => self.establish_attempt(attempt, session),
      LifecycleEvent::Ended { attempt } => self.end_attempt(attempt),
    }
  }

  fn start_attempt(&mut self, attempt: u64, request: ConnectRequest) {
    if !self.lifecycle.activate_attempt(attempt) {
      self.finish_attempt(attempt);
      return;
    }

    let info = request.info().clone();
    let args = request.args();
    let identity = match prepare_identity(args, self.brokered_macos) {
      Ok(identity) => identity,
      Err(err) => {
        warn!("Failed to prepare client identity: {err}");
        self.finish_attempt(attempt);
        return;
      }
    };
    let allow_extend_session = args.allow_extend_session();
    let vpn_builder = match crate::vpn_script::builder(request.gateway().server(), args.cookie(), args) {
      Ok(builder) => builder,
      Err(err) => {
        warn!("Failed to select the VPNC script: {err}");
        self.finish_attempt(attempt);
        return;
      }
    };
    let csd_uid = match resolve_csd_uid(self.trusted_csd_uid, args.csd_uid(), args.hip()) {
      Ok(uid) => uid,
      Err(err) => {
        warn!("Failed to select the HIP script user: {err}");
        self.finish_attempt(attempt);
        return;
      }
    };
    let vpn = match vpn_builder
      .user_agent(args.user_agent())
      .os(args.openconnect_os())
      .os_version(args.os_version())
      .client_version(args.client_version())
      .host_id(args.host_id())
      .certificate(identity.certificate.clone())
      .sslkey(identity.sslkey.clone())
      .key_password(args.key_password())
      .hip(args.hip())
      .csd_uid(csd_uid)
      .csd_wrapper(args.csd_wrapper())
      .reconnect_timeout(args.reconnect_timeout())
      .mtu(args.mtu())
      .disable_ipv6(args.disable_ipv6())
      .no_dtls(args.no_dtls())
      .local_hostname(args.local_hostname())
      .dpd_interval(args.force_dpd())
      .no_xmlpost(args.no_xmlpost())
      .build()
    {
      Ok(vpn) => Arc::new(vpn),
      Err(err) => {
        warn!("Failed to create VPN: {err}");
        self.finish_attempt(attempt);
        return;
      }
    };

    if self.lifecycle.is_stop_requested(attempt) {
      self.finish_attempt(attempt);
      return;
    }

    let events = self.lifecycle.events.clone();
    let started = {
      let admissions = self
        .lifecycle
        .admissions
        .lock()
        .expect("VPN lifecycle admissions mutex poisoned");
      let can_start = admissions.current.as_ref().is_some_and(|admission| {
        admission.attempt == attempt && admission.phase == AdmissionPhase::Active && !admission.stop_requested
      });
      if !can_start {
        false
      } else {
        self.active = Some(ActiveAttempt {
          attempt,
          vpn: Arc::clone(&vpn),
          info: info.clone(),
          allow_extend_session,
          _identity_files: identity.files,
        });
        self.send_state(VpnState::Connecting(Box::new(info)));
        thread::spawn(move || {
          let established_events = events.clone();
          vpn.connect(move |vpn_session_info| {
            let _ = established_events.blocking_send(LifecycleEvent::Established {
              attempt,
              session: vpn_session_info,
            });
          });
          let _ = events.blocking_send(LifecycleEvent::Ended { attempt });
        });
        true
      }
    };
    if !started {
      self.finish_attempt(attempt);
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
    active.vpn.disconnect();
  }

  fn establish_attempt(&mut self, attempt: u64, vpn_session_info: VpnSessionInfo) {
    let Some(active) = self.active.as_ref() else {
      return;
    };
    if active.attempt != attempt {
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

  fn end_attempt(&mut self, attempt: u64) {
    if self.active.as_ref().is_none_or(|active| active.attempt != attempt) {
      return;
    }
    self.active = None;
    self.finish_attempt(attempt);
  }

  fn finish_attempt(&self, attempt: u64) {
    self.send_state(VpnState::Disconnected);
    self.lifecycle.clear_attempt(attempt);
  }

  fn send_state(&self, state: VpnState) {
    let _ = self.vpn_state_tx.send(state);
  }
}

fn resolve_csd_uid(trusted_uid: Option<u32>, requested_uid: u32, hip_enabled: bool) -> anyhow::Result<u32> {
  let uid = trusted_uid.unwrap_or(requested_uid);
  if hip_enabled && uid == 0 {
    anyhow::bail!("HIP scripts must not run as root");
  }
  Ok(uid)
}

struct PreparedIdentity {
  certificate: Option<String>,
  sslkey: Option<String>,
  files: Vec<tempfile::NamedTempFile>,
}

fn prepare_identity(
  args: &gpapi::service::request::ConnectArgs,
  brokered_macos: bool,
) -> anyhow::Result<PreparedIdentity> {
  if !brokered_macos {
    return Ok(PreparedIdentity {
      certificate: args.certificate(),
      sslkey: args.sslkey(),
      files: vec![],
    });
  }

  let certificate = args.certificate_data()?.map(Zeroizing::new);
  let sslkey = args.sslkey_data()?.map(Zeroizing::new);
  let total_size = certificate.as_ref().map_or(0, |data| data.len()) + sslkey.as_ref().map_or(0, |data| data.len());
  if total_size > MAX_CLIENT_IDENTITY_DATA {
    anyhow::bail!("Client certificate and key exceed the macOS size limit");
  }

  let mut files = Vec::new();
  let certificate = write_identity_file(certificate, &mut files)?;
  let sslkey = write_identity_file(sslkey, &mut files)?;
  Ok(PreparedIdentity {
    certificate,
    sslkey,
    files,
  })
}

fn write_identity_file(
  data: Option<Zeroizing<Vec<u8>>>,
  files: &mut Vec<tempfile::NamedTempFile>,
) -> anyhow::Result<Option<String>> {
  let Some(data) = data else { return Ok(None) };
  let mut file = tempfile::NamedTempFile::new_in("/var/run/com.yuezk.gpgui")?;
  file.as_file().set_permissions(std::fs::Permissions::from_mode(0o600))?;
  file.write_all(&data)?;
  file.flush()?;
  let path = file.path().to_string_lossy().into_owned();
  files.push(file);
  Ok(Some(path))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn disconnect_before_start_discards_the_pending_attempt() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (mut task, lifecycle) = VpnTask::new(vpn_state_tx, false, None);
    let request = ConnectRequest::new(
      ConnectInfo::new(
        "portal.example.com".to_string(),
        gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
        vec![],
      ),
      "cookie".to_string(),
    );

    assert!(matches!(lifecycle.submit_connect(request), ServiceResult::Accepted));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    let start = task.events.try_recv().unwrap();
    task.handle_event(start);

    assert!(!lifecycle.has_attempt(0));
    assert!(task.active.is_none());
  }

  #[test]
  fn late_established_event_is_ignored_after_disconnect_is_accepted() {
    let (vpn_state_tx, vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let state_rx = vpn_state_rx.clone();
    let (task, lifecycle) = VpnTask::new(vpn_state_tx, false, None);
    let info = ConnectInfo::new(
      "portal.example.com".to_string(),
      gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
      vec![],
    );
    let request = ConnectRequest::new(info.clone(), "cookie".to_string());

    assert!(matches!(lifecycle.submit_connect(request), ServiceResult::Accepted));
    assert!(lifecycle.activate_attempt(0));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    task.publish_connected(0, info, SessionInfo::default());

    assert!(matches!(*state_rx.borrow(), VpnState::Disconnected));
  }

  #[tokio::test]
  async fn disconnect_waiter_completes_for_its_attempt_before_a_later_connect() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (task, lifecycle) = VpnTask::new(vpn_state_tx, false, None);
    let request = || {
      ConnectRequest::new(
        ConnectInfo::new(
          "portal.example.com".to_string(),
          gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
          vec![],
        ),
        "cookie".to_string(),
      )
    };

    assert!(matches!(lifecycle.submit_connect(request()), ServiceResult::Accepted));
    let waiter_lifecycle = lifecycle.clone();
    let waiter = tokio::spawn(async move { waiter_lifecycle.disconnect_and_wait().await });
    while !lifecycle.is_stop_requested(0) {
      tokio::task::yield_now().await;
    }

    task.finish_attempt(0);
    assert!(matches!(lifecycle.submit_connect(request()), ServiceResult::Accepted));
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
    let (mut task, lifecycle) = VpnTask::new(vpn_state_tx, false, None);
    let request = ConnectRequest::new(
      ConnectInfo::new(
        "portal.example.com".to_string(),
        gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
        vec![],
      ),
      "cookie".to_string(),
    );

    assert!(matches!(lifecycle.submit_connect(request), ServiceResult::Accepted));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    assert!(matches!(lifecycle.request_disconnect(), ServiceResult::Accepted));
    assert!(matches!(task.events.try_recv(), Ok(LifecycleEvent::Start { .. })));
    assert!(matches!(task.events.try_recv(), Ok(LifecycleEvent::Stop { .. })));
    assert!(matches!(task.events.try_recv(), Err(mpsc::error::TryRecvError::Empty)));
  }

  #[test]
  fn second_connect_is_rejected_while_an_attempt_is_admitted() {
    let (vpn_state_tx, _vpn_state_rx) = watch::channel(VpnState::Disconnected);
    let (_task, lifecycle) = VpnTask::new(vpn_state_tx, false, None);
    let request = || {
      ConnectRequest::new(
        ConnectInfo::new(
          "portal.example.com".to_string(),
          gpapi::gateway::Gateway::new("vpn".to_string(), "vpn.example.com".to_string()),
          vec![],
        ),
        "cookie".to_string(),
      )
    };

    assert!(matches!(lifecycle.submit_connect(request()), ServiceResult::Accepted));
    assert!(matches!(
      lifecycle.submit_connect(request()),
      ServiceResult::Rejected(rejection) if rejection.code() == ServiceErrorCode::Busy
    ));
  }

  #[test]
  fn trusted_csd_uid_overrides_request_uid() {
    assert_eq!(resolve_csd_uid(Some(1000), 0, true).unwrap(), 1000);
  }

  #[test]
  fn root_csd_uid_is_rejected_when_hip_is_enabled() {
    assert!(resolve_csd_uid(None, 0, true).is_err());
    assert_eq!(resolve_csd_uid(None, 0, false).unwrap(), 0);
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
