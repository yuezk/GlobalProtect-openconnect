use std::sync::Arc;

use axum::extract::ws::Message;
use common::constants::GP_AUTH_BINARY;
use gpapi::{
  service::{event::WsEvent, request::WsRequest, vpn_env::VpnEnv, vpn_state::VpnState},
  utils::{crypto::Crypto, lock_file::LockFile, redact::Redaction},
};
use log::{info, warn};
use openconnect::{find_csd_wrapper, find_vpnc_script};
use serde::de::DeserializeOwned;
use tokio::{
  net::TcpListener,
  sync::{mpsc, watch, RwLock},
};
use tokio_util::sync::CancellationToken;

use crate::{routes, ws_connection::WsConnection};

pub(crate) struct WsServerContext {
  crypto: Arc<Crypto>,
  ws_req_tx: mpsc::Sender<WsRequest>,
  vpn_state_rx: watch::Receiver<VpnState>,
  redaction: Arc<Redaction>,
  connections: RwLock<Vec<Arc<WsConnection>>>,
}

impl WsServerContext {
  pub fn new(
    api_key: Vec<u8>,
    ws_req_tx: mpsc::Sender<WsRequest>,
    vpn_state_rx: watch::Receiver<VpnState>,
    redaction: Arc<Redaction>,
  ) -> Self {
    Self {
      crypto: Arc::new(Crypto::new(api_key)),
      ws_req_tx,
      vpn_state_rx,
      redaction,
      connections: Default::default(),
    }
  }

  pub fn decrypt<T: DeserializeOwned>(&self, encrypted: Vec<u8>) -> anyhow::Result<T> {
    self.crypto.decrypt(encrypted)
  }

  pub async fn send_event(&self, event: WsEvent) {
    let connections = self.connections.read().await;

    for conn in connections.iter() {
      let _ = conn.send_event(&event).await;
    }
  }

  pub async fn add_connection(&self) -> (Arc<WsConnection>, mpsc::Receiver<Message>) {
    let (tx, rx) = mpsc::channel::<Message>(32);
    let conn = Arc::new(WsConnection::new(Arc::clone(&self.crypto), tx));

    // Send current VPN state to new client
    info!("Sending current environment to new client");
    let vpn_env = VpnEnv {
      vpn_state: self.vpn_state_rx.borrow().clone(),
      vpnc_script: find_vpnc_script().map(|s| s.to_owned()),
      csd_wrapper: find_csd_wrapper().map(|s| s.to_owned()),
      auth_executable: GP_AUTH_BINARY.to_owned(),
    };

    if let Err(err) = conn.send_event(&WsEvent::VpnEnv(vpn_env)).await {
      warn!("Failed to send VPN state to new client: {}", err);
    }

    self.connections.write().await.push(Arc::clone(&conn));

    (conn, rx)
  }

  pub async fn remove_connection(&self, conn: Arc<WsConnection>) {
    let mut connections = self.connections.write().await;
    connections.retain(|c| !Arc::ptr_eq(c, &conn));
  }

  fn vpn_state_rx(&self) -> watch::Receiver<VpnState> {
    self.vpn_state_rx.clone()
  }

  pub async fn forward_req(&self, req: WsRequest) -> anyhow::Result<()> {
    if let WsRequest::Connect(ref req) = req {
      self
        .redaction
        .add_values(&[req.gateway().server(), req.args().cookie()])?
    }

    self.ws_req_tx.send(req).await?;

    Ok(())
  }
}

pub(crate) struct WsServer {
  ctx: Arc<WsServerContext>,
  cancel_token: CancellationToken,
  lock_file: Arc<LockFile>,
}

impl WsServer {
  pub fn new(
    api_key: Vec<u8>,
    ws_req_tx: mpsc::Sender<WsRequest>,
    vpn_state_rx: watch::Receiver<VpnState>,
    lock_file: Arc<LockFile>,
    redaction: Arc<Redaction>,
  ) -> Self {
    let ctx = Arc::new(WsServerContext::new(api_key, ws_req_tx, vpn_state_rx, redaction));
    let cancel_token = CancellationToken::new();

    Self {
      ctx,
      cancel_token,
      lock_file,
    }
  }

  pub fn context(&self) -> Arc<WsServerContext> {
    Arc::clone(&self.ctx)
  }

  pub fn cancel_token(&self) -> CancellationToken {
    self.cancel_token.clone()
  }

  pub async fn start(&self, shutdown_tx: mpsc::Sender<()>) {
    let listener = match self.start_tcp_server().await {
      Ok(listener) => listener,
      Err(err) => {
        warn!("Failed to start WS server: {}", err);
        let _ = shutdown_tx.send(()).await;
        return;
      }
    };

    tokio::select! {
      _ = watch_vpn_state(self.ctx.vpn_state_rx(), Arc::clone(&self.ctx)) => {
        info!("VPN state watch task completed");
      }
      _ = start_server(listener, self.ctx.clone()) => {
          info!("WS server stopped");
      }
      _ = self.cancel_token.cancelled() => {
        info!("WS server cancelled");
      }
    }

    let _ = shutdown_tx.send(()).await;
  }

  async fn start_tcp_server(&self) -> anyhow::Result<TcpListener> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let local_addr = listener.local_addr()?;
    let port = local_addr.port();

    info!("WS server listening on port: {}", port);

    self.lock_file.lock(&port.to_string())?;

    Ok(listener)
  }
}

async fn watch_vpn_state(mut vpn_state_rx: watch::Receiver<VpnState>, ctx: Arc<WsServerContext>) {
  while vpn_state_rx.changed().await.is_ok() {
    let vpn_state = vpn_state_rx.borrow().clone();
    ctx.send_event(WsEvent::VpnState(vpn_state)).await;
  }
}

async fn start_server(listener: TcpListener, ctx: Arc<WsServerContext>) -> anyhow::Result<()> {
  let routes = routes::routes(ctx);

  axum::serve(listener, routes).await?;

  Ok(())
}
