use std::{ops::ControlFlow, sync::Arc};

use axum::extract::ws::{CloseFrame, Message};
use gpapi::{
  service::{event::WsEvent, request::WsRequest},
  utils::crypto::Crypto,
};
use log::{info, warn};
use tokio::sync::mpsc;

pub(crate) struct WsConnection {
  crypto: Arc<Crypto>,
  tx: mpsc::Sender<Message>,
}

impl WsConnection {
  pub fn new(crypto: Arc<Crypto>, tx: mpsc::Sender<Message>) -> Self {
    Self { crypto, tx }
  }

  pub async fn send_event(&self, event: &WsEvent) -> anyhow::Result<()> {
    let encrypted = self.crypto.encrypt(event)?;
    let msg = Message::Binary(encrypted);

    self.tx.send(msg).await?;

    Ok(())
  }

  pub fn recv_msg(&self, msg: Message) -> ControlFlow<(), WsRequest> {
    match msg {
      Message::Binary(data) => match self.crypto.decrypt(data) {
        Ok(ws_req) => ControlFlow::Continue(ws_req),
        Err(err) => {
          info!("Failed to decrypt message: {}", err);
          ControlFlow::Break(())
        }
      },
      Message::Close(cf) => {
        if let Some(CloseFrame { code, reason }) = cf {
          info!("Client sent close, code {} and reason `{}`", code, reason);
        } else {
          info!("Client somehow sent close message without CloseFrame");
        }
        ControlFlow::Break(())
      }
      _ => {
        warn!("WS server received unexpected message: {:?}", msg);
        ControlFlow::Break(())
      }
    }
  }
}
