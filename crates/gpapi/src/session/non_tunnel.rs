use std::{
  future::Future,
  io,
  pin::Pin,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};

use crate::{gateway::Gateway, process::collection::CollectionControl};

use super::{
  GatewaySession, GatewaySessionSummary, GatewaySessions, InternalSessionPolicy, MaintenanceState, SessionEndReason,
  SessionMode, transport::GatewayRequestError,
};

const COLLECTION_TIMEOUT: Duration = Duration::from_secs(60);

/// This request is deliberately independent of OpenConnect's callback/FFI types.
#[derive(Clone)]
pub struct SessionHipRequest {
  pub gateway: Gateway,
  pub cookie: String,
  pub client_ip: Option<String>,
  pub client_ipv6: Option<String>,
  pub md5: String,
  pub collect_posture: bool,
}

impl Drop for SessionHipRequest {
  fn drop(&mut self) {
    self.cookie.zeroize();
  }
}

pub struct SessionCollectionControl {
  cancellation: CancellationToken,
  deadline: Instant,
}

impl CollectionControl for SessionCollectionControl {
  fn check(&self) -> io::Result<()> {
    if self.cancellation.is_cancelled() {
      Err(io::Error::new(io::ErrorKind::Interrupted, "HIP collection cancelled"))
    } else if Instant::now() >= self.deadline {
      Err(io::Error::new(io::ErrorKind::TimedOut, "HIP collection timed out"))
    } else {
      Ok(())
    }
  }
}

pub type SessionReportProducer =
  dyn Fn(&SessionHipRequest, &SessionCollectionControl) -> io::Result<String> + Send + Sync;
pub type SessionReportObserver = dyn Fn(&SessionHipRequest, &str) + Send + Sync;
pub type SessionStatusObserver = dyn Fn(GatewaySessionSummary) + Send + Sync;
pub type SessionNetworkValidator =
  dyn Fn(CancellationToken) -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send + Sync;

#[derive(Clone)]
pub struct SessionCallbacks {
  pub produce_report: Arc<SessionReportProducer>,
  pub report_submitted: Arc<SessionReportObserver>,
  pub status_changed: Arc<SessionStatusObserver>,
  pub validate_network: Arc<SessionNetworkValidator>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionExit {
  Disconnected,
  Failed(SessionEndReason),
}

/// Owns maintenance workers until they have stopped. The session owner retains
/// its ledger and logs out after joining every worker. Interactive authentication
/// also belongs to that owner. Cancel and await this future, never abort/drop it:
/// blocking collectors must finish before cleanup releases their resources.
pub async fn maintain_non_tunnel_sessions(
  sessions: &mut GatewaySessions,
  policy: InternalSessionPolicy,
  mut callbacks: SessionCallbacks,
  cancellation: CancellationToken,
  network_change: impl Future<Output = SessionEndReason>,
) -> anyhow::Result<SessionExit> {
  anyhow::ensure!(!sessions.is_empty(), "Non-tunnel session set is empty");
  anyhow::ensure!(
    sessions
      .members()
      .iter()
      .all(|session| session.transport.authentication().mode() == SessionMode::NonTunnel),
    "Tunnel member supplied to non-tunnel runtime"
  );
  let workers_cancel = cancellation.child_token();
  let observer = callbacks.status_changed.clone();
  let status_gate = Arc::new(Mutex::new(()));
  let gate = status_gate.clone();
  let cancelled = workers_cancel.clone();
  let worker_observer = observer.clone();
  callbacks.status_changed = Arc::new(move |summary| {
    let _guard = gate.lock().unwrap_or_else(|error| error.into_inner());
    if !cancelled.is_cancelled() {
      worker_observer(summary);
    }
  });
  let mut workers = JoinSet::new();
  for session in sessions.members() {
    let session = session.clone();
    let callbacks = callbacks.clone();
    let cancellation = workers_cancel.clone();
    workers.spawn(async move {
      let server = session.gateway.server().to_owned();
      (server, maintain_member(session, policy, callbacks, cancellation).await)
    });
  }
  tokio::pin!(network_change);
  let mut worker_failure = None;
  let outcome = loop {
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => break SessionExit::Disconnected,
      reason = &mut network_change => break SessionExit::Failed(reason),
      result = workers.join_next() => match result {
        Some(Ok((_, MemberExit::Terminal(reason)))) => break SessionExit::Failed(reason),
        Some(Ok((server, MemberExit::Rejected))) => {
          sessions.logout_gateway(&server).await;
          if workers.is_empty() { break SessionExit::Failed(SessionEndReason::AllMembersFailed); }
        },
        Some(Ok((_, MemberExit::Cancelled))) => {},
        Some(Err(error)) => { worker_failure = Some(error); break SessionExit::Disconnected; },
        None => break SessionExit::Failed(SessionEndReason::AllMembersFailed),
      },
    }
  };
  workers_cancel.cancel();
  if matches!(outcome, SessionExit::Failed(_)) || worker_failure.is_some() {
    // Invalidate advertised health before waiting for collectors or remote
    // cleanup. Cancellation prevents workers from publishing Healthy afterwards.
    let _guard = status_gate.lock().unwrap_or_else(|error| error.into_inner());
    for session in sessions.members() {
      observer(GatewaySessionSummary {
        gateway: session.gateway.clone(),
        mode: SessionMode::NonTunnel,
        client_ip: session.addresses.ipv4.map(|ip| ip.to_string()),
        client_ipv6: session.addresses.ipv6.map(|ip| ip.to_string()),
        maintenance: MaintenanceState::Rejected,
      });
    }
  }
  // Do not abort workers: a blocking collector must observe cancellation and be
  // joined before approval/identity resources can be released by the caller.
  while let Some(result) = workers.join_next().await {
    if let Err(error) = result {
      worker_failure.get_or_insert(error);
    }
  }
  if let Some(error) = worker_failure {
    return Err(error.into());
  }
  // Deliberate disconnect wins even when it arrives during coordinated cleanup.
  if cancellation.is_cancelled() {
    Ok(SessionExit::Disconnected)
  } else {
    Ok(outcome)
  }
}

async fn maintain_member(
  session: Arc<GatewaySession>,
  policy: InternalSessionPolicy,
  callbacks: SessionCallbacks,
  cancellation: CancellationToken,
) -> MemberExit {
  let request = SessionHipRequest {
    gateway: session.gateway.clone(),
    cookie: session.transport.authentication().cookie().into(),
    client_ip: session.addresses.ipv4.map(|ip| ip.to_string()),
    client_ipv6: session.addresses.ipv6.map(|ip| ip.to_string()),
    md5: session.transport.hip_token(),
    collect_posture: policy.collect_hip_data(),
  };
  let status = |maintenance| {
    if !cancellation.is_cancelled() {
      (callbacks.status_changed)(GatewaySessionSummary {
        gateway: session.gateway.clone(),
        mode: SessionMode::NonTunnel,
        client_ip: request.client_ip.clone(),
        client_ipv6: request.client_ipv6.clone(),
        maintenance,
      });
    }
  };
  status(MaintenanceState::Authenticated);
  loop {
    if (callbacks.validate_network)(cancellation.clone()).await.is_err() {
      return MemberExit::Terminal(SessionEndReason::NetworkChanged);
    }
    let sent_at = tokio::time::Instant::now();
    let check = tokio::select! {
      biased;
      _ = cancellation.cancelled() => return MemberExit::Cancelled,
      result = session.transport.check_hip(session.addresses.ipv4.map(Into::into), session.addresses.ipv6.map(Into::into)) => result,
    };
    let (result, delay) = match check {
      Ok(check) => {
        let result = if check.report_needed {
          collect_and_submit(&session, &request, &callbacks, &cancellation).await
        } else {
          Ok(())
        };
        (result, check.delay)
      }
      Err(error) => (Err(MemberError::Gateway(error)), Duration::ZERO),
    };
    let next_check = match result {
      Ok(()) => {
        if cancellation.is_cancelled() {
          return MemberExit::Cancelled;
        }
        status(MaintenanceState::Healthy);
        let Some(next_check) = sent_at
          .checked_add(policy.hip_interval())
          .and_then(|time| time.checked_add(delay))
        else {
          status(MaintenanceState::Rejected);
          return MemberExit::Rejected;
        };
        next_check
      }
      Err(MemberError::Gateway(GatewayRequestError::InvalidCookie)) => {
        status(MaintenanceState::Rejected);
        return MemberExit::Terminal(SessionEndReason::InvalidCookie);
      }
      Err(MemberError::Network) => return MemberExit::Terminal(SessionEndReason::NetworkChanged),
      Err(MemberError::Authorization) => return MemberExit::Terminal(SessionEndReason::AuthorizationRevoked),
      Err(error) => {
        if cancellation.is_cancelled() {
          return MemberExit::Cancelled;
        }
        status(MaintenanceState::Rejected);
        log::warn!("Gateway maintenance failed: {error}");
        return MemberExit::Rejected;
      }
    };
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => return MemberExit::Cancelled,
      _ = tokio::time::sleep_until(next_check) => {},
    }
  }
}

enum MemberExit {
  Cancelled,
  Rejected,
  Terminal(SessionEndReason),
}

#[derive(Debug, thiserror::Error)]
enum MemberError {
  #[error("{0}")]
  Gateway(#[from] GatewayRequestError),
  // Collector/script errors can contain user-provided text. Do not log payloads.
  #[error("HIP report collection failed")]
  Collection,
  #[error("Physical network validation failed")]
  Network,
  #[error("HIP report authorization was revoked")]
  Authorization,
}

async fn collect_and_submit(
  session: &GatewaySession,
  request: &SessionHipRequest,
  callbacks: &SessionCallbacks,
  cancellation: &CancellationToken,
) -> Result<(), MemberError> {
  let producer = callbacks.produce_report.clone();
  let input = request.clone();
  let control = SessionCollectionControl {
    cancellation: cancellation.clone(),
    deadline: Instant::now() + COLLECTION_TIMEOUT,
  };
  let report = tokio::task::spawn_blocking(move || {
    control.check()?;
    let report = Zeroizing::new(producer(&input, &control)?);
    control.check()?;
    Ok::<_, io::Error>(report)
  })
  .await
  .map_err(|_| MemberError::Collection)?
  .map_err(|error| {
    if error.kind() == io::ErrorKind::PermissionDenied {
      MemberError::Authorization
    } else {
      MemberError::Collection
    }
  })?;
  (callbacks.validate_network)(cancellation.clone())
    .await
    .map_err(|_| MemberError::Network)?;
  tokio::select! {
    biased;
    _ = cancellation.cancelled() => return Err(MemberError::Collection),
    result = session.transport.submit_hip(&report, session.addresses.ipv4.map(Into::into), session.addresses.ipv6.map(Into::into)) => result?,
  }
  if !cancellation.is_cancelled() {
    (callbacks.report_submitted)(request, &report);
  }
  Ok(())
}

#[cfg(test)]
mod tests;
