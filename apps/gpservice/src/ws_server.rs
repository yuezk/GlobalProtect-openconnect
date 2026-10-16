use std::{collections::HashMap, sync::Arc};

use common::binary_paths;
use gpapi::{
  os_profile::HostIdentity,
  service::{
    event::WsEvent,
    transport::{CloseReason, ServerMessage},
    vpn_env::{HostInfo, VpnEnv},
    vpn_state::VpnState,
  },
  session::tunnel::{TunnelStatisticsSnapshot, TunnelUnavailableReason},
  utils::lock_file::LockFile,
};
use log::{info, warn};
use openconnect::find_vpnc_script;
use tokio::{
  net::TcpListener,
  sync::{Mutex, Semaphore, mpsc, oneshot, watch},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
  request_dispatcher::RequestDispatcher,
  routes,
  session_registry::{HandshakePermit, PreparedAttach, SessionError, SessionRegistry},
  ws_connection::{ConnectionCommand, ConnectionControl},
};

struct ServiceState {
  revision: u64,
  snapshot: VpnEnv,
  connections: HashMap<Uuid, ConnectionControl>,
  tunnel: crate::vpn_task::TunnelBinding,
}

impl ServiceState {
  fn refresh_demand(&self) {
    if let Some(monitor) = &self.tunnel.monitor {
      monitor.set_statistics_enabled(
        self
          .connections
          .values()
          .any(|connection| connection.statistics_enabled() && !connection.is_closed()),
      );
    }
  }

  fn snapshot(&self) -> VpnEnv {
    let mut snapshot = self.snapshot.clone();
    if let Some(monitor) = &self.tunnel.monitor {
      snapshot.tunnel_state = Some(monitor.state());
    }
    snapshot
  }
}

pub(crate) struct WsServerContext {
  product_version: &'static str,
  registry: Arc<SessionRegistry>,
  dispatcher: Arc<RequestDispatcher>,
  state: Mutex<ServiceState>,
  handshake_slots: Arc<Semaphore>,
  pub(crate) hip_previews: crate::hip_preview::HipPreviews,
}

impl WsServerContext {
  pub fn new(
    product_version: &'static str,
    registry: Arc<SessionRegistry>,
    dispatcher: Arc<RequestDispatcher>,
    vpn_state_rx: &watch::Receiver<VpnState>,
  ) -> Self {
    Self {
      product_version,
      registry,
      dispatcher,
      state: Mutex::new(ServiceState {
        revision: 0,
        snapshot: build_snapshot(vpn_state_rx.borrow().clone()),
        connections: HashMap::new(),
        tunnel: Default::default(),
      }),
      handshake_slots: Arc::new(Semaphore::new(16)),
      hip_previews: crate::hip_preview::HipPreviews::new(),
    }
  }

  pub fn product_version(&self) -> &'static str {
    self.product_version
  }

  pub async fn host_identity(&self) -> HostIdentity {
    self.state.lock().await.snapshot.host_info.host_identity.clone()
  }

  pub fn registry(&self) -> &Arc<SessionRegistry> {
    &self.registry
  }

  pub fn dispatcher(&self) -> Arc<RequestDispatcher> {
    Arc::clone(&self.dispatcher)
  }

  pub fn try_handshake_slot(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
    Arc::clone(&self.handshake_slots).try_acquire_owned().ok()
  }

  pub async fn prepare_connection(
    &self,
    permit: &HandshakePermit,
    control: ConnectionControl,
  ) -> Result<PreparedAttach, SessionError> {
    let mut state = self.state.lock().await;
    let prepared = self.registry.prepare_attach(permit)?;

    let mut attached = ServerMessage::Attached {
      connection_id: control.connection_id(),
      current_revision: state.revision,
      snapshot: state.snapshot(),
    };
    fit_snapshot_message(&mut attached).map_err(|_| SessionError::Unavailable)?;
    control
      .send(ConnectionCommand::Send(attached))
      .map_err(|_| SessionError::Unavailable)?;
    state.connections.insert(control.connection_id(), control);
    Ok(prepared)
  }

  pub async fn commit_connection(
    &self,
    permit: &HandshakePermit,
    connection: &ConnectionControl,
  ) -> Result<Option<ConnectionControl>, SessionError> {
    let mut state = self.state.lock().await;
    if !state
      .connections
      .get(&connection.connection_id())
      .is_some_and(|candidate| !candidate.is_closed())
    {
      return Err(SessionError::Unavailable);
    }
    match self.registry.commit_attach(permit, connection.clone()) {
      Ok(replaced) => {
        connection.mark_committed();
        if let Some(replaced) = &replaced {
          replaced.set_statistics_enabled(false);
          state.connections.remove(&replaced.connection_id());
        }
        state.refresh_demand();
        Ok(replaced)
      }
      Err(err) => {
        state.connections.remove(&connection.connection_id());
        state.refresh_demand();
        Err(err)
      }
    }
  }

  pub async fn abandon_connection(&self, connection_id: Uuid) {
    let mut state = self.state.lock().await;
    state.connections.remove(&connection_id);
    state.refresh_demand();
  }

  pub async fn remove_closed_connections(&self) {
    let mut state = self.state.lock().await;
    state.connections.retain(|_, connection| !connection.is_closed());
    state.refresh_demand();
  }

  pub async fn send_snapshot(&self, control: &ConnectionControl) -> Result<(), ()> {
    let state = self.state.lock().await;
    let mut message = ServerMessage::Snapshot {
      current_revision: state.revision,
      snapshot: state.snapshot(),
    };
    fit_snapshot_message(&mut message)?;
    control.send(ConnectionCommand::Send(message))
  }

  pub async fn remove_connection(&self, connection_id: Uuid, session_id: Uuid, generation: u64) {
    self.registry.disconnect_if_current(session_id, generation);
    let mut state = self.state.lock().await;
    state.connections.remove(&connection_id);
    state.refresh_demand();
  }

  pub async fn send_event(&self, event: WsEvent) {
    let mut state = self.state.lock().await;
    if let WsEvent::VpnEnv(env) = &event {
      state.snapshot = env.clone();
    } else if let WsEvent::VpnState(vpn_state) = &event {
      state.snapshot.vpn_state = vpn_state.clone();
    } else if let WsEvent::TunnelState(tunnel) = &event {
      state.snapshot.tunnel_state = tunnel.clone();
    }

    let Some(revision) = state.revision.checked_add(1) else {
      for connection in state.connections.values() {
        connection.close(CloseReason::InternalError);
      }
      state.connections.clear();
      state.refresh_demand();
      return;
    };
    state.revision = revision;

    state.connections.retain(|_, connection| {
      if connection
        .send(ConnectionCommand::Send(ServerMessage::Event {
          revision,
          event: event.clone(),
        }))
        .is_ok()
      {
        true
      } else {
        connection.close(CloseReason::InternalError);
        false
      }
    });
    state.refresh_demand();
  }

  pub async fn set_statistics_enabled(
    &self,
    control: &ConnectionControl,
    session_id: Uuid,
    generation: u64,
    enabled: bool,
  ) -> gpapi::service::transport::ServiceResult {
    use gpapi::service::transport::{ServiceErrorCode, ServiceResult};
    let state = self.state.lock().await;
    if !control.is_committed()
      || control.is_closed()
      || !state.connections.contains_key(&control.connection_id())
      || !self.registry.is_current(session_id, generation)
    {
      return ServiceResult::rejected(
        ServiceErrorCode::InvalidRequest,
        "Service attachment is no longer active",
      );
    }
    control.set_statistics_enabled(enabled);
    state.refresh_demand();
    let snapshot = match &state.tunnel.monitor {
      Some(monitor) => monitor.statistics(),
      None => TunnelStatisticsSnapshot::Unavailable {
        reason: if state.tunnel.non_tunnel {
          TunnelUnavailableReason::NonTunnel
        } else {
          TunnelUnavailableReason::NoTunnel
        },
      },
    };
    ServiceResult::TunnelStatisticsSnapshot(if enabled { snapshot } else { snapshot.disabled() })
  }

  async fn bind_tunnel(&self, binding: crate::vpn_task::TunnelBinding) {
    let mut state = self.state.lock().await;
    if let Some(previous) = &state.tunnel.monitor {
      previous.set_statistics_enabled(false);
    }
    state.tunnel = binding;
    for connection in state.connections.values() {
      connection.clear_statistics();
    }
    state.refresh_demand();
    let tunnel_state = state.tunnel.monitor.as_ref().map(|monitor| monitor.state());
    drop(state);
    self.send_event(WsEvent::TunnelState(tunnel_state)).await;
    let state = self.state.lock().await;
    let snapshot = match &state.tunnel.monitor {
      Some(monitor) => monitor.statistics(),
      None => TunnelStatisticsSnapshot::unavailable(if state.tunnel.non_tunnel {
        TunnelUnavailableReason::NonTunnel
      } else {
        TunnelUnavailableReason::NoTunnel
      }),
    };
    for connection in state.connections.values() {
      connection.publish_statistics(snapshot.clone());
    }
  }

  async fn publish_tunnel(&self, attempt: u64) {
    let state = self.state.lock().await;
    if state.tunnel.attempt != attempt {
      return;
    }
    let Some(monitor) = &state.tunnel.monitor else {
      return;
    };
    let current = monitor.state();
    let changed = state
      .snapshot
      .tunnel_state
      .as_ref()
      .is_none_or(|previous| previous.tunnel_id != current.tunnel_id || previous.sequence != current.sequence);
    drop(state);
    if changed {
      self.send_event(WsEvent::TunnelState(Some(current))).await;
    }
    let state = self.state.lock().await;
    if state.tunnel.attempt != attempt {
      return;
    }
    if let Some(monitor) = &state.tunnel.monitor {
      for connection in state.connections.values() {
        connection.publish_statistics(monitor.statistics());
      }
    }
  }
}

pub(crate) struct WsServer {
  ctx: Arc<WsServerContext>,
  cancel_token: CancellationToken,
  lock_file: Arc<LockFile>,
  vpn_state_rx: watch::Receiver<VpnState>,
}

impl WsServer {
  pub fn new(
    product_version: &'static str,
    registry: Arc<SessionRegistry>,
    dispatcher: Arc<RequestDispatcher>,
    vpn_state_rx: watch::Receiver<VpnState>,
    lock_file: Arc<LockFile>,
  ) -> Self {
    Self {
      ctx: Arc::new(WsServerContext::new(
        product_version,
        registry,
        dispatcher,
        &vpn_state_rx,
      )),
      cancel_token: CancellationToken::new(),
      lock_file,
      vpn_state_rx,
    }
  }

  pub fn context(&self) -> Arc<WsServerContext> {
    Arc::clone(&self.ctx)
  }

  pub fn cancel_token(&self) -> CancellationToken {
    self.cancel_token.clone()
  }

  pub async fn start(&self, shutdown_tx: mpsc::Sender<()>, ready_tx: oneshot::Sender<()>) {
    let listener = match self.start_tcp_server().await {
      Ok(listener) => listener,
      Err(err) => {
        warn!("Failed to start WS server: {err}");
        let _ = shutdown_tx.send(()).await;
        return;
      }
    };
    let _ = ready_tx.send(());

    tokio::select! {
      _ = watch_vpn_state(self.vpn_state_rx.clone(), Arc::clone(&self.ctx)) => {
        info!("VPN state watch task completed");
      }
      _ = watch_tunnel(self.ctx.dispatcher.subscribe_tunnel(), Arc::clone(&self.ctx)) => {}
      result = axum::serve(listener, routes::routes(Arc::clone(&self.ctx))) => {
        if let Err(err) = result {
          warn!("WS server stopped: {err}");
        }
      }
      _ = self.cancel_token.cancelled() => {
        info!("WS server cancelled");
      }
    }
    self.ctx.hip_previews.shutdown().await;
    let _ = shutdown_tx.send(()).await;
  }

  async fn start_tcp_server(&self) -> anyhow::Result<TcpListener> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    info!("WS server listening on port: {port}");
    self.lock_file.lock(&port.to_string())?;
    Ok(listener)
  }
}

fn build_snapshot(vpn_state: VpnState) -> VpnEnv {
  VpnEnv {
    tunnel_state: None,
    vpn_state,
    vpnc_script: find_vpnc_script(),
    auth_executable: binary_paths::gpauth().to_string_lossy().into_owned(),
    host_info: HostInfo {
      host_identity: HostIdentity::collect(),
    },
  }
}

/// Apply the observation's explicit truncation policy to the space remaining in
/// a complete envelope. Existing host/connection facts are never shortened.
fn fit_snapshot_message(message: &mut ServerMessage) -> Result<(), ()> {
  let encoded_size = serde_json::to_vec(&message).map_err(|_| ())?.len();
  let Some(excess) = encoded_size.checked_sub(gpapi::service::transport::MAX_PLAINTEXT) else {
    return Ok(());
  };
  if excess == 0 {
    return Ok(());
  }
  let snapshot = match message {
    ServerMessage::Attached { snapshot, .. } | ServerMessage::Snapshot { snapshot, .. } => snapshot,
    _ => return Err(()),
  };
  let configuration = snapshot
    .tunnel_state
    .as_mut()
    .and_then(|state| state.configuration.as_mut())
    .ok_or(())?;
  let configuration_size = serde_json::to_vec(&configuration).map_err(|_| ())?.len();
  let remaining = configuration_size.checked_sub(excess).ok_or(())?;
  if !configuration.limit_encoded_size(remaining) {
    return Err(());
  }
  if serde_json::to_vec(message).map_err(|_| ())?.len() > gpapi::service::transport::MAX_PLAINTEXT {
    return Err(());
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, atomic::AtomicBool};

  use gpapi::{
    service::{event::WsEvent, transport::ServerMessage},
    utils::redact::Redaction,
  };
  use tokio::sync::{mpsc, watch};

  use super::*;

  fn context() -> (Arc<WsServerContext>, Arc<SessionRegistry>) {
    let registry = Arc::new(SessionRegistry::for_tests(Uuid::new_v4()));
    let dispatcher = Arc::new(RequestDispatcher::new(
      crate::vpn_task::LifecycleHandle::for_tests(),
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      true,
      false,
    ));
    let (_, state_rx) = watch::channel(VpnState::Disconnected);
    (
      Arc::new(WsServerContext::new(
        env!("CARGO_PKG_VERSION"),
        registry.clone(),
        dispatcher,
        &state_rx,
      )),
      registry,
    )
  }

  async fn attach(
    context: &WsServerContext,
    registry: &Arc<SessionRegistry>,
  ) -> (ConnectionControl, Uuid, u64, mpsc::Receiver<ConnectionCommand>) {
    let credential = registry.issue(env!("CARGO_PKG_VERSION"), None).unwrap();
    let permit = registry
      .begin_handshake(
        credential.service_instance_id(),
        credential.session_id(),
        credential.product_version(),
      )
      .unwrap();
    let (tx, mut rx) = mpsc::channel(8);
    let (control, _) = ConnectionControl::new(Uuid::new_v4(), tx);
    let prepared = context.prepare_connection(&permit, control.clone()).await.unwrap();
    rx.recv().await.unwrap();
    context.commit_connection(&permit, &control).await.unwrap();
    (control, credential.session_id(), prepared.generation, rx)
  }

  #[tokio::test]
  async fn statistics_demand_is_per_attachment_and_does_not_consume_revisions() {
    use gpapi::service::transport::ServiceResult;
    use gpapi::session::tunnel::TunnelStatisticsState;
    let (context, registry) = context();
    let (a, a_session, a_generation, _a_rx) = attach(&context, &registry).await;
    let (b, b_session, b_generation, _b_rx) = attach(&context, &registry).await;
    let vpn = Arc::new(
      openconnect::Vpn::builder("vpn.example.com", "cookie")
        .network_configuration_handler(|_| Ok(()))
        .build()
        .unwrap(),
    );
    let monitor = gptunnel::TunnelMonitor::attach(vpn);
    context
      .bind_tunnel(crate::vpn_task::TunnelBinding {
        attempt: 1,
        monitor: Some(monitor.clone()),
        non_tunnel: false,
      })
      .await;
    let revision = context.state.lock().await.revision;
    for (connection, session, generation) in [(&a, a_session, a_generation), (&b, b_session, b_generation)] {
      assert!(matches!(
        context
          .set_statistics_enabled(connection, session, generation, true)
          .await,
        ServiceResult::TunnelStatisticsSnapshot(_)
      ));
    }
    assert_eq!(
      monitor.statistics().view().unwrap().state,
      TunnelStatisticsState::Waiting
    );
    let ServiceResult::TunnelStatisticsSnapshot(disabled) =
      context.set_statistics_enabled(&b, b_session, b_generation, false).await
    else {
      panic!("expected snapshot");
    };
    assert_eq!(disabled.view().unwrap().state, TunnelStatisticsState::Disabled);
    assert_eq!(
      monitor.statistics().view().unwrap().state,
      TunnelStatisticsState::Waiting
    );
    context
      .remove_connection(a.connection_id(), a_session, a_generation)
      .await;
    assert_eq!(
      monitor.statistics().view().unwrap().state,
      TunnelStatisticsState::Disabled
    );
    assert_eq!(context.state.lock().await.revision, revision);
    monitor.stop().await;
  }

  #[tokio::test]
  async fn sample_slot_coalesces_without_consuming_control_queue_and_disable_clears_it() {
    let (context, registry) = context();
    let (control, session, generation, mut commands) = attach(&context, &registry).await;
    context
      .set_statistics_enabled(&control, session, generation, true)
      .await;
    let mut samples = control.subscribe_statistics();
    let snapshot = TunnelStatisticsSnapshot::unavailable(TunnelUnavailableReason::NoTunnel);
    for _ in 0..1000 {
      control.publish_statistics(snapshot.clone());
    }
    assert_eq!(samples.borrow_and_update().as_ref(), Some(&snapshot));
    assert!(commands.try_recv().is_err());
    context
      .set_statistics_enabled(&control, session, generation, false)
      .await;
    assert!(samples.borrow().is_none());
    control.publish_statistics(snapshot);
    assert!(samples.borrow().is_none());
  }

  #[test]
  fn snapshot_budget_trims_observation_without_changing_existing_metadata() {
    use gpapi::session::tunnel::{TunnelConfiguration, TunnelPhase, TunnelState, TunnelTransport};
    let mut snapshot = build_snapshot(VpnState::Disconnected);
    let mut configuration = TunnelConfiguration::default();
    configuration.included_routes.items = vec!["r".repeat(1000); 15];
    configuration.included_routes.total_count = 15;
    snapshot.tunnel_state = Some(TunnelState {
      tunnel_id: Uuid::nil(),
      sequence: 4,
      phase: TunnelPhase::Connected,
      configuration_revision: 1,
      configuration: Some(configuration),
      transport: TunnelTransport::IpsecEsp,
      elapsed_ms: 600_000,
    });
    let original_metadata = "a".repeat(52_000);
    snapshot.auth_executable = original_metadata.clone();
    let mut message = ServerMessage::Snapshot {
      current_revision: 9,
      snapshot,
    };
    assert!(serde_json::to_vec(&message).unwrap().len() > gpapi::service::transport::MAX_PLAINTEXT);
    fit_snapshot_message(&mut message).unwrap();
    assert!(serde_json::to_vec(&message).unwrap().len() <= gpapi::service::transport::MAX_PLAINTEXT);
    let ServerMessage::Snapshot {
      mut snapshot,
      current_revision,
    } = message
    else {
      unreachable!()
    };
    assert_eq!(current_revision, 9);
    assert_eq!(snapshot.auth_executable, original_metadata);
    let configuration = snapshot.tunnel_state.as_ref().unwrap().configuration.as_ref().unwrap();
    assert!(configuration.included_routes.truncated);
    assert_eq!(configuration.included_routes.total_count, 15);
    snapshot.auth_executable = "a".repeat(gpapi::service::transport::MAX_PLAINTEXT);
    assert!(
      fit_snapshot_message(&mut ServerMessage::Snapshot {
        current_revision: 9,
        snapshot
      })
      .is_err()
    );
  }

  #[tokio::test]
  async fn resync_snapshot_is_ordered_with_state_events() {
    let registry = Arc::new(SessionRegistry::for_tests(Uuid::new_v4()));
    let credential = registry.issue(env!("CARGO_PKG_VERSION"), None).unwrap();
    let permit = registry
      .begin_handshake(
        credential.service_instance_id(),
        credential.session_id(),
        credential.product_version(),
      )
      .unwrap();
    let dispatcher = Arc::new(RequestDispatcher::new(
      crate::vpn_task::LifecycleHandle::for_tests(),
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      true,
      false,
    ));
    let (_state_tx, state_rx) = watch::channel(VpnState::Disconnected);
    let context = Arc::new(WsServerContext::new(
      env!("CARGO_PKG_VERSION"),
      registry,
      dispatcher,
      &state_rx,
    ));
    let (tx, mut rx) = mpsc::channel(8);
    let (control, _close_rx) = ConnectionControl::new(Uuid::new_v4(), tx);
    context.prepare_connection(&permit, control.clone()).await.unwrap();
    assert!(matches!(
      rx.recv().await,
      Some(ConnectionCommand::Send(ServerMessage::Attached { .. }))
    ));

    let (snapshot_result, ()) = tokio::join!(
      context.send_snapshot(&control),
      context.send_event(WsEvent::VpnState(VpnState::Disconnected)),
    );
    snapshot_result.unwrap();
    let first = rx.recv().await.unwrap();
    let second = rx.recv().await.unwrap();
    match (first, second) {
      (
        ConnectionCommand::Send(ServerMessage::Snapshot {
          current_revision: 0, ..
        }),
        ConnectionCommand::Send(ServerMessage::Event { revision: 1, .. }),
      )
      | (
        ConnectionCommand::Send(ServerMessage::Event { revision: 1, .. }),
        ConnectionCommand::Send(ServerMessage::Snapshot {
          current_revision: 1, ..
        }),
      ) => {}
      _ => panic!("snapshot and event were not atomically ordered"),
    }
  }

  #[tokio::test]
  async fn removed_prepared_candidate_cannot_commit_takeover() {
    let registry = Arc::new(SessionRegistry::for_tests(Uuid::new_v4()));
    let credential = registry.issue(env!("CARGO_PKG_VERSION"), None).unwrap();
    let dispatcher = Arc::new(RequestDispatcher::new(
      crate::vpn_task::LifecycleHandle::for_tests(),
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      true,
      false,
    ));
    let (_state_tx, state_rx) = watch::channel(VpnState::Disconnected);
    let context = Arc::new(WsServerContext::new(
      env!("CARGO_PKG_VERSION"),
      Arc::clone(&registry),
      dispatcher,
      &state_rx,
    ));

    let first = registry
      .begin_handshake(
        credential.service_instance_id(),
        credential.session_id(),
        credential.product_version(),
      )
      .unwrap();
    let (first_tx, mut first_rx) = mpsc::channel(4);
    let (first_control, _first_close) = ConnectionControl::new(Uuid::new_v4(), first_tx);
    context.prepare_connection(&first, first_control.clone()).await.unwrap();
    first_rx.recv().await.unwrap();
    context.commit_connection(&first, &first_control).await.unwrap();

    let reconnect = registry
      .begin_handshake(
        credential.service_instance_id(),
        credential.session_id(),
        credential.product_version(),
      )
      .unwrap();
    let (candidate_tx, _candidate_rx) = mpsc::channel(1);
    let (candidate, _candidate_close) = ConnectionControl::new(Uuid::new_v4(), candidate_tx);
    context.prepare_connection(&reconnect, candidate.clone()).await.unwrap();
    context.send_event(WsEvent::VpnState(VpnState::Disconnected)).await;

    assert!(context.commit_connection(&reconnect, &candidate).await.is_err());
    assert!(registry.is_current(credential.session_id(), 1));
  }
}

async fn watch_vpn_state(mut vpn_state_rx: watch::Receiver<VpnState>, ctx: Arc<WsServerContext>) {
  while vpn_state_rx.changed().await.is_ok() {
    let vpn_state = vpn_state_rx.borrow().clone();
    ctx.send_event(WsEvent::VpnState(vpn_state)).await;
  }
}

async fn watch_tunnel(mut binding_rx: watch::Receiver<crate::vpn_task::TunnelBinding>, ctx: Arc<WsServerContext>) {
  loop {
    let binding = binding_rx.borrow_and_update().clone();
    let mut updates = binding.monitor.as_ref().map(|monitor| monitor.subscribe());
    let attempt = binding.attempt;
    ctx.bind_tunnel(binding).await;
    loop {
      tokio::select! {
        biased;
        changed = binding_rx.changed() => {
          if changed.is_err() { return; }
          break;
        }
        changed = async {
          match &mut updates {
            Some(receiver) => receiver.changed().await,
            None => std::future::pending().await,
          }
        } => {
          if changed.is_err() { break; }
          // Fence an update against a new lifecycle binding already published.
          if binding_rx.has_changed().unwrap_or(true) { break; }
          ctx.publish_tunnel(attempt).await;
        }
      }
    }
  }
}
