use crate::cmd::{Connect, Disconnect, Status};
use crate::reader::Reader;
use crate::request::CommandPayload;
use crate::response::ResponseData;
use crate::writer::Writer;
use crate::RequestPool;
use crate::Response;
use crate::SOCKET_PATH;
use crate::{Request, VpnStatus};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::sync::Arc;
use tokio::io::{self, ReadHalf, WriteHalf};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
enum ServerEvent {
    Response(Response),
    ServerDisconnected,
}

impl From<Response> for ServerEvent {
    fn from(response: Response) -> Self {
        Self::Response(response)
    }
}

#[derive(Debug)]
pub struct Client {
    // pool of requests that are waiting for responses
    request_pool: Arc<RequestPool>,
    // tx for sending requests to the channel
    request_tx: mpsc::Sender<Request>,
    // rx for receiving requests from the channel
    request_rx: Arc<Mutex<mpsc::Receiver<Request>>>,
    // tx for sending responses to the channel
    server_event_tx: mpsc::Sender<ServerEvent>,
    // rx for receiving responses from the channel
    server_event_rx: Arc<Mutex<mpsc::Receiver<ServerEvent>>>,
    is_healthy: Arc<RwLock<bool>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerApiError {
    pub message: String,
}

impl Display for ServerApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{message}", message = self.message)
    }
}

impl From<String> for ServerApiError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<&str> for ServerApiError {
    fn from(message: &str) -> Self {
        Self {
            message: message.to_string(),
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<Request>(32);
        let (server_event_tx, server_event_rx) = mpsc::channel::<ServerEvent>(32);

        Self {
            request_pool: Default::default(),
            request_tx,
            request_rx: Arc::new(Mutex::new(request_rx)),
            server_event_tx,
            server_event_rx: Arc::new(Mutex::new(server_event_rx)),
            is_healthy: Default::default(),
        }
    }
}

impl Client {
    pub fn subscribe_status(&self, callback: impl Fn(VpnStatus) + Send + Sync + 'static) {
        let server_event_rx = self.server_event_rx.clone();

        tokio::spawn(async move {
            loop {
                let mut server_event_rx = server_event_rx.lock().await;
                if let Some(server_event) = server_event_rx.recv().await {
                    match server_event {
                        ServerEvent::ServerDisconnected => {
                            callback(VpnStatus::Disconnected);
                        }
                        ServerEvent::Response(response) => {
                            if let ResponseData::Status(vpn_status) = response.data() {
                                callback(vpn_status);
                            }
                        }
                    }
                }
            }
        });
    }

    pub async fn run(&self) {
        loop {
            match self.connect_to_server().await {
                Ok(_) => {
                    println!("Disconnected from server, reconnecting...");
                }
                Err(err) => {
                    println!(
                        "Disconnected from server with error: {:?}, reconnecting...",
                        err
                    )
                }
            }

            // wait for a second before trying to reconnect
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }

    async fn connect_to_server(&self) -> Result<(), Box<dyn std::error::Error>> {
        let stream = UnixStream::connect(SOCKET_PATH).await?;
        let (read_stream, write_stream) = io::split(stream);
        let cancel_token = CancellationToken::new();

        let read_handle = tokio::spawn(handle_read(
            read_stream,
            self.request_pool.clone(),
            self.server_event_tx.clone(),
            cancel_token.clone(),
        ));

        let write_handle = tokio::spawn(handle_write(
            write_stream,
            self.request_rx.clone(),
            cancel_token,
        ));

        *self.is_healthy.write().await = true;
        println!("Connected to server");
        let _ = tokio::join!(read_handle, write_handle);
        *self.is_healthy.write().await = false;

        Ok(())
    }

    async fn send_command<T: TryFrom<ResponseData>>(
        &self,
        payload: CommandPayload,
    ) -> Result<T, ServerApiError> {
        if !*self.is_healthy.read().await {
            return Err("Background service is not running".into());
        }

        let (request, response_rx) = self.request_pool.create_request(payload).await;

        if let Err(err) = self.request_tx.send(request).await {
            return Err(format!("Error sending request to the channel: {}", err).into());
        }

        if let Ok(response) = response_rx.await {
            if response.success() {
                match response.data().try_into() {
                    Ok(it) => Ok(it),
                    Err(_) => Err("Error parsing response data".into()),
                }
            } else {
                Err(response.message().into())
            }
        } else {
            Err("Error receiving response from the channel".into())
        }
    }

    pub async fn connect(&self, server: String, cookie: String) -> Result<(), ServerApiError> {
        self.send_command(Connect::new(server, cookie).into()).await
    }

    pub async fn disconnect(&self) -> Result<(), ServerApiError> {
        self.send_command(Disconnect.into()).await
    }

    pub async fn status(&self) -> Result<VpnStatus, ServerApiError> {
        self.send_command(Status.into()).await
    }
}

async fn handle_read(
    read_stream: ReadHalf<UnixStream>,
    request_pool: Arc<RequestPool>,
    server_event_tx: mpsc::Sender<ServerEvent>,
    cancel_token: CancellationToken,
) {
    let mut reader: Reader = read_stream.into();

    loop {
        match reader.read_multiple::<Response>().await {
            Ok(responses) => {
                for response in responses {
                    match response.request_id() {
                        Some(id) => request_pool.complete_request(id, response).await,
                        None => {
                            if let Err(err) = server_event_tx.send(response.into()).await {
                                println!("Error sending response to output channel: {}", err);
                            }
                        }
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::ConnectionAborted => {
                println!("Server disconnected");
                if let Err(err) = server_event_tx.send(ServerEvent::ServerDisconnected).await {
                    println!("Error sending server disconnected event: {}", err);
                }
                cancel_token.cancel();
                break;
            }
            Err(err) => {
                println!("Error reading from server: {}", err);
            }
        }
    }
}

async fn handle_write(
    write_stream: WriteHalf<UnixStream>,
    request_rx: Arc<Mutex<mpsc::Receiver<Request>>>,
    cancel_token: CancellationToken,
) {
    let mut writer: Writer = write_stream.into();
    loop {
        let mut request_rx = request_rx.lock().await;
        tokio::select! {
            Some(request) = request_rx.recv() => {
                if let Err(err) = writer.write(&request).await {
                    println!("Error writing to server: {}", err);
                }
            }
            _ = cancel_token.cancelled() => {
                println!("The read loop has been cancelled, exiting the write loop");
                break;
            }
            else => {
                println!("Error reading command from channel");
            }
        }
    }
}
