use data_encoding::HEXUPPER;
use ring::digest::{Context, SHA256};
use std::{
    fs::File,
    io::{BufReader, Read},
    path::Path,
};

pub const SOCKET_PATH: &str = "/tmp/gpservice.sock";

mod client;
mod cmd;
mod connection;
mod reader;
mod request;
mod response;
pub mod server;
mod vpn;
mod writer;

pub(crate) use request::Request;
pub(crate) use request::RequestPool;

pub use response::Response;
pub use response::ResponseData;
pub use response::TryFromResponseDataError;

pub(crate) use reader::Reader;
pub(crate) use writer::Writer;

pub use client::Client;
pub use client::ServerApiError;
pub use client::ClientStatus;
pub use vpn::VpnStatus;

pub fn sha256_digest<P: AsRef<Path>>(file_path: P) -> Result<String, std::io::Error> {
    let input = File::open(file_path)?;
    let mut reader = BufReader::new(input);

    let mut context = Context::new(&SHA256);
    let mut buffer = [0; 1024];

    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        context.update(&buffer[..count]);
    }
    Ok(HEXUPPER.encode(context.finish().as_ref()))
}
