use common::{Client, SOCKET_PATH};
use tokio::{io::AsyncReadExt, net::UnixStream, sync::mpsc};

#[tokio::main]
async fn main() {
    // let mut stream = UnixStream::connect(SOCKET_PATH).await.unwrap();

    // let mut buf = [0u8; 34];
    // let _ = stream.read(&mut buf).await.unwrap();

    // // The first two bytes are the port number, the rest is the AES key
    // let http_port = u16::from_be_bytes([buf[0], buf[1]]);
    // let aes_key = &buf[2..];

    // println!("http_port: {http_port}");
    // println!("aes_key: {aes_key:?}");
    let (output_tx, mut output_rx) = mpsc::channel::<String>(32);
    let client = Client::default();

    tokio::select! {
        _ = client.run() => {
            println!("Client finished");
        }
    }
}
