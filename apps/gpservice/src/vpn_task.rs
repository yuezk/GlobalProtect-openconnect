use std::{sync::Arc, thread};

use gpapi::service::{
  request::{ConnectRequest, WsRequest},
  vpn_state::VpnState,
};
use log::info;
use openconnect::Vpn;
use tokio::sync::{mpsc, oneshot, watch, RwLock};
use tokio_util::sync::CancellationToken;

pub(crate) struct VpnTaskContext {
  vpn_handle: Arc<RwLock<Option<Vpn>>>,
  vpn_state_tx: Arc<watch::Sender<VpnState>>,
  disconnect_rx: RwLock<Option<oneshot::Receiver<()>>>,
}

impl VpnTaskContext {
  pub fn new(vpn_state_tx: watch::Sender<VpnState>) -> Self {
    Self {
      vpn_handle: Default::default(),
      vpn_state_tx: Arc::new(vpn_state_tx),
      disconnect_rx: Default::default(),
    }
  }

  pub async fn connect(&self, req: ConnectRequest) {
    let vpn_state = self.vpn_state_tx.borrow().clone();
    if !matches!(vpn_state, VpnState::Disconnected) {
      info!("VPN is not disconnected, ignore the request");
      return;
    }

    let info = req.info().clone();
    let vpn_handle = self.vpn_handle.clone();
    let args = req.args();
    let vpn = Vpn::builder(req.gateway().server(), args.cookie())
      .user_agent(args.user_agent())
      .script(args.vpnc_script())
      .os(args.openconnect_os())
      .build();

    // Save the VPN handle
    vpn_handle.write().await.replace(vpn);

    let vpn_state_tx = self.vpn_state_tx.clone();
    let connect_info = Box::new(info.clone());
    vpn_state_tx.send(VpnState::Connecting(connect_info)).ok();

    let (disconnect_tx, disconnect_rx) = oneshot::channel::<()>();
    self.disconnect_rx.write().await.replace(disconnect_rx);

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
      // Remove the VPN handle
      vpn_handle.blocking_write().take();

      disconnect_tx.send(()).ok();
    });
  }

  pub async fn disconnect(&self) {
    if let Some(disconnect_rx) = self.disconnect_rx.write().await.take() {
      if let Some(vpn) = self.vpn_handle.read().await.as_ref() {
        self.vpn_state_tx.send(VpnState::Disconnecting).ok();
        vpn.disconnect()
      }
      // Wait for the VPN to be disconnected
      disconnect_rx.await.ok();
      info!("VPN disconnected");
    } else {
      info!("VPN is not connected, skip disconnect");
      self.vpn_state_tx.send(VpnState::Disconnected).ok();
    }
  }
}

pub(crate) struct VpnTask {
  ws_req_rx: mpsc::Receiver<WsRequest>,
  ctx: Arc<VpnTaskContext>,
  cancel_token: CancellationToken,
}

impl VpnTask {
  pub fn new(ws_req_rx: mpsc::Receiver<WsRequest>, vpn_state_tx: watch::Sender<VpnState>) -> Self {
    let ctx = Arc::new(VpnTaskContext::new(vpn_state_tx));
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
  }
}
