use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

#[derive(Debug)]
pub struct DuplexStreamHandle {
    stream: DuplexStream,
    buf_size: usize,
}

impl DuplexStreamHandle {
    fn new(stream: DuplexStream, buf_size: usize) -> Self {
        Self { stream, buf_size }
    }

    pub async fn write(&mut self, data: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.stream.write_all(data.as_bytes()).await?;
        Ok(())
    }

    pub async fn read(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        let mut buffer = vec![0; self.buf_size];
        match self.stream.read(&mut buffer).await {
            Ok(0) => Err("EOF".into()),
            Ok(n) => Ok(String::from_utf8_lossy(&buffer[..n]).to_string()),
            Err(err) => Err(err.to_string().into()),
        }
    }
}

pub(crate) fn duplex(max_buf_size: usize) -> (DuplexStreamHandle, DuplexStreamHandle) {
    let (a, b) = tokio::io::duplex(max_buf_size);
    (
        DuplexStreamHandle::new(a, max_buf_size),
        DuplexStreamHandle::new(b, max_buf_size),
    )
}