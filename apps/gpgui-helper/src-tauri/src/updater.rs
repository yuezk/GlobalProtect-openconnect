use std::{
  error::Error,
  fmt::{self, Display, Formatter},
  sync::{Arc, RwLock},
  time::Duration,
};

use anyhow::{Context, bail};
use futures_util::{SinkExt, StreamExt};
use gpapi::{
  service::{
    request::{UpdateGuiRequest, WsRequest},
    transport::{
      ClientMessage, ClientPrelude, MAX_WS_MESSAGE, NoiseInitiator, NoiseTransport, ServerMessage, ServerPrelude,
      ServiceResult, SessionCredential,
    },
  },
  utils::{checksum::verify_checksum, endpoint::ws_endpoint},
};
use log::{info, warn};
use tauri::{Emitter, WebviewWindow};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::{
  MaybeTlsStream, WebSocketStream, connect_async_with_config,
  tungstenite::{Message, protocol::WebSocketConfig},
};
use uuid::Uuid;

use crate::downloader::{ChecksumFetcher, FileDownloader};

#[cfg(not(debug_assertions))]
const SNAPSHOT: &str = match option_env!("SNAPSHOT") {
  Some(val) => val,
  None => "false",
};

#[cfg(target_env = "musl")]
const GUI_LIBC_SUFFIX: &str = "-musl";

#[cfg(not(target_env = "musl"))]
const GUI_LIBC_SUFFIX: &str = "";

pub struct ProgressNotifier {
  win: WebviewWindow,
}

impl ProgressNotifier {
  pub fn new(win: WebviewWindow) -> Self {
    Self { win }
  }

  fn notify(&self, progress: Option<f64>) {
    let _ = self.win.emit("app://update-progress", progress);
  }

  fn notify_error(&self) {
    let _ = self.win.emit("app://update-error", ());
  }

  fn notify_done(&self) {
    let _ = self.win.emit("app://update-done", ());
  }
}

pub struct Installer {
  command_tx: mpsc::Sender<InstallCommand>,
}

struct InstallCommand {
  request: UpdateGuiRequest,
  reply: oneshot::Sender<anyhow::Result<()>>,
}

impl Installer {
  pub fn new(credential: Arc<SessionCredential>, credential_lease: watch::Receiver<bool>) -> Self {
    let (command_tx, command_rx) = mpsc::channel(1);
    tokio::spawn(async move {
      run_installer(credential, credential_lease, command_rx).await;
    });
    Self { command_tx }
  }

  async fn install(&self, path: &str, checksum: &str) -> anyhow::Result<()> {
    let request = UpdateGuiRequest {
      path: path.to_string(),
      checksum: checksum.to_string(),
    };
    let (reply, result) = oneshot::channel();
    self
      .command_tx
      .send(InstallCommand { request, reply })
      .await
      .context("Authenticated GUI installer is unavailable")?;
    tokio::time::timeout(Duration::from_secs(60), result)
      .await
      .context("Service update request timed out")?
      .context("Authenticated GUI installer stopped before replying")?
  }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
struct TerminalConnectError(String);

impl Display for TerminalConnectError {
  fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl Error for TerminalConnectError {}

async fn run_installer(
  credential: Arc<SessionCredential>,
  mut credential_lease: watch::Receiver<bool>,
  mut commands: mpsc::Receiver<InstallCommand>,
) {
  let mut pending = None;

  loop {
    if !*credential_lease.borrow() {
      return;
    }

    let connection = tokio::select! {
      changed = credential_lease.changed() => {
        if changed.is_err() || !*credential_lease.borrow() {
          return;
        }
        continue;
      }
      result = tokio::time::timeout(Duration::from_secs(5), connect_installer(&credential)) => {
        match result {
          Ok(result) => result,
          Err(_) => Err(anyhow::anyhow!("Authenticated GUI installer handshake timed out")),
        }
      }
    };

    match connection {
      Ok((mut socket, mut noise)) => {
        info!("Authenticated GUI installer connected");
        match run_installer_connection(
          &mut socket,
          &mut noise,
          &mut credential_lease,
          &mut commands,
          &mut pending,
        )
        .await
        {
          Ok(ConnectionEnd::CredentialLeaseEnded | ConnectionEnd::CommandsClosed) => return,
          Err(err) => warn!("Authenticated GUI installer connection lost: {err}"),
        }
      }
      Err(err) if err.is::<TerminalConnectError>() => {
        warn!("Authenticated GUI installer stopped: {err}");
        return;
      }
      Err(err) => warn!("Authenticated GUI installer connection failed: {err}"),
    }

    tokio::select! {
      changed = credential_lease.changed() => {
        if changed.is_err() || !*credential_lease.borrow() {
          return;
        }
      }
      _ = tokio::time::sleep(Duration::from_millis(100)) => {}
    }
  }
}

enum ConnectionEnd {
  CredentialLeaseEnded,
  CommandsClosed,
}

async fn run_installer_connection(
  socket: &mut Socket,
  noise: &mut NoiseTransport,
  credential_lease: &mut watch::Receiver<bool>,
  commands: &mut mpsc::Receiver<InstallCommand>,
  pending: &mut Option<InstallCommand>,
) -> anyhow::Result<ConnectionEnd> {
  if pending.as_ref().is_some_and(|command| command.reply.is_closed()) {
    pending.take();
  }
  let mut pending_id = match pending.as_ref() {
    Some(command) => Some(send_install_command(socket, noise, command).await?),
    None => None,
  };

  loop {
    tokio::select! {
      changed = credential_lease.changed() => {
        if changed.is_err() || !*credential_lease.borrow() {
          return Ok(ConnectionEnd::CredentialLeaseEnded);
        }
      }
      command = commands.recv() => {
        let Some(command) = command else { return Ok(ConnectionEnd::CommandsClosed) };
        if command.reply.is_closed() {
          continue;
        }
        if pending.is_some() {
          let _ = command.reply.send(Err(anyhow::anyhow!("GUI installation is already pending")));
          continue;
        }
        *pending = Some(command);
        pending_id = Some(send_install_command(socket, noise, pending.as_ref().unwrap()).await?);
      }
      frame = socket.next() => {
        match frame.context("Service closed the authenticated GUI installer")?? {
          Message::Binary(data) => match noise.decrypt::<ServerMessage>(&data)? {
            ServerMessage::Reply { id, result } => {
              if pending_id != Some(id) {
                bail!("Service sent a mismatched GUI installation reply");
              }
              let Some(command) = pending.take() else {
                bail!("Service sent an unexpected GUI installation reply");
              };
              pending_id = None;
              let result = match result {
                ServiceResult::Accepted => Ok(()),
                ServiceResult::Rejected(rejection) => Err(anyhow::anyhow!(
                  "Service rejected GUI installation ({:?}): {}",
                  rejection.code(),
                  rejection.message()
                )),
              };
              let _ = command.reply.send(result);
            }
            ServerMessage::Event { .. } | ServerMessage::Snapshot { .. } => {}
            ServerMessage::Attached { .. } => bail!("Service sent a duplicate attach response"),
          },
          Message::Ping(data) => socket.send(Message::Pong(data)).await?,
          Message::Pong(_) => {}
          Message::Close(_) => bail!("Service closed the authenticated GUI installer"),
          _ => bail!("Service sent an unexpected WebSocket frame"),
        }
      }
    }
  }
}

async fn send_install_command(
  socket: &mut Socket,
  noise: &mut NoiseTransport,
  command: &InstallCommand,
) -> anyhow::Result<Uuid> {
  let id = Uuid::new_v4();
  send_client_message(
    socket,
    noise,
    &ClientMessage::Request {
      id,
      request: WsRequest::UpdateGui(command.request.clone()),
    },
  )
  .await?;
  Ok(id)
}

async fn connect_installer(credential: &SessionCredential) -> anyhow::Result<(Socket, NoiseTransport)> {
  let endpoint = ws_endpoint().await?;
  connect_installer_at(credential, &endpoint).await
}

async fn connect_installer_at(
  credential: &SessionCredential,
  endpoint: &str,
) -> anyhow::Result<(Socket, NoiseTransport)> {
  if credential.product_version() != env!("CARGO_PKG_VERSION") {
    return Err(TerminalConnectError("Service credential version does not match gpgui-helper".to_owned()).into());
  }
  let config = WebSocketConfig::default()
    .max_message_size(Some(MAX_WS_MESSAGE))
    .max_frame_size(Some(MAX_WS_MESSAGE));
  let (mut socket, _) = connect_async_with_config(endpoint, Some(config), false).await?;
  let prelude = ClientPrelude {
    product_version: credential.product_version().to_owned(),
    service_instance_id: credential.service_instance_id(),
    session_id: credential.session_id(),
  };
  socket.send(Message::Binary(prelude.encode()?.into())).await?;
  let server_prelude = ServerPrelude::decode(&receive_binary(&mut socket).await?)
    .map_err(|err| TerminalConnectError(format!("Service sent an invalid update-session response: {err}")))?;
  match server_prelude {
    ServerPrelude::Ready => {}
    ServerPrelude::Rejected(reason) => {
      return Err(TerminalConnectError(format!("Service rejected the update session: {reason:?}")).into());
    }
  }

  let mut initiator = NoiseInitiator::new(
    credential.product_version(),
    credential.service_instance_id(),
    credential.session_id(),
    credential.secret(),
  )?;
  socket.send(Message::Binary(initiator.write_first()?.into())).await?;
  let mut noise = initiator.read_response(&receive_binary(&mut socket).await?)?;
  send_client_message(
    &mut socket,
    &mut noise,
    &ClientMessage::Attach {
      last_event_revision: None,
    },
  )
  .await?;
  if !matches!(
    receive_server_message(&mut socket, &mut noise).await?,
    ServerMessage::Attached { .. }
  ) {
    bail!("Service did not attach the update session");
  }

  Ok((socket, noise))
}

async fn send_client_message(
  socket: &mut Socket,
  noise: &mut NoiseTransport,
  message: &ClientMessage,
) -> anyhow::Result<()> {
  socket.send(Message::Binary(noise.encrypt(message)?.into())).await?;
  Ok(())
}

async fn receive_server_message(socket: &mut Socket, noise: &mut NoiseTransport) -> anyhow::Result<ServerMessage> {
  Ok(noise.decrypt(&receive_binary(socket).await?)?)
}

async fn receive_binary(socket: &mut Socket) -> anyhow::Result<Vec<u8>> {
  match socket.next().await {
    Some(Ok(Message::Binary(data))) => Ok(data.to_vec()),
    Some(Ok(_)) => bail!("Service sent an unexpected handshake frame"),
    Some(Err(err)) => Err(err.into()),
    None => bail!("Service closed during handshake"),
  }
}

pub struct GuiUpdater {
  version: String,
  notifier: Arc<ProgressNotifier>,
  installer: Installer,
  in_progress: RwLock<bool>,
  progress: Arc<RwLock<Option<f64>>>,
}

impl GuiUpdater {
  pub fn new(version: String, notifier: ProgressNotifier, installer: Installer) -> Self {
    Self {
      version,
      notifier: Arc::new(notifier),
      installer,
      in_progress: Default::default(),
      progress: Default::default(),
    }
  }

  pub async fn update(&self) {
    if !cfg!(any(target_arch = "x86_64", target_arch = "aarch64")) {
      info!("GUI version is not supported on this architecture.");
      return;
    }
    info!("Update GUI, version: {}", self.version);

    #[cfg(debug_assertions)]
    let release_tag = "snapshot";
    #[cfg(not(debug_assertions))]
    let release_tag = if SNAPSHOT == "true" {
      String::from("snapshot")
    } else {
      format!("v{}", self.version)
    };

    let arch = if cfg!(target_arch = "x86_64") {
      "x86_64"
    } else {
      "aarch64"
    };

    let file_url = format!(
      "https://github.com/yuezk/GlobalProtect-openconnect/releases/download/{}/gpgui_{}{}.bin.tar.xz",
      release_tag, arch, GUI_LIBC_SUFFIX
    );
    let checksum_url = format!("{}.sha256", file_url);

    info!("Downloading file: {}", file_url);

    let dl = FileDownloader::new(&file_url);
    let cf = ChecksumFetcher::new(&checksum_url);
    let notifier = Arc::clone(&self.notifier);

    let progress_ref = Arc::clone(&self.progress);
    dl.on_progress(move |progress| {
      // Save progress to shared state so that it can be notified to the UI when needed
      if let Ok(mut guard) = progress_ref.try_write() {
        *guard = progress;
      }
      notifier.notify(progress);
    });

    self.set_in_progress(true);
    let res = tokio::try_join!(dl.download(), cf.fetch());

    let (file, checksum) = match res {
      Ok((file, checksum)) => (file, checksum),
      Err(err) => {
        warn!("Download error: {}", err);
        self.notify_error();
        return;
      }
    };

    let path = file.into_temp_path();
    let file_path = path.to_string_lossy();

    if let Err(err) = verify_checksum(&file_path, &checksum) {
      warn!("Checksum error: {}", err);
      self.notify_error();
      return;
    }

    info!("Checksum success");

    if let Err(err) = self.installer.install(&file_path, &checksum).await {
      warn!("Install error: {}", err);
      self.notify_error();
    } else {
      info!("Install success");
      self.notify_done();
    }
  }

  pub fn is_in_progress(&self) -> bool {
    if let Ok(guard) = self.in_progress.try_read() {
      *guard
    } else {
      info!("Failed to acquire in_progress lock");
      false
    }
  }

  fn set_in_progress(&self, in_progress: bool) {
    if let Ok(mut guard) = self.in_progress.try_write() {
      *guard = in_progress;
    } else {
      info!("Failed to acquire in_progress lock");
    }
  }

  fn notify_error(&self) {
    self.set_in_progress(false);
    self.notifier.notify_error();
  }

  fn notify_done(&self) {
    self.set_in_progress(false);
    self.notifier.notify_done();
  }

  pub fn notify_progress(&self) {
    let progress = if let Ok(guard) = self.progress.try_read() {
      *guard
    } else {
      info!("Failed to acquire progress lock");
      None
    };

    self.notifier.notify(progress);
  }
}

#[cfg(test)]
mod tests {
  use gpapi::{
    os_profile::HostIdentity,
    service::{
      transport::{ClientPrelude, NoiseResponder},
      vpn_env::{HostInfo, VpnEnv},
      vpn_state::VpnState,
    },
  };
  use tokio::net::TcpListener;
  use tokio_tungstenite::accept_async;

  use super::*;

  #[tokio::test]
  async fn pending_install_reconnects_with_a_fresh_handshake() {
    let credential = Arc::new(SessionCredential::generate(Uuid::new_v4(), env!("CARGO_PKG_VERSION")).unwrap());
    let (command_tx, mut commands) = mpsc::channel(1);
    let (reply_tx, reply_rx) = oneshot::channel();
    command_tx
      .send(InstallCommand {
        request: UpdateGuiRequest {
          path: "/tmp/gpgui-update".to_owned(),
          checksum: "checksum".to_owned(),
        },
        reply: reply_tx,
      })
      .await
      .unwrap();
    let (_lease_tx, mut credential_lease) = watch::channel(true);
    let mut pending = None;

    let (first_endpoint, first_service) = spawn_test_service(Arc::clone(&credential), false).await;
    let (mut socket, mut noise) = connect_installer_at(&credential, &first_endpoint).await.unwrap();
    assert!(
      run_installer_connection(
        &mut socket,
        &mut noise,
        &mut credential_lease,
        &mut commands,
        &mut pending,
      )
      .await
      .is_err()
    );
    first_service.await.unwrap();
    assert!(pending.is_some());

    let (second_endpoint, second_service) = spawn_test_service(Arc::clone(&credential), true).await;
    let (mut socket, mut noise) = connect_installer_at(&credential, &second_endpoint).await.unwrap();
    assert!(
      run_installer_connection(
        &mut socket,
        &mut noise,
        &mut credential_lease,
        &mut commands,
        &mut pending,
      )
      .await
      .is_err()
    );
    second_service.await.unwrap();
    assert!(pending.is_none());
    assert!(reply_rx.await.unwrap().is_ok());
  }

  #[tokio::test]
  async fn credential_version_mismatch_is_terminal() {
    let credential = SessionCredential::generate(Uuid::new_v4(), "different-version").unwrap();
    let Err(err) = connect_installer_at(&credential, "ws://127.0.0.1:1/ws").await else {
      panic!("version mismatch should fail before connecting");
    };
    assert!(err.is::<TerminalConnectError>());
  }

  async fn spawn_test_service(
    credential: Arc<SessionCredential>,
    reply: bool,
  ) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("ws://{}/ws", listener.local_addr().unwrap());
    let service = tokio::spawn(async move {
      let (stream, _) = listener.accept().await.unwrap();
      let mut socket = accept_async(stream).await.unwrap();
      let prelude = match socket.next().await.unwrap().unwrap() {
        Message::Binary(data) => ClientPrelude::decode(&data).unwrap(),
        frame => panic!("unexpected prelude frame: {frame:?}"),
      };
      assert_eq!(prelude.service_instance_id, credential.service_instance_id());
      assert_eq!(prelude.session_id, credential.session_id());
      socket
        .send(Message::Binary(ServerPrelude::Ready.encode().unwrap().into()))
        .await
        .unwrap();

      let mut responder = NoiseResponder::new(
        credential.product_version(),
        credential.service_instance_id(),
        credential.session_id(),
        credential.secret(),
      )
      .unwrap();
      let first = match socket.next().await.unwrap().unwrap() {
        Message::Binary(data) => data,
        frame => panic!("unexpected handshake frame: {frame:?}"),
      };
      responder.read_first(&first).unwrap();
      let (response, mut noise) = responder.write_response().unwrap();
      socket.send(Message::Binary(response.into())).await.unwrap();

      let attach = match socket.next().await.unwrap().unwrap() {
        Message::Binary(data) => data,
        frame => panic!("unexpected attach frame: {frame:?}"),
      };
      assert!(matches!(
        noise.decrypt::<ClientMessage>(&attach).unwrap(),
        ClientMessage::Attach { .. }
      ));
      let attached = ServerMessage::Attached {
        connection_id: Uuid::new_v4(),
        current_revision: 0,
        snapshot: VpnEnv {
          vpn_state: VpnState::Disconnected,
          vpnc_script: None,
          csd_wrapper: None,
          auth_executable: String::new(),
          host_info: HostInfo {
            host_identity: HostIdentity::collect(),
          },
        },
      };
      socket
        .send(Message::Binary(noise.encrypt(&attached).unwrap().into()))
        .await
        .unwrap();

      let request = match socket.next().await.unwrap().unwrap() {
        Message::Binary(data) => noise.decrypt::<ClientMessage>(&data).unwrap(),
        frame => panic!("unexpected request frame: {frame:?}"),
      };
      let ClientMessage::Request {
        id,
        request: WsRequest::UpdateGui(update),
      } = request
      else {
        panic!("unexpected installer request");
      };
      assert_eq!(update.path, "/tmp/gpgui-update");
      if reply {
        let message = ServerMessage::Reply {
          id,
          result: ServiceResult::Accepted,
        };
        socket
          .send(Message::Binary(noise.encrypt(&message).unwrap().into()))
          .await
          .unwrap();
      }
      socket.close(None).await.unwrap();
    });
    (endpoint, service)
  }
}
