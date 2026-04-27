use std::{
  sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
  },
  thread,
};

use gpapi::{
  gateway, logger,
  service::{
    request::{ConnectRequest, UpdateLogLevelRequest, WsRequest},
    session::SessionInfo,
    vpn_state::VpnState,
  },
};
use log::{info, warn};
use openconnect::Vpn;
use tokio::sync::{RwLock, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

pub(crate) struct VpnTaskContext {
  vpn_handle: Arc<RwLock<Option<Vpn>>>,
  session_generation: Arc<AtomicU64>,
  vpn_state_tx: Arc<watch::Sender<VpnState>>,
  session_info_tx: Arc<watch::Sender<Option<SessionInfo>>>,
  disconnect_rx: RwLock<Option<oneshot::Receiver<()>>>,
}

impl VpnTaskContext {
  pub fn new(vpn_state_tx: watch::Sender<VpnState>, session_info_tx: watch::Sender<Option<SessionInfo>>) -> Self {
    Self {
      vpn_handle: Default::default(),
      session_generation: Arc::new(AtomicU64::new(0)),
      vpn_state_tx: Arc::new(vpn_state_tx),
      session_info_tx: Arc::new(session_info_tx),
      disconnect_rx: Default::default(),
    }
  }

  pub async fn connect(&self, req: ConnectRequest) {
    let vpn_state = self.vpn_state_tx.borrow().clone();
    if !matches!(vpn_state, VpnState::Disconnected) {
      info!("VPN is not disconnected, ignore the request");
      return;
    }

    let vpn_state_tx = self.vpn_state_tx.clone();
    let session_info_tx = self.session_info_tx.clone();
    let session_generation = Arc::clone(&self.session_generation);
    let info = req.info().clone();
    let generation = self.next_session_generation();
    let vpn_handle = Arc::clone(&self.vpn_handle);
    let args = req.args();
    let vpn = match Vpn::builder(req.gateway().server(), args.cookie())
      .script(args.vpnc_script())
      .user_agent(args.user_agent())
      .os(args.openconnect_os())
      .os_version(args.os_version())
      .client_version(args.client_version())
      .certificate(args.certificate())
      .sslkey(args.sslkey())
      .key_password(args.key_password())
      .hip(args.hip())
      .csd_uid(args.csd_uid())
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
      Ok(vpn) => vpn,
      Err(err) => {
        warn!("Failed to create VPN: {}", err);
        self.clear_session_info_state();
        vpn_state_tx.send(VpnState::Disconnected).ok();
        return;
      }
    };

    // Save the VPN handle
    vpn_handle.write().await.replace(vpn);
    let connect_info = Box::new(info.clone());
    vpn_state_tx.send(VpnState::Connecting(connect_info)).ok();

    let (disconnect_tx, disconnect_rx) = oneshot::channel::<()>();
    self.disconnect_rx.write().await.replace(disconnect_rx);
    let thread_session_generation = Arc::clone(&session_generation);

    // Spawn a new thread to process the VPN connection, cannot use tokio::spawn here.
    // Otherwise, it will block the tokio runtime and cannot send the VPN state to the channel
    thread::spawn(move || {
      let vpn_state_tx_clone = vpn_state_tx.clone();

      vpn_handle.blocking_read().as_ref().map(|vpn| {
        vpn.connect(move || {
          let connect_info = Box::new(info.clone());
          vpn_state_tx.send(VpnState::Connected(connect_info)).ok();
        })
      });

      // Notify the VPN is disconnected
      vpn_state_tx_clone.send(VpnState::Disconnected).ok();
      thread_session_generation.fetch_add(1, Ordering::SeqCst);
      session_info_tx.send(None).ok();
      // Remove the VPN handle
      vpn_handle.blocking_write().take();

      disconnect_tx.send(()).ok();
    });

    let session_info_tx = Arc::clone(&self.session_info_tx);
    tokio::spawn(async move {
      match gateway::retrieve_session_info(&req).await {
        Ok(session_info) => {
          publish_session_info_if_current(&session_generation, &session_info_tx, generation, Some(session_info));
        }
        Err(err) => {
          warn!("Failed to retrieve session info: {}", err);
          publish_session_info_if_current(&session_generation, &session_info_tx, generation, None);
        }
      }
    });
  }

  pub async fn disconnect(&self) -> bool {
    if let Some(disconnect_rx) = self.disconnect_rx.write().await.take() {
      info!("Disconnecting VPN...");
      self.clear_session_info_state();
      if let Some(vpn) = self.vpn_handle.read().await.as_ref() {
        info!("VPN is connected, start disconnecting...");
        self.vpn_state_tx.send(VpnState::Disconnecting).ok();
        vpn.disconnect()
      }
      // Wait for the VPN to be disconnected
      disconnect_rx.await.ok();
      info!("VPN disconnected");

      true
    } else {
      info!("VPN is not connected, skip disconnect");
      self.vpn_state_tx.send(VpnState::Disconnected).ok();
      self.clear_session_info_state();
      false
    }
  }

  fn next_session_generation(&self) -> u64 {
    self.session_generation.fetch_add(1, Ordering::SeqCst) + 1
  }

  fn clear_session_info_state(&self) {
    self.next_session_generation();
    self.session_info_tx.send(None).ok();
  }
}

fn publish_session_info_if_current(
  session_generation: &AtomicU64,
  session_info_tx: &watch::Sender<Option<SessionInfo>>,
  generation: u64,
  session_info: Option<SessionInfo>,
) {
  if session_generation.load(Ordering::SeqCst) == generation {
    session_info_tx.send(session_info).ok();
  }
}

pub(crate) struct VpnTask {
  ws_req_rx: mpsc::Receiver<WsRequest>,
  ctx: Arc<VpnTaskContext>,
  cancel_token: CancellationToken,
}

impl VpnTask {
  pub fn new(
    ws_req_rx: mpsc::Receiver<WsRequest>,
    vpn_state_tx: watch::Sender<VpnState>,
    session_info_tx: watch::Sender<Option<SessionInfo>>,
  ) -> Self {
    let ctx = Arc::new(VpnTaskContext::new(vpn_state_tx, session_info_tx));
    let cancel_token = CancellationToken::new();

    Self {
      ws_req_rx,
      ctx,
      cancel_token,
    }
  }

  pub fn cancel_token(&self) -> CancellationToken {
    self.cancel_token.clone()
  }

  pub async fn start(&mut self, server_cancel_token: CancellationToken) {
    let cancel_token = self.cancel_token.clone();

    tokio::select! {
        _ = self.recv() => {
            info!("VPN task stopped");
        }
        _ = cancel_token.cancelled() => {
            info!("VPN task cancelled");
            self.ctx.disconnect().await;
        }
    }

    server_cancel_token.cancel();
  }

  pub fn context(&self) -> Arc<VpnTaskContext> {
    return Arc::clone(&self.ctx);
  }

  async fn recv(&mut self) {
    while let Some(req) = self.ws_req_rx.recv().await {
      tokio::spawn(process_ws_req(req, self.ctx.clone()));
    }
  }
}

async fn process_ws_req(req: WsRequest, ctx: Arc<VpnTaskContext>) {
  match req {
    WsRequest::Connect(req) => {
      ctx.connect(*req).await;
    }
    WsRequest::Disconnect(_) => {
      ctx.disconnect().await;
    }
    WsRequest::UpdateLogLevel(UpdateLogLevelRequest(level)) => {
      let level = level.parse().unwrap_or_else(|_| log::Level::Info);
      info!("Updating log level to: {}", level);
      if let Err(err) = logger::set_max_level(level) {
        warn!("Failed to update log level: {}", err);
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn publish_session_info_ignores_stale_generation() {
    let (vpn_state_tx, _) = watch::channel(VpnState::Disconnected);
    let (session_info_tx, session_info_rx) = watch::channel(None);
    let ctx = VpnTaskContext::new(vpn_state_tx, session_info_tx);
    let generation = ctx.next_session_generation();
    ctx.clear_session_info_state();

    publish_session_info_if_current(
      &ctx.session_generation,
      &ctx.session_info_tx,
      generation,
      Some(SessionInfo::default()),
    );

    assert_eq!(*session_info_rx.borrow(), None);
  }

  #[test]
  fn publish_session_info_accepts_current_generation() {
    let (vpn_state_tx, _) = watch::channel(VpnState::Disconnected);
    let (session_info_tx, session_info_rx) = watch::channel(None);
    let ctx = VpnTaskContext::new(vpn_state_tx, session_info_tx);
    let generation = ctx.next_session_generation();
    let session_info = SessionInfo {
      allow_extend_session: true,
      ..Default::default()
    };

    publish_session_info_if_current(
      &ctx.session_generation,
      &ctx.session_info_tx,
      generation,
      Some(session_info.clone()),
    );

    assert_eq!(*session_info_rx.borrow(), Some(session_info));
  }
}
