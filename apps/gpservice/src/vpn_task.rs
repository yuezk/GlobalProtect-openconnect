use std::{io::Write, os::unix::fs::PermissionsExt, sync::Arc, thread};

use gpapi::{
  logger,
  service::{
    request::{ConnectRequest, MAX_CLIENT_IDENTITY_DATA, UpdateLogLevelRequest, WsRequest},
    vpn_state::{ConnectedInfo, VpnState},
  },
  session::{SessionInfo, SessionWarning},
};
use log::{info, warn};
use openconnect::Vpn;
use tokio::sync::{RwLock, mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

pub(crate) struct VpnTaskContext {
  vpn_handle: Arc<RwLock<Option<Vpn>>>,
  vpn_state_tx: Arc<watch::Sender<VpnState>>,
  disconnect_rx: RwLock<Option<oneshot::Receiver<()>>>,
  brokered_macos: bool,
  trusted_csd_uid: Option<u32>,
  identity_files: Arc<RwLock<Vec<tempfile::NamedTempFile>>>,
}

impl VpnTaskContext {
  pub fn new(vpn_state_tx: watch::Sender<VpnState>, brokered_macos: bool, trusted_csd_uid: Option<u32>) -> Self {
    Self {
      vpn_handle: Default::default(),
      vpn_state_tx: Arc::new(vpn_state_tx),
      disconnect_rx: Default::default(),
      brokered_macos,
      trusted_csd_uid,
      identity_files: Default::default(),
    }
  }

  pub async fn connect(&self, req: ConnectRequest) {
    let vpn_state = self.vpn_state_tx.borrow().clone();
    if !matches!(vpn_state, VpnState::Disconnected) {
      info!("VPN is not disconnected, ignore the request");
      return;
    }

    let vpn_state_tx = self.vpn_state_tx.clone();
    let info = req.info().clone();
    let vpn_handle = Arc::clone(&self.vpn_handle);
    let args = req.args();
    let identity = match prepare_identity(args, self.brokered_macos) {
      Ok(identity) => identity,
      Err(err) => {
        warn!("Failed to prepare client identity: {err}");
        vpn_state_tx.send(VpnState::Disconnected).ok();
        return;
      }
    };
    let allow_extend_session = args.allow_extend_session();
    let vpn_builder = match crate::vpn_script::builder(req.gateway().server(), args.cookie(), args) {
      Ok(builder) => builder,
      Err(err) => {
        warn!("Failed to select the VPNC script: {err}");
        vpn_state_tx.send(VpnState::Disconnected).ok();
        return;
      }
    };
    let csd_uid = match resolve_csd_uid(self.trusted_csd_uid, args.csd_uid(), args.hip()) {
      Ok(uid) => uid,
      Err(err) => {
        warn!("Failed to select the HIP script user: {err}");
        vpn_state_tx.send(VpnState::Disconnected).ok();
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
      Ok(vpn) => vpn,
      Err(err) => {
        warn!("Failed to create VPN: {}", err);
        vpn_state_tx.send(VpnState::Disconnected).ok();
        return;
      }
    };

    // Save the VPN handle
    vpn_handle.write().await.replace(vpn);
    *self.identity_files.write().await = identity.files;
    let connect_info = Box::new(info.clone());
    vpn_state_tx.send(VpnState::Connecting(connect_info)).ok();

    let (disconnect_tx, disconnect_rx) = oneshot::channel::<()>();
    self.disconnect_rx.write().await.replace(disconnect_rx);
    let identity_files = Arc::clone(&self.identity_files);

    // Spawn a new thread to process the VPN connection, cannot use tokio::spawn here.
    // Otherwise, it will block the tokio runtime and cannot send the VPN state to the channel
    thread::spawn(move || {
      let vpn_state_tx_clone = vpn_state_tx.clone();

      vpn_handle.blocking_read().as_ref().map(|vpn| {
        vpn.connect(move |vpn_session_info| {
          let session_info = SessionInfo::from_vpn_session_fields(
            vpn_session_info.lifetime_secs,
            vpn_session_info.user_expires,
            vpn_session_info.lifetime_warning.map(|warning| SessionWarning {
              prior_secs: warning.prior_secs,
              message: warning.message,
            }),
            allow_extend_session,
          );
          info!("VPN session info: {}", session_info.log_summary());
          let connected_info = Box::new(ConnectedInfo::new(info.clone(), Some(session_info)));
          vpn_state_tx.send(VpnState::Connected(connected_info)).ok();
        })
      });

      // Notify the VPN is disconnected
      vpn_state_tx_clone.send(VpnState::Disconnected).ok();
      // Remove the VPN handle
      vpn_handle.blocking_write().take();
      identity_files.blocking_write().clear();

      disconnect_tx.send(()).ok();
    });
  }

  pub async fn disconnect(&self) -> bool {
    if let Some(disconnect_rx) = self.disconnect_rx.write().await.take() {
      info!("Disconnecting VPN...");
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
      false
    }
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
    brokered_macos: bool,
    trusted_csd_uid: Option<u32>,
  ) -> Self {
    let ctx = Arc::new(VpnTaskContext::new(vpn_state_tx, brokered_macos, trusted_csd_uid));
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
    Arc::clone(&self.ctx)
  }

  async fn recv(&mut self) {
    while let Some(req) = self.ws_req_rx.recv().await {
      tokio::spawn(process_ws_req(req, self.ctx.clone()));
    }
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

async fn process_ws_req(req: WsRequest, ctx: Arc<VpnTaskContext>) {
  match req {
    WsRequest::Connect(req) => {
      ctx.connect(*req).await;
    }
    WsRequest::Disconnect(_) => {
      ctx.disconnect().await;
    }
    WsRequest::UpdateLogLevel(UpdateLogLevelRequest(level)) => {
      let level = level.parse().unwrap_or(log::Level::Info);
      info!("Updating log level to: {}", level);
      if let Err(err) = logger::set_max_level(level) {
        warn!("Failed to update log level: {}", err);
      }
    }
    WsRequest::RestartGui | WsRequest::UpdateGui(_) => {
      warn!("Non-VPN request reached the VPN task");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

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
