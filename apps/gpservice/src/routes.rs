use std::sync::Arc;

use axum::{routing::{get, post}, Router};

use crate::{handlers, ws_server::WsServerContext};

pub(crate) fn routes(ctx: Arc<WsServerContext>) -> Router {
  Router::new()
    .route("/health", get(handlers::health))
    .route("/active-gui", post(handlers::active_gui))
    .route("/ws", get(handlers::ws_handler))
    .with_state(ctx)
}
