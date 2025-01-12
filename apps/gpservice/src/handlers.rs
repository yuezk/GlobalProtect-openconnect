use std::{
  fs::{File, Permissions},
  io::BufReader,
  ops::ControlFlow,
  os::unix::fs::PermissionsExt,
  path::PathBuf,
  sync::Arc,
};

use anyhow::bail;
use axum::{
  body::Bytes,
  extract::{
    ws::{self, CloseFrame, Message, Utf8Bytes, WebSocket},
    State, WebSocketUpgrade,
  },
  http::StatusCode,
  response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use gpapi::{
  service::{event::WsEvent, request::UpdateGuiRequest},
  utils::checksum::verify_checksum,
  GP_GUI_BINARY,
};
use log::{info, warn};
use tar::Archive;
use tokio::fs;
use xz2::read::XzDecoder;

use crate::ws_server::WsServerContext;

pub(crate) async fn health() -> impl IntoResponse {
  "OK"
}

pub(crate) async fn active_gui(State(ctx): State<Arc<WsServerContext>>) -> impl IntoResponse {
  ctx.send_event(WsEvent::ActiveGui).await;
}

pub async fn update_gui(State(ctx): State<Arc<WsServerContext>>, body: Bytes) -> Result<(), StatusCode> {
  let payload = match ctx.decrypt::<UpdateGuiRequest>(body.to_vec()) {
    Ok(payload) => payload,
    Err(err) => {
      warn!("Failed to decrypt update payload: {}", err);
      return Err(StatusCode::BAD_REQUEST);
    }
  };

  info!("Update GUI: {:?}", payload);
  let UpdateGuiRequest { path, checksum } = payload;

  info!("Verifying checksum");
  verify_checksum(&path, &checksum).map_err(|err| {
    warn!("Failed to verify checksum: {}", err);
    StatusCode::BAD_REQUEST
  })?;

  info!("Installing GUI");
  install_gui(&path).await.map_err(|err| {
    warn!("Failed to install GUI: {}", err);
    StatusCode::INTERNAL_SERVER_ERROR
  })?;

  Ok(())
}

// Unpack GPGUI archive, gpgui_2.0.0_{arch}.bin.tar.xz and install it
async fn install_gui(src: &str) -> anyhow::Result<()> {
  let path = PathBuf::from(GP_GUI_BINARY);
  let Some(dir) = path.parent() else {
    bail!("Failed to get parent directory of GUI binary");
  };

  fs::create_dir_all(dir).await?;

  // Unpack the archive
  info!("Unpacking GUI archive");
  let tar = XzDecoder::new(BufReader::new(File::open(src)?));
  let mut ar = Archive::new(tar);

  for entry in ar.entries()? {
    let mut entry = entry?;
    let path = entry.path()?;

    if let Some(name) = path.file_name() {
      let name = name.to_string_lossy();

      if name == "gpgui" {
        let mut file = File::create(GP_GUI_BINARY)?;
        std::io::copy(&mut entry, &mut file)?;
        break;
      }
    }
  }

  // Make the binary executable
  fs::set_permissions(GP_GUI_BINARY, Permissions::from_mode(0o755)).await?;

  Ok(())
}

pub(crate) async fn ws_handler(ws: WebSocketUpgrade, State(ctx): State<Arc<WsServerContext>>) -> impl IntoResponse {
  ws.on_upgrade(move |socket| handle_socket(socket, ctx))
}

async fn handle_socket(mut socket: WebSocket, ctx: Arc<WsServerContext>) {
  // Send ping message
  if let Err(err) = socket.send(Message::Ping("Hi".into())).await {
    warn!("Failed to send ping: {}", err);
    return;
  }

  // Wait for pong message
  if socket.recv().await.is_none() {
    warn!("Failed to receive pong");
    return;
  }

  info!("New client connected");

  let (mut sender, mut receiver) = socket.split();
  let (connection, mut msg_rx) = ctx.add_connection().await;

  let send_task = tokio::spawn(async move {
    while let Some(msg) = msg_rx.recv().await {
      if let Err(err) = sender.send(msg).await {
        info!("Failed to send message: {}", err);
        break;
      }
    }

    let close_msg = Message::Close(Some(CloseFrame {
      code: ws::close_code::NORMAL,
      reason: Utf8Bytes::from("Goodbye"),
    }));

    if let Err(err) = sender.send(close_msg).await {
      warn!("Failed to close socket: {}", err);
    }
  });

  let conn = Arc::clone(&connection);
  let ctx_clone = Arc::clone(&ctx);
  let recv_task = tokio::spawn(async move {
    while let Some(Ok(msg)) = receiver.next().await {
      let ControlFlow::Continue(ws_req) = conn.recv_msg(msg) else {
        break;
      };

      if let Err(err) = ctx_clone.forward_req(ws_req).await {
        info!("Failed to forward request: {}", err);
        break;
      }
    }
  });

  tokio::select! {
    _ = send_task => {
        info!("WS server send task completed");
    },
    _ = recv_task => {
        info!("WS server recv task completed");
    }
  }

  info!("Client disconnected");

  ctx.remove_connection(connection).await;
}
