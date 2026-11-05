use gpapi::{
  credential::{Credential, PasswordCredential},
  error::AuthenticationRejected,
  gateway::GatewayLoginClient,
  gp_params::GpParams,
  os_profile::{ClientOs, HostIdentity, HostSnapshot, OsProfile},
  portal,
};
use tokio::{
  io::{AsyncReadExt, AsyncWriteExt},
  net::TcpListener,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn explicit_rejection_is_typed_for_ios_without_changing_desktop_errors() {
  for os in [ClientOs::Ios, ClientOs::Linux, ClientOs::Mac, ClientOs::Windows] {
    for status in [401, 403, 503] {
      for gateway in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
          let (mut socket, _) = listener.accept().await.unwrap();
          let mut request = Vec::new();
          loop {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).await.unwrap();
            if count == 0 {
              break;
            }
            request.extend_from_slice(&chunk[..count]);
            if let Some(end) = request.windows(4).position(|v| v == b"\r\n\r\n") {
              let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
              let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:").and_then(|s| s.trim().parse().ok()))
                .unwrap_or(0);
              if request.len() >= end + 4 + length {
                break;
              }
            }
          }
          socket
            .write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes())
            .await
            .unwrap();
        });
        let profile = OsProfile::builder(os).build(HostSnapshot {
          runtime_os: os,
          identity: HostIdentity::application("fixture".into(), "fixture-id".into()),
          os_version: "fixture".into(),
          software_version: String::new(),
        });
        let params = GpParams::builder(profile).build();
        let credential = Credential::Password(PasswordCredential::new("fixture", "fixture-password"));
        let token = CancellationToken::new();
        let error = if gateway {
          GatewayLoginClient::new(&origin, params)
            .unwrap()
            .login(&credential, None, &token)
            .await
            .err()
            .unwrap()
        } else {
          portal::retrieve_config(&origin, &credential, &params, &token)
            .await
            .err()
            .unwrap()
        };
        server.await.unwrap();
        assert_eq!(
          error.is::<AuthenticationRejected>(),
          os == ClientOs::Ios && status != 503,
          "os={os:?}, status={status}, gateway={gateway}"
        );
        if os != ClientOs::Ios {
          let prefix = if gateway {
            "Gateway login error:"
          } else {
            "Portal config error:"
          };
          assert!(error.to_string().starts_with(prefix));
        }
      }
    }
  }
}
