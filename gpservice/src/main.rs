include!(concat!(env!("OUT_DIR"), "/client_hash.rs"));

use gpcommon::{server, SOCKET_PATH};
use log::error;
use std::fs::File;
use tokio::signal;

// static mut HTTP_PORT: u16 = 0;
// static mut AES_KEY: [u8; 32] = [0; 32];

// async fn start_unix_server() {
//     println!("Starting unix server...");
//     // remove old socket file
//     match std::fs::remove_file(SOCKET_PATH) {
//         Ok(_) => println!("Removed old socket file"),
//         Err(err) if err.kind() != std::io::ErrorKind::NotFound => {
//             println!("Error: {err}");
//             return;
//         }
//         Err(_) => (),
//     }

//     let listener = UnixListener::bind(SOCKET_PATH).unwrap();

//     // set the socket file permissions to 666
//     let Ok(metadata) = fs::metadata(SOCKET_PATH) else {
//         return;
//     };

//     let mut permissions = metadata.permissions();
//     permissions.set_mode(0o666);
//     fs::set_permissions(SOCKET_PATH, permissions).unwrap();

//     loop {
//         let (stream, _) = listener.accept().await.unwrap();
//         tokio::spawn(handle_unix_client(stream));
//     }
// }

// async fn handle_unix_client(mut stream: UnixStream) {
//     if !is_validate_client(&mut stream).await {
//         println!("Invalid client");
//         stream.shutdown().await.unwrap();
//         return;
//     }

// Read

// let mut message: [u8; 34] = [0; 34];
// unsafe {
//     message[0..2].copy_from_slice(&HTTP_PORT.to_be_bytes());
//     message[2..34].copy_from_slice(&AES_KEY[..]);
// }
// stream.write_all(&message[..]).await.unwrap();
// }

// async fn is_validate_client(stream: &mut UnixStream) -> bool {
//     let Ok(ucred) = stream.peer_cred() else {
//         return false;
//     };

//     if let Some(pid) = ucred.pid() {
//         let Ok(proc) = Process::new(pid) else {
//             return false;
//         };

//         let Ok(exe) = proc.exe() else {
//             return false;
//         };

//         let Ok(exe_hash) = sha256_digest(exe) else {
//             return false;
//         };
//         return exe_hash == GPCLIENT_HASH;
//     }

//     false
// }

// async fn start_http_server() {
//     println!("Starting http server...");
//     // Match any request and return hello world!
//     let routes = warp::any().map(|| "Hello, World!");
//     let (addr, server) =
//         warp::serve(routes).bind_with_graceful_shutdown(([127, 0, 0, 1], 0), async {
//             tokio::signal::ctrl_c().await.unwrap();
//         });

//     unsafe {
//         HTTP_PORT = addr.port();
//     }

//     println!("Listening on http://{addr}/");
//     tokio::spawn(server).await.unwrap();
//     println!("Shutting down http server");
// }

const LOG_FILE: &str = "/var/log/gpservice.log";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // println!("{GPCLIENT_HASH}");

    // unsafe {
    //     let aes_key = Aes256Gcm::generate_key(&mut OsRng);
    //     AES_KEY = aes_key.as_slice().try_into().unwrap();
    // }
    // tokio::spawn(start_unix_server());
    // start_http_server().await;
    // server::start().await

    let log_file = File::create(LOG_FILE)?;
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{} {} {}] {}",
                humantime::format_rfc3339_millis(std::time::SystemTime::now()),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(log::LevelFilter::Info)
        .chain(std::io::stdout())
        .chain(log_file)
        .apply()?;

    if let Err(err) = server::run(SOCKET_PATH, signal::ctrl_c()).await {
        error!("Error running server: {}", err);
    }
    Ok(())
}
