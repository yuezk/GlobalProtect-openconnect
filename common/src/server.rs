use crate::{connection::handle_connection, vpn::Vpn};
use log::{warn, info};
use std::{future::Future, os::unix::prelude::PermissionsExt, path::Path, sync::Arc};
use tokio::fs;
use tokio::net::{UnixListener, UnixStream};

#[derive(Debug, Default)]
pub(crate) struct ServerContext {
    vpn: Arc<Vpn>,
}

struct Server {
    socket_path: String,
    context: Arc<ServerContext>,
}

impl ServerContext {
    pub fn vpn(&self) -> Arc<Vpn> {
        self.vpn.clone()
    }
}

impl Server {
    fn new(socket_path: String) -> Self {
        Self {
            socket_path,
            context: Default::default(),
        }
    }

    // Check if an instance of the server is already running.
    // by trying to connect to the socket.
    async fn is_running(&self) -> bool {
        UnixStream::connect(&self.socket_path).await.is_ok()
    }

    async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        if Path::new(&self.socket_path).exists() {
            fs::remove_file(&self.socket_path).await?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        info!("Listening on socket: {:?}", listener.local_addr()?);

        let metadata = fs::metadata(&self.socket_path).await?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o666);
        fs::set_permissions(&self.socket_path, permissions).await?;

        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    info!("Accepted connection: {:?}", socket.peer_addr()?);
                    tokio::spawn(handle_connection(socket, self.context.clone()));
                }
                Err(err) => {
                    warn!("Error accepting connection: {:?}", err);
                }
            }
        }
    }

    async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.context.vpn().disconnect().await;
        fs::remove_file(&self.socket_path).await?;
        Ok(())
    }
}

pub async fn run(
    socket_path: &str,
    shutdown: impl Future,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = Server::new(socket_path.to_string());

    if server.is_running().await {
        return Err("Another instance of the server is already running".into());
    }

    tokio::select! {
        res = server.start() => {
            res?
        },
        _ = shutdown => {
            info!("Shutting down the server...");
            server.stop().await?;
        },
    }

    Ok(())
}
