use serde::Deserialize;
use tokio::io::{self, AsyncReadExt, ReadHalf};
use tokio::net::UnixStream;

pub(crate) struct Reader {
    stream: ReadHalf<UnixStream>,
}

impl From<ReadHalf<UnixStream>> for Reader {
    fn from(stream: ReadHalf<UnixStream>) -> Self {
        Self { stream }
    }
}

impl Reader {
    pub async fn read_multiple<T: for<'a> Deserialize<'a>>(&mut self) -> Result<Vec<T>, io::Error> {
        let mut buffer = [0; 2048];

        match self.stream.read(&mut buffer).await {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Peer disconnected",
            )),
            Ok(bytes_read) => {
                let response_str = String::from_utf8_lossy(&buffer[..bytes_read]);
                let responses: Vec<&str> = response_str.split("\n\n").collect();
                let responses = responses
                    .iter()
                    .filter_map(|r| {
                        if !r.is_empty() {
                            serde_json::from_str(r).ok()
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<T>>();

                Ok(responses)
            }
            Err(err) => Err(err),
        }
    }
}
