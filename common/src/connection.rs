use crate::request::Request;
use crate::server::ServerContext;
use crate::Reader;
use crate::Response;
use crate::ResponseData;
use crate::VpnStatus;
use crate::Writer;
use std::sync::Arc;
use tokio::io::{self, ReadHalf, WriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

async fn handle_read(
    read_stream: ReadHalf<UnixStream>,
    server_context: Arc<ServerContext>,
    response_tx: mpsc::Sender<Response>,
    cancel_token: CancellationToken,
) {
    let mut reader: Reader = read_stream.into();

    loop {
        match reader.read::<Request>().await {
            Ok(request) => {
                println!("Received request: {:?}", request);
                let command = request.command();
                let context = server_context.clone().into();

                let mut response = match command.handle(context).await {
                    Ok(data) => Response::from(data),
                    Err(err) => Response::from(err.to_string()),
                };
                response.set_request_id(request.id());

                let _ = response_tx.send(response).await;
            }

            Err(err) if err.kind() == io::ErrorKind::ConnectionAborted => {
                println!("Client disconnected");
                cancel_token.cancel();
                break;
            }

            Err(err) => {
                println!("Error receiving command: {:?}", err);
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
                println!("Sending response: {:?}", response);
                if let Err(err) = writer.write(&response).await {
                    println!("Error sending response: {:?}", err);
                } else {
                    println!("Response sent");
                }
            }
            _ = cancel_token.cancelled() => {
                println!("Exiting write loop");
                break;
            }
            else => {
                println!("Error receiving response");
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
    println!("Waiting for status change");
    let start_time = std::time::Instant::now();

    loop {
        tokio::select! {
            _ = status_rx.changed() => {
                println!("Status changed: {:?}", start_time.elapsed());
                send_status(&status_rx, &response_tx).await;
            }
            _ = cancel_token.cancelled() => {
                println!("Exiting status loop");
                break;
            }
            else => {
                println!("Error receiving status");
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
        println!("Error sending status: {:?}", err);
    }
}

pub(crate) async fn handle_connection(socket: UnixStream, context: Arc<ServerContext>) {
    let (read_stream, write_stream) = io::split(socket);
    let (response_tx, response_rx) = mpsc::channel::<Response>(32);
    let cancel_token = CancellationToken::new();
    let status_rx = context.vpn().status_rx().await;

    let read_handle = tokio::spawn(handle_read(
        read_stream,
        context.clone(),
        response_tx.clone(),
        cancel_token.clone(),
    ));

    let write_handle = tokio::spawn(handle_write(
        write_stream,
        response_rx,
        cancel_token.clone(),
    ));

    let status_handle = tokio::spawn(handle_status_change(
        status_rx,
        response_tx.clone(),
        cancel_token,
    ));

    let _ = tokio::join!(read_handle, write_handle, status_handle);

    println!("Connection closed")
}
