use crate::request::Request;
use crate::server::ServerContext;
use crate::Reader;
use crate::Response;
use crate::ResponseData;
use crate::VpnStatus;
use crate::Writer;
use log::{debug, info, warn};
use std::sync::Arc;
use tokio::io::{self, ReadHalf, WriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

async fn handle_read(
    read_stream: ReadHalf<UnixStream>,
    server_context: Arc<ServerContext>,
    response_tx: mpsc::Sender<Response>,
    peer_pid: Option<i32>,
    cancel_token: CancellationToken,
) {
    let mut reader: Reader = read_stream.into();
    let mut authenticated: Option<bool> = None;

    loop {
        match reader.read_multiple::<Request>().await {
            Ok(requests) => {
                if authenticated.is_none() {
                    authenticated = Some(authenticate(peer_pid));
                }
                if !authenticated.unwrap_or(false) {
                    warn!("Client not authenticated, closing connection");
                    cancel_token.cancel();
                    break;
                }

                for request in requests {
                    debug!("Received client request: {:?}", request);

                    let command = request.command();
                    let context = server_context.clone().into();

                    let mut response = match command.handle(context).await {
                        Ok(data) => Response::from(data),
                        Err(err) => Response::from(err.to_string()),
                    };
                    response.set_request_id(request.id());

                    let _ = response_tx.send(response).await;
                }
            }

            Err(err) if err.kind() == io::ErrorKind::ConnectionAborted => {
                info!("Client disconnected");
                cancel_token.cancel();
                break;
            }

            Err(err) => {
                warn!("Error receiving request: {:?}", err);
            }
        }
    }
}

async fn handle_write(
    write_stream: WriteHalf<UnixStream>,
    mut response_rx: mpsc::Receiver<Response>,
    cancel_token: CancellationToken,
) {
    let mut writer: Writer = write_stream.into();

    loop {
        tokio::select! {
            Some(response) = response_rx.recv() => {
                debug!("Sending response: {:?}", response);
                if let Err(err) = writer.write(&response).await {
                    warn!("Error sending response: {:?}", err);
                }
            }
            _ = cancel_token.cancelled() => {
                info!("Exiting the write loop");
                break;
            }
            else => {
                warn!("Error receiving response from channel");
            }
        }
    }
}

async fn handle_status_change(
    mut status_rx: watch::Receiver<VpnStatus>,
    response_tx: mpsc::Sender<Response>,
    cancel_token: CancellationToken,
) {
    // Send the initial status
    send_status(&status_rx, &response_tx).await;
    debug!("Waiting for status change");
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            _ = status_rx.changed() => {
                debug!("Status changed: {:?}", start_time.elapsed());
                send_status(&status_rx, &response_tx).await;
            }
            _ = cancel_token.cancelled() => {
                info!("Exiting the status loop");
                break;
            }
            else => {
                warn!("Error receiving status from channel");
            }
        }
    }
}

async fn send_status(status_rx: &watch::Receiver<VpnStatus>, response_tx: &mpsc::Sender<Response>) {
    let status = *status_rx.borrow();
    if let Err(err) = response_tx
        .send(Response::from(ResponseData::Status(status)))
        .await
    {
        warn!("Error sending status: {:?}", err);
    }
}

pub(crate) async fn handle_connection(socket: UnixStream, context: Arc<ServerContext>) {
    let peer_pid = peer_pid(&socket);
    let (read_stream, write_stream) = io::split(socket);
    let (response_tx, response_rx) = mpsc::channel::<Response>(32);
    let cancel_token = CancellationToken::new();
    let status_rx = context.vpn().status_rx().await;

    // Read requests from the client
    let read_handle = tokio::spawn(handle_read(
        read_stream,
        context.clone(),
        response_tx.clone(),
        peer_pid,
        cancel_token.clone(),
    ));

    // Write responses to the client
    let write_handle = tokio::spawn(handle_write(
        write_stream,
        response_rx,
        cancel_token.clone(),
    ));

    // Watch for status changes
    let status_handle = tokio::spawn(handle_status_change(
        status_rx,
        response_tx.clone(),
        cancel_token,
    ));

    let _ = tokio::join!(read_handle, write_handle, status_handle);

    debug!("Client connection closed");
}

fn peer_pid(socket: &UnixStream) -> Option<i32> {
    match socket.peer_cred() {
        Ok(ucred) => ucred.pid(),
        Err(_) => None,
    }
}

// TODO - Implement authentication
fn authenticate(peer_pid: Option<i32>) -> bool {
    if let Some(pid) = peer_pid {
        info!("Peer PID: {}", pid);
        true
    } else {
        false
    }
}
