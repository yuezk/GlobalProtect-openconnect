use crate::{connection::handle_connection, vpn::Vpn};
use std::{future::Future, os::unix::prelude::PermissionsExt, path::Path, sync::Arc};
use tokio::{fs, net::UnixListener};

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

    async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
        if Path::new(&self.socket_path).exists() {
            fs::remove_file(&self.socket_path).await?;
        }

        let listener = UnixListener::bind(&self.socket_path)?;
        println!("Listening on socket: {:?}", listener.local_addr()?);

        let metadata = fs::metadata(&self.socket_path).await?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o666);
        fs::set_permissions(&self.socket_path, permissions).await?;

        loop {
            match listener.accept().await {
                Ok((socket, _)) => {
                    println!("Accepted connection: {:?}", socket.peer_addr()?);
                    tokio::spawn(handle_connection(socket, self.context.clone()));
                }
                Err(err) => {
                    println!("Error accepting connection: {:?}", err);
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

    tokio::select! {
        res = server.start() => {
            if let Err(err) = res {
                println!("Error starting server: {:?}", err);
            }
        },
        _ = shutdown => {
            println!("Shutting down");
            server.stop().await?;
        },
    }

    Ok(())
}
