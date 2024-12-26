use std::io::Cursor;

use log::info;
use tiny_http::{Header, Response, Server};
use uuid::Uuid;

pub(super) struct AuthServer {
  server: Server,
  auth_id: String,
}

impl AuthServer {
  pub fn new() -> anyhow::Result<Self> {
    let server = Server::http("127.0.0.1:0").map_err(|err| anyhow::anyhow!(err))?;
    let auth_id = Uuid::new_v4().to_string();

    Ok(Self { server, auth_id })
  }

  pub fn auth_url(&self) -> String {
    format!("http://{}/{}", self.server.server_addr(), self.auth_id)
  }

  pub fn serve_request(&self, auth_request: &str) {
    info!("auth server started at: {}", self.auth_url());

    for req in self.server.incoming_requests() {
      info!("received request, method: {}, url: {}", req.method(), req.url());

      if req.url() != format!("/{}", self.auth_id) {
        let forbidden = Response::from_string("forbidden").with_status_code(403);
        let _ = req.respond(forbidden);
      } else {
        let auth_response = build_auth_response(auth_request);
        if let Err(err) = req.respond(auth_response) {
          info!("failed to respond to request: {}", err);
        } else {
          info!("stop the auth server");
          break;
        }
      }
    }
  }
}

fn build_auth_response(auth_request: &str) -> Response<Cursor<Vec<u8>>> {
  if auth_request.starts_with("http") {
    let header = format!("location: {}", auth_request);
    let header: Header = header.parse().unwrap();
    Response::from_string("redirect")
      .with_status_code(302)
      .with_header(header)
  } else {
    let content_type: Header = "content-type: text/html".parse().unwrap();
    Response::from_string(auth_request).with_header(content_type)
  }
}
