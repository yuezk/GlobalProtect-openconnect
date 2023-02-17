use serde::Serialize;
use tokio::io::{self, AsyncWriteExt, WriteHalf};
use tokio::net::UnixStream;

pub(crate) struct Writer {
    stream: WriteHalf<UnixStream>,
}

impl From<WriteHalf<UnixStream>> for Writer {
    fn from(stream: WriteHalf<UnixStream>) -> Self {
        Self { stream }
    }
}

impl Writer {
    pub async fn write<T: Serialize>(&mut self, data: &T) -> Result<(), io::Error> {
        let data = serde_json::to_string(data)?;
        let data = format!("{}\n\n", data);

        self.stream.write_all(data.as_bytes()).await?;
        self.stream.flush().await?;
        Ok(())
    }
}
