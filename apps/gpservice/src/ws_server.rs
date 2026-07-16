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
  utils::lock_file::LockFile,
};
use log::{info, warn};
use openconnect::{find_csd_wrapper, find_vpnc_script};
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
}

pub(crate) struct WsServerContext {
  product_version: &'static str,
  registry: Arc<SessionRegistry>,
  dispatcher: Arc<RequestDispatcher>,
  state: Mutex<ServiceState>,
  handshake_slots: Arc<Semaphore>,
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
      }),
      handshake_slots: Arc::new(Semaphore::new(16)),
    }
  }

  pub fn product_version(&self) -> &'static str {
    self.product_version
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

    control
      .send(ConnectionCommand::Send(ServerMessage::Attached {
        connection_id: control.connection_id(),
        current_revision: state.revision,
        snapshot: state.snapshot.clone(),
      }))
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
        if let Some(replaced) = &replaced {
          state.connections.remove(&replaced.connection_id());
        }
        Ok(replaced)
      }
      Err(err) => {
        state.connections.remove(&connection.connection_id());
        Err(err)
      }
    }
  }

  pub async fn abandon_connection(&self, connection_id: Uuid) {
    self.state.lock().await.connections.remove(&connection_id);
  }

  pub async fn remove_closed_connections(&self) {
    self
      .state
      .lock()
      .await
      .connections
      .retain(|_, connection| !connection.is_closed());
  }

  pub async fn send_snapshot(&self, control: &ConnectionControl) -> Result<(), ()> {
    let state = self.state.lock().await;
    control.send(ConnectionCommand::Send(ServerMessage::Snapshot {
      current_revision: state.revision,
      snapshot: state.snapshot.clone(),
    }))
  }

  pub async fn remove_connection(&self, connection_id: Uuid, session_id: Uuid, generation: u64) {
    self.state.lock().await.connections.remove(&connection_id);
    self.registry.disconnect_if_current(session_id, generation);
  }

  pub async fn send_event(&self, event: WsEvent) {
    let mut state = self.state.lock().await;
    if let WsEvent::VpnEnv(env) = &event {
      state.snapshot = env.clone();
    } else if let WsEvent::VpnState(vpn_state) = &event {
      state.snapshot.vpn_state = vpn_state.clone();
    }

    let Some(revision) = state.revision.checked_add(1) else {
      for connection in state.connections.values() {
        connection.close(CloseReason::InternalError);
      }
      state.connections.clear();
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
      result = axum::serve(listener, routes::routes(Arc::clone(&self.ctx))) => {
        if let Err(err) = result {
          warn!("WS server stopped: {err}");
        }
      }
      _ = self.cancel_token.cancelled() => {
        info!("WS server cancelled");
      }
    }
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
    vpn_state,
    vpnc_script: find_vpnc_script().map(ToOwned::to_owned),
    csd_wrapper: find_csd_wrapper().map(ToOwned::to_owned),
    auth_executable: binary_paths::gpauth().to_string_lossy().into_owned(),
    host_info: HostInfo {
      host_identity: HostIdentity::collect(),
    },
  }
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

  #[tokio::test]
  async fn resync_snapshot_is_ordered_with_state_events() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue(env!("CARGO_PKG_VERSION")).unwrap();
    let permit = registry
      .begin_handshake(
        credential.service_instance_id(),
        credential.session_id(),
        credential.product_version(),
      )
      .unwrap();
    let (request_tx, _request_rx) = mpsc::channel(1);
    let dispatcher = Arc::new(RequestDispatcher::new(
      request_tx,
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
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
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue(env!("CARGO_PKG_VERSION")).unwrap();
    let (request_tx, _request_rx) = mpsc::channel(1);
    let dispatcher = Arc::new(RequestDispatcher::new(
      request_tx,
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
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
