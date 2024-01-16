use std::{borrow::Cow, ops::ControlFlow, sync::Arc};

use axum::{
  extract::{
    ws::{self, CloseFrame, Message, WebSocket},
    State, WebSocketUpgrade,
  },
  response::IntoResponse,
};
use futures::{SinkExt, StreamExt};
use gpapi::service::event::WsEvent;
use log::{info, warn};

use crate::ws_server::WsServerContext;

pub(crate) async fn health() -> impl IntoResponse {
  "OK"
}

pub(crate) async fn active_gui(State(ctx): State<Arc<WsServerContext>>) -> impl IntoResponse {
  ctx.send_event(WsEvent::ActiveGui).await;
}

pub(crate) async fn ws_handler(
  ws: WebSocketUpgrade,
  State(ctx): State<Arc<WsServerContext>>,
) -> impl IntoResponse {
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
      reason: Cow::from("Goodbye"),
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
