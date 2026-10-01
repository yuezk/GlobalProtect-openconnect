use std::{collections::HashSet, sync::Arc, time::Duration};

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use futures::StreamExt;
use gpapi::service::transport::{
  ClientMessage, ClientPrelude, CloseReason, HandshakeRejection, NoiseResponder, NoiseTransport, ServerMessage,
  ServerPrelude, ServiceErrorCode, ServiceResult,
};
use gpapi::{hip::HipSource, service::request::WsRequest};
use log::{info, warn};
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use uuid::Uuid;

use crate::{
  session_registry::{HandshakePermit, SessionError},
  ws_server::WsServerContext,
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
const PONG_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECTION_QUEUE: usize = 64;
const MAX_PENDING_REQUESTS: usize = 64;

pub(crate) enum ConnectionCommand {
  Send(ServerMessage),
  Reply { id: Uuid, result: ServiceResult },
}

#[derive(Clone)]
pub(crate) struct ConnectionControl {
  connection_id: Uuid,
  tx: mpsc::Sender<ConnectionCommand>,
  close_tx: watch::Sender<Option<CloseReason>>,
}

impl ConnectionControl {
  pub fn new(connection_id: Uuid, tx: mpsc::Sender<ConnectionCommand>) -> (Self, watch::Receiver<Option<CloseReason>>) {
    let (close_tx, close_rx) = watch::channel(None);
    (
      Self {
        connection_id,
        tx,
        close_tx,
      },
      close_rx,
    )
  }

  pub fn connection_id(&self) -> Uuid {
    self.connection_id
  }

  pub fn send(&self, command: ConnectionCommand) -> Result<(), ()> {
    self.tx.try_send(command).map_err(|_| ())
  }

  pub fn is_closed(&self) -> bool {
    self.tx.is_closed()
  }

  pub fn close(&self, reason: CloseReason) {
    let _ = self.close_tx.send(Some(reason));
  }

  pub async fn send_reply(&self, id: Uuid, result: ServiceResult) {
    let _ = self.tx.send(ConnectionCommand::Reply { id, result }).await;
  }
}

pub(crate) struct WsConnection;

impl WsConnection {
  pub async fn accept(mut socket: WebSocket, ctx: Arc<WsServerContext>) {
    let Some(slot) = ctx.try_handshake_slot() else {
      send_close(&mut socket, CloseReason::InternalError).await;
      return;
    };

    let established = tokio::time::timeout(HANDSHAKE_TIMEOUT, establish(&mut socket, &ctx)).await;
    let (mut noise, permit, control, mut command_rx, mut close_rx, generation) = match established {
      Ok(Ok(established)) => established,
      Ok(Err(reason)) => {
        send_close(&mut socket, reason).await;
        return;
      }
      Err(_) => {
        ctx.remove_closed_connections().await;
        send_close(&mut socket, CloseReason::HandshakeTimeout).await;
        return;
      }
    };
    drop(slot);

    info!("Authenticated GUI connection {}", control.connection_id());
    let mut pending_requests = HashSet::new();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(1));
    let mut last_outbound = Instant::now();
    let mut pong_deadline = None;

    loop {
      tokio::select! {
        command = command_rx.recv() => {
          let Some(command) = command else { break };
          match command {
            ConnectionCommand::Send(message) => {
              if send_encrypted(&mut socket, &mut noise, &message).await.is_err() {
                break;
              }
              last_outbound = Instant::now();
            }
            ConnectionCommand::Reply { id, result } => {
              if !pending_requests.remove(&id) {
                send_close(&mut socket, CloseReason::MalformedMessage).await;
                break;
              }
              let message = ServerMessage::Reply { id, result };
              if send_encrypted(&mut socket, &mut noise, &message).await.is_err() {
                break;
              }
              last_outbound = Instant::now();
            }
          }
        }
        changed = close_rx.changed() => {
          if changed.is_err() { break; }
          let reason = *close_rx.borrow_and_update();
          if let Some(reason) = reason {
            send_close(&mut socket, reason).await;
            break;
          }
        }
        frame = socket.next() => {
          let Some(frame) = frame else { break };
          match frame {
            Ok(Message::Binary(data)) => {
              let message = match noise.decrypt::<ClientMessage>(&data) {
                Ok(message) => message,
                Err(err) => {
                  warn!("Failed to authenticate WS message: {err}");
                  send_close(&mut socket, CloseReason::AuthenticationFailed).await;
                  break;
                }
              };
              if handle_client_message(
                message,
                &ctx,
                &control,
                permit.session_id,
                permit.desktop_uid,
                generation,
                &mut pending_requests,
              ).await.is_err() {
                send_close(&mut socket, CloseReason::MalformedMessage).await;
                break;
              }
            }
            Ok(Message::Ping(data)) => {
              if send_frame(&mut socket, Message::Pong(data)).await.is_err() { break; }
              last_outbound = Instant::now();
            }
            Ok(Message::Pong(_)) => pong_deadline = None,
            Ok(Message::Close(_)) | Err(_) => break,
            Ok(_) => {
              send_close(&mut socket, CloseReason::MalformedMessage).await;
              break;
            }
          }
        }
        _ = heartbeat.tick() => {
          let now = Instant::now();
          if pong_deadline.is_some_and(|deadline| now >= deadline) {
            send_close(&mut socket, CloseReason::HeartbeatTimeout).await;
            break;
          }
          if pong_deadline.is_none() && now.duration_since(last_outbound) >= HEARTBEAT_INTERVAL {
            if send_frame(&mut socket, Message::Ping(Vec::new().into())).await.is_err() { break; }
            last_outbound = now;
            pong_deadline = Some(now + PONG_TIMEOUT);
          }
        }
      }
    }

    ctx
      .hip_previews
      .cancel_connection(permit.session_id, control.connection_id())
      .await;
    ctx
      .remove_connection(control.connection_id(), permit.session_id, generation)
      .await;
    info!("GUI connection {} closed", control.connection_id());
  }
}

async fn establish(
  socket: &mut WebSocket,
  ctx: &Arc<WsServerContext>,
) -> Result<
  (
    NoiseTransport,
    HandshakePermit,
    ConnectionControl,
    mpsc::Receiver<ConnectionCommand>,
    watch::Receiver<Option<CloseReason>>,
    u64,
  ),
  CloseReason,
> {
  let prelude = receive_prelude(socket).await?;
  if prelude.product_version != ctx.product_version() {
    send_server_prelude(
      socket,
      ServerPrelude::Rejected(HandshakeRejection::VersionMismatch {
        client: prelude.product_version,
        service: ctx.product_version().to_owned(),
      }),
    )
    .await?;
    return Err(CloseReason::VersionMismatch);
  }
  if prelude.hip_protocol_version != gpapi::service::transport::HIP_PROTOCOL_VERSION {
    send_server_prelude(
      socket,
      ServerPrelude::Rejected(HandshakeRejection::HipProtocolMismatch {
        client: prelude.hip_protocol_version,
        service: gpapi::service::transport::HIP_PROTOCOL_VERSION,
      }),
    )
    .await?;
    return Err(CloseReason::VersionMismatch);
  }

  let permit = match ctx.registry().begin_handshake(
    prelude.service_instance_id,
    prelude.session_id,
    &prelude.product_version,
  ) {
    Ok(permit) => permit,
    Err(SessionError::ServiceRestarted { service_instance_id }) => {
      send_server_prelude(
        socket,
        ServerPrelude::Rejected(HandshakeRejection::ServiceRestarted { service_instance_id }),
      )
      .await?;
      return Err(CloseReason::ServiceRestarted);
    }
    Err(SessionError::ReconnectSuperseded) => {
      send_server_prelude(socket, ServerPrelude::Rejected(HandshakeRejection::ReconnectSuperseded)).await?;
      return Err(CloseReason::ReconnectSuperseded);
    }
    Err(_) => {
      send_server_prelude(socket, ServerPrelude::Rejected(HandshakeRejection::Unauthorized)).await?;
      return Err(CloseReason::Unauthorized);
    }
  };

  send_server_prelude(socket, ServerPrelude::Ready).await?;
  let handshake = async {
    let mut responder = NoiseResponder::new(
      ctx.product_version(),
      prelude.service_instance_id,
      prelude.session_id,
      &permit.secret,
    )
    .map_err(|_| CloseReason::HandshakeFailed)?;
    let first = receive_binary(socket).await?;
    responder.read_first(&first).map_err(|_| CloseReason::HandshakeFailed)?;
    let (response, mut noise) = responder.write_response().map_err(|_| CloseReason::HandshakeFailed)?;
    socket
      .send(Message::Binary(response.into()))
      .await
      .map_err(|_| CloseReason::HandshakeFailed)?;

    let attach = receive_binary(socket).await?;
    match noise
      .decrypt::<ClientMessage>(&attach)
      .map_err(|_| CloseReason::AuthenticationFailed)?
    {
      ClientMessage::Attach { .. } => {}
      _ => return Err(CloseReason::MalformedMessage),
    }

    let (tx, mut rx) = mpsc::channel(CONNECTION_QUEUE);
    let (control, close_rx) = ConnectionControl::new(Uuid::new_v4(), tx);
    let prepared = ctx
      .prepare_connection(&permit, control.clone())
      .await
      .map_err(|err| match err {
        SessionError::ReconnectSuperseded => CloseReason::ReconnectSuperseded,
        SessionError::Unauthorized => CloseReason::Unauthorized,
        _ => CloseReason::InternalError,
      })?;

    let Some(ConnectionCommand::Send(attached @ ServerMessage::Attached { .. })) = rx.recv().await else {
      ctx.abandon_connection(control.connection_id()).await;
      return Err(CloseReason::InternalError);
    };
    if send_encrypted(socket, &mut noise, &attached).await.is_err() {
      ctx.abandon_connection(control.connection_id()).await;
      return Err(CloseReason::InternalError);
    }
    let replaced = ctx
      .commit_connection(&permit, &control)
      .await
      .map_err(|err| match err {
        SessionError::ReconnectSuperseded => CloseReason::ReconnectSuperseded,
        SessionError::Unauthorized => CloseReason::Unauthorized,
        _ => CloseReason::InternalError,
      })?;
    if let Some(replaced) = replaced {
      replaced.close(CloseReason::SessionReplaced);
    }
    Ok((noise, control, rx, close_rx, prepared.generation))
  }
  .await;

  handshake.map(|(noise, control, rx, close_rx, generation)| (noise, permit, control, rx, close_rx, generation))
}

async fn handle_client_message(
  message: ClientMessage,
  ctx: &Arc<WsServerContext>,
  control: &ConnectionControl,
  session_id: Uuid,
  desktop_uid: Option<u32>,
  generation: u64,
  pending_requests: &mut HashSet<Uuid>,
) -> Result<(), ()> {
  match message {
    ClientMessage::Attach { .. } => Err(()),
    ClientMessage::Resync { .. } => ctx.send_snapshot(control).await,
    ClientMessage::Request { id, request } => {
      if pending_requests.len() >= MAX_PENDING_REQUESTS || !pending_requests.insert(id) {
        return Err(());
      }
      if !ctx.registry().is_current(session_id, generation) {
        pending_requests.remove(&id);
        return Err(());
      }

      if let WsRequest::StoreEditedHipReportChunk(report) = request {
        let upload_id = report.upload_id.as_deref().and_then(|id| Uuid::parse_str(id).ok());
        let result = if report.upload_id.is_some() && upload_id.is_none() {
          ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Invalid HIP upload ID")
        } else {
          match ctx.registry().append_edited_hip_report_chunk(
            session_id,
            upload_id,
            report.offset,
            report.xml,
            report.complete,
          ) {
            Ok(crate::session_registry::HipUploadResult::Progress(upload_id)) => ServiceResult::HipUploadProgress {
              upload_id: upload_id.to_string(),
            },
            Ok(crate::session_registry::HipUploadResult::Stored(report_id)) => ServiceResult::StoredHipReport {
              report_id: report_id.to_string(),
            },
            Err(_) => ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Invalid HIP report upload"),
          }
        };
        if control.send(ConnectionCommand::Reply { id, result }).is_err() {
          pending_requests.remove(&id);
          control.close(CloseReason::InternalError);
        }
        return Ok(());
      }

      if let WsRequest::ReadHipPreviewChunk(chunk) = request {
        let result = Uuid::parse_str(&chunk.preview_id)
          .ok()
          .and_then(|preview_id| {
            ctx
              .registry()
              .hip_preview_chunk(session_id, preview_id, chunk.offset)
              .ok()
              .map(|(xml, complete)| ServiceResult::HipPreviewChunk {
                preview_id: preview_id.to_string(),
                offset: chunk.offset,
                xml,
                complete,
              })
          })
          .unwrap_or_else(|| ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "HIP preview is unavailable"));
        control.send(ConnectionCommand::Reply { id, result }).map_err(|_| ())?;
        return Ok(());
      }

      if let WsRequest::GetLastSubmittedHipReport = request {
        let result = match ctx.registry().last_successful_hip_report(session_id) {
          Ok(Some(report)) => match ctx.registry().store_hip_preview(session_id, report.to_string()) {
            Ok(preview_id) => match ctx.registry().hip_preview_chunk(session_id, preview_id, 0) {
              Ok((xml, complete)) => ServiceResult::HipPreviewChunk {
                preview_id: preview_id.to_string(),
                offset: 0,
                xml,
                complete,
              },
              Err(_) => ServiceResult::rejected(ServiceErrorCode::Internal, "Submitted HIP report is unavailable"),
            },
            Err(_) => ServiceResult::rejected(ServiceErrorCode::Internal, "Submitted HIP report is unavailable"),
          },
          Ok(None) => ServiceResult::NoSubmittedHipReport,
          Err(_) => ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Session is unavailable"),
        };
        control.send(ConnectionCommand::Reply { id, result }).map_err(|_| ())?;
        return Ok(());
      }

      if let WsRequest::GetHipApprovalStatus { approval_id } = request {
        let result = if let Some(uid) = desktop_uid.filter(|uid| *uid != 0) {
          #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
          {
            match gpservice::hip_approval::status(&approval_id, uid) {
              Ok(status) => ServiceResult::HipApprovalStatus(status),
              Err(_) => ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Invalid HIP approval ID"),
            }
          }
          #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
          {
            let _ = (uid, approval_id);
            ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "HIP script approvals are unavailable")
          }
        } else {
          ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Desktop user is unavailable")
        };
        control.send(ConnectionCommand::Reply { id, result }).map_err(|_| ())?;
        return Ok(());
      }

      if let WsRequest::PreviewHipReport(preview) = request {
        let edited_report = if let HipSource::Edited { report_id } = &preview.source {
          let report_id = match Uuid::parse_str(report_id) {
            Ok(id) => id,
            Err(_) => {
              control
                .send(ConnectionCommand::Reply {
                  id,
                  result: ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Invalid HIP report ID"),
                })
                .map_err(|_| ())?;
              return Ok(());
            }
          };
          match ctx.registry().edited_hip_report(session_id, report_id) {
            Ok(report) => Some(report),
            Err(_) => {
              control
                .send(ConnectionCommand::Reply {
                  id,
                  result: ServiceResult::rejected(
                    ServiceErrorCode::InvalidRequest,
                    "HIP report is unavailable for this session",
                  ),
                })
                .map_err(|_| ())?;
              return Ok(());
            }
          }
        } else {
          None
        };
        if !ctx
          .hip_previews
          .start(crate::hip_preview::PreviewJob {
            session_id,
            request_id: id,
            request: preview,
            edited_report,
            desktop_uid,
            identity: ctx.host_identity().await,
            registry: Arc::clone(ctx.registry()),
            connection: control.clone(),
          })
          .await
        {
          control
            .send(ConnectionCommand::Reply {
              id,
              result: ServiceResult::rejected(ServiceErrorCode::Busy, "HIP preview is already running"),
            })
            .map_err(|_| ())?;
        }
        return Ok(());
      }

      let edited_report = if let WsRequest::Connect(connect) = &request {
        if let HipSource::Edited { report_id } = connect.args().hip_source() {
          let report_id = match Uuid::parse_str(report_id) {
            Ok(id) => id,
            Err(_) => {
              control
                .send(ConnectionCommand::Reply {
                  id,
                  result: ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Invalid HIP report ID"),
                })
                .map_err(|_| ())?;
              return Ok(());
            }
          };
          match ctx.registry().edited_hip_report(session_id, report_id) {
            Ok(report) => Some(report),
            Err(_) => {
              control
                .send(ConnectionCommand::Reply {
                  id,
                  result: ServiceResult::rejected(
                    ServiceErrorCode::InvalidRequest,
                    "HIP report is unavailable for this session",
                  ),
                })
                .map_err(|_| ())?;
              return Ok(());
            }
          }
        } else {
          None
        }
      } else {
        None
      };

      let dispatcher = ctx.dispatcher();
      let request = match dispatcher.dispatch_lifecycle(request, desktop_uid, edited_report, Some(session_id)) {
        Ok(request) => request,
        Err(result) => {
          if control.send(ConnectionCommand::Reply { id, result }).is_err() {
            pending_requests.remove(&id);
            control.close(CloseReason::InternalError);
          }
          return Ok(());
        }
      };
      let control = control.clone();
      tokio::spawn(async move {
        let result = dispatcher.dispatch(request).await;
        control.send_reply(id, result).await;
      });
      Ok(())
    }
  }
}

async fn receive_prelude(socket: &mut WebSocket) -> Result<ClientPrelude, CloseReason> {
  let data = receive_binary(socket).await?;
  ClientPrelude::decode(&data).map_err(|_| CloseReason::MalformedMessage)
}

async fn receive_binary(socket: &mut WebSocket) -> Result<Vec<u8>, CloseReason> {
  match socket.next().await {
    Some(Ok(Message::Binary(data))) => Ok(data.to_vec()),
    Some(Ok(_)) => Err(CloseReason::MalformedMessage),
    Some(Err(_)) | None => Err(CloseReason::HandshakeFailed),
  }
}

async fn send_server_prelude(socket: &mut WebSocket, prelude: ServerPrelude) -> Result<(), CloseReason> {
  let data = prelude.encode().map_err(|_| CloseReason::InternalError)?;
  socket
    .send(Message::Binary(data.into()))
    .await
    .map_err(|_| CloseReason::HandshakeFailed)
}

async fn send_encrypted(
  socket: &mut WebSocket,
  noise: &mut NoiseTransport,
  message: &ServerMessage,
) -> anyhow::Result<()> {
  let encrypted = noise.encrypt(message)?;
  send_frame(socket, Message::Binary(encrypted.into())).await?;
  Ok(())
}

async fn send_frame(socket: &mut WebSocket, frame: Message) -> anyhow::Result<()> {
  tokio::time::timeout(WRITE_TIMEOUT, socket.send(frame))
    .await
    .map_err(|_| anyhow::anyhow!("WebSocket write timed out"))??;
  Ok(())
}

async fn send_close(socket: &mut WebSocket, reason: CloseReason) {
  let _ = send_frame(
    socket,
    Message::Close(Some(CloseFrame {
      code: reason.code(),
      reason: Utf8Bytes::from_static(reason.reason()),
    })),
  )
  .await;
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, atomic::AtomicBool};

  use futures::{SinkExt, StreamExt};
  use gpapi::{
    service::{
      request::{UpdateGuiRequest, UpdateLogLevelRequest, WsRequest},
      transport::{ClientMessage, ClientPrelude, NoiseInitiator, ServerMessage, ServerPrelude, ServiceResult},
      vpn_state::VpnState,
    },
    utils::redact::Redaction,
  };
  use tokio::sync::watch;
  use tokio_tungstenite::{connect_async, tungstenite::Message as ClientFrame};

  use super::*;
  use crate::{
    request_dispatcher::RequestDispatcher, routes, session_registry::SessionRegistry, ws_server::WsServerContext,
  };

  #[tokio::test]
  async fn authenticates_attaches_and_replies_to_a_request() {
    let registry = Arc::new(SessionRegistry::new(Uuid::new_v4()));
    let credential = registry.issue(env!("CARGO_PKG_VERSION"), None).unwrap();
    let dispatcher = Arc::new(RequestDispatcher::new(
      crate::vpn_task::LifecycleHandle::for_tests(),
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      true,
      None,
    ));
    let (_state_tx, state_rx) = watch::channel(VpnState::Disconnected);
    let context = Arc::new(WsServerContext::new(
      env!("CARGO_PKG_VERSION"),
      Arc::clone(&registry),
      dispatcher,
      &state_rx,
    ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
      axum::serve(listener, routes::routes(context)).await.unwrap();
    });

    let (mut socket, _) = connect_async(format!("ws://{address}/ws")).await.unwrap();
    let prelude = ClientPrelude {
      product_version: env!("CARGO_PKG_VERSION").to_owned(),
      hip_protocol_version: gpapi::service::transport::HIP_PROTOCOL_VERSION,
      service_instance_id: credential.service_instance_id(),
      session_id: credential.session_id(),
    };
    socket
      .send(ClientFrame::Binary(prelude.encode().unwrap().into()))
      .await
      .unwrap();
    let ready = receive_client_binary(&mut socket).await;
    assert_eq!(ServerPrelude::decode(&ready).unwrap(), ServerPrelude::Ready);

    let mut initiator = NoiseInitiator::new(
      env!("CARGO_PKG_VERSION"),
      credential.service_instance_id(),
      credential.session_id(),
      credential.secret(),
    )
    .unwrap();
    socket
      .send(ClientFrame::Binary(initiator.write_first().unwrap().into()))
      .await
      .unwrap();
    let response = receive_client_binary(&mut socket).await;
    let mut noise = initiator.read_response(&response).unwrap();
    let attach = noise
      .encrypt(&ClientMessage::Attach {
        last_event_revision: None,
      })
      .unwrap();
    socket.send(ClientFrame::Binary(attach.into())).await.unwrap();
    let attached = noise
      .decrypt::<ServerMessage>(&receive_client_binary(&mut socket).await)
      .unwrap();
    assert!(matches!(attached, ServerMessage::Attached { .. }));

    let request_id = Uuid::new_v4();
    let request = ClientMessage::Request {
      id: request_id,
      request: WsRequest::UpdateLogLevel(UpdateLogLevelRequest("info".to_owned())),
    };
    socket
      .send(ClientFrame::Binary(noise.encrypt(&request).unwrap().into()))
      .await
      .unwrap();
    let reply = noise
      .decrypt::<ServerMessage>(&receive_client_binary(&mut socket).await)
      .unwrap();
    assert!(matches!(
      reply,
      ServerMessage::Reply {
        id,
        result: ServiceResult::Accepted,
      } if id == request_id
    ));

    let update_id = Uuid::new_v4();
    let update_file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(update_file.path(), b"invalid archive").unwrap();
    let update = ClientMessage::Request {
      id: update_id,
      request: WsRequest::UpdateGui(UpdateGuiRequest {
        path: update_file.path().to_string_lossy().into_owned(),
        checksum: "invalid".to_owned(),
      }),
    };
    socket
      .send(ClientFrame::Binary(noise.encrypt(&update).unwrap().into()))
      .await
      .unwrap();
    let reply = noise
      .decrypt::<ServerMessage>(&receive_client_binary(&mut socket).await)
      .unwrap();
    assert!(matches!(
      reply,
      ServerMessage::Reply {
        id,
        result: ServiceResult::Rejected(ref rejection),
      } if id == update_id && rejection.code() == gpapi::service::transport::ServiceErrorCode::ChecksumFailed
    ));

    server.abort();
  }

  async fn receive_client_binary(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
  ) -> Vec<u8> {
    match socket.next().await.unwrap().unwrap() {
      ClientFrame::Binary(data) => data.to_vec(),
      frame => panic!("unexpected frame: {frame:?}"),
    }
  }
}
