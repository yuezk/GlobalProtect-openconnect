use std::sync::Arc;

use axum::{
  extract::{State, WebSocketUpgrade},
  response::IntoResponse,
};
use gpapi::service::{event::WsEvent, transport::MAX_WS_MESSAGE};

use crate::{ws_connection::WsConnection, ws_server::WsServerContext};

pub(crate) async fn health() -> impl IntoResponse {
  "OK"
}

pub(crate) async fn active_gui(State(ctx): State<Arc<WsServerContext>>) -> impl IntoResponse {
  ctx.send_event(WsEvent::ActiveGui).await;
}

pub(crate) async fn ws_handler(ws: WebSocketUpgrade, State(ctx): State<Arc<WsServerContext>>) -> impl IntoResponse {
  ws.max_message_size(MAX_WS_MESSAGE)
    .max_frame_size(MAX_WS_MESSAGE)
    .on_upgrade(move |socket| WsConnection::accept(socket, ctx))
}
