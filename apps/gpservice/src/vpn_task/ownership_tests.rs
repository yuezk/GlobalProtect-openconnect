use std::{
  io::{Read, Write},
  sync::Arc,
  time::Duration,
};

use gpapi::{
  gateway::Gateway,
  gp_params::GpParams,
  service::{
    transport::ServiceResult,
    vpn_state::{ConnectInfo, VpnState},
  },
  session::{ClientAddresses, GatewayAuthentication, GatewaySession, GatewaySessions, transport::GatewayTransport},
};
use openssl::{
  asn1::Asn1Time,
  hash::MessageDigest,
  pkey::PKey,
  rsa::Rsa,
  ssl::{SslAcceptor, SslMethod},
  x509::{X509, X509NameBuilder},
};
use tokio::sync::{oneshot, watch};

use super::{LifecycleEvent, PreparedConnection, VpnTask, test_connect_request};

fn tls_acceptor() -> SslAcceptor {
  let key = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
  let mut name = X509NameBuilder::new().unwrap();
  name.append_entry_by_text("CN", "localhost").unwrap();
  let name = name.build();
  let mut cert = X509::builder().unwrap();
  cert.set_version(2).unwrap();
  cert.set_subject_name(&name).unwrap();
  cert.set_issuer_name(&name).unwrap();
  cert.set_pubkey(&key).unwrap();
  cert.set_not_before(&Asn1Time::days_from_now(0).unwrap()).unwrap();
  cert.set_not_after(&Asn1Time::days_from_now(1).unwrap()).unwrap();
  cert.sign(&key, MessageDigest::sha256()).unwrap();
  let mut acceptor = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
  acceptor.set_private_key(&key).unwrap();
  acceptor.set_certificate(&cert.build()).unwrap();
  acceptor.check_private_key().unwrap();
  acceptor.build()
}

/// Exercise the real HTTPS logout transport without any production network,
/// host collectors, or user identity files. The response gate proves admission
/// stays owned until the server request finishes.
#[tokio::test]
async fn cancelled_pending_owner_logs_out_and_releases_admission_after_response() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let endpoint = listener.local_addr().unwrap();
  let server_name = format!("https://{endpoint}");
  let mut gateway = serde_json::to_value(Gateway::new("fixture".into(), server_name.clone())).unwrap();
  gateway["kind"] = serde_json::json!("internal");
  let gateway: Gateway = serde_json::from_value(gateway).unwrap();
  let registry = Arc::new(crate::session_registry::SessionRegistry::new(uuid::Uuid::new_v4()));
  let credential = registry.issue("fixture", None).unwrap();
  let session_id = credential.session_id();
  let permit = registry
    .begin_handshake(credential.service_instance_id(), session_id, "fixture")
    .unwrap();
  let (tx, _rx) = tokio::sync::mpsc::channel(4);
  let control = crate::ws_connection::ConnectionControl::new(uuid::Uuid::new_v4(), tx).0;
  let connection_id = control.connection_id();
  registry.commit_attach(&permit, control).unwrap();
  let fixture = test_connect_request(ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![]));
  let mut member = fixture.plan().members()[0].clone();
  member.authentication =
    GatewayAuthentication::new("authcookie=fixture&user=fixture".into(), "internal".into()).unwrap();
  let plan = gpapi::session::ConnectionPlan::new(vec![member], vec![], fixture.plan().policy()).unwrap();
  let request = gpapi::service::request::ConnectRequest::new(fixture.info().clone(), plan);
  let mut prepared = PreparedConnection::lifecycle_fixture(request);
  let mut params = GpParams::builder(prepared.resources.profile.clone());
  params.ignore_tls_errors(true);
  prepared.sessions = GatewaySessions::new(vec![GatewaySession {
    gateway,
    transport: GatewayTransport::new(
      &server_name,
      prepared.request.plan().members()[0].authentication.clone(),
      &params.build(),
      prepared.request.plan().members()[0].binding.as_ref(),
    )
    .unwrap(),
    addresses: ClientAddresses::default(),
  }]);
  let acceptor = tls_acceptor();
  let (arrived, arrival) = oneshot::channel();
  let (release, response_gate) = oneshot::channel();
  let server = tokio::spawn(async move {
    let (socket, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept())
      .await
      .unwrap()
      .unwrap();
    let socket = socket.into_std().unwrap();
    socket.set_nonblocking(false).unwrap();
    socket.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    let request = tokio::task::spawn_blocking(move || {
      let mut stream = acceptor.accept(socket).unwrap();
      let mut bytes = Vec::new();
      let (headers_end, body_len) = loop {
        let mut chunk = [0; 1024];
        let count = stream.read(&mut chunk).unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&chunk[..count]);
        if let Some(position) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
          let headers = std::str::from_utf8(&bytes[..position]).unwrap();
          let length = headers
            .lines()
            .find_map(|line| {
              let (key, value) = line.split_once(':')?;
              key
                .eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap();
          break (position + 4, length);
        }
      };
      while bytes.len() < headers_end + body_len {
        let mut chunk = [0; 1024];
        let count = stream.read(&mut chunk).unwrap();
        assert_ne!(count, 0);
        bytes.extend_from_slice(&chunk[..count]);
      }
      arrived.send(()).unwrap();
      response_gate.blocking_recv().unwrap();
      let body = "<response status=\"success\"/>";
      write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
      )
      .unwrap();
      stream.flush().unwrap();
      String::from_utf8(bytes).unwrap()
    })
    .await
    .unwrap();
    // A single accepted authentication must produce a single logout.
    assert!(
      tokio::time::timeout(Duration::from_millis(100), listener.accept())
        .await
        .is_err()
    );
    request
  });
  let (state, state_rx) = watch::channel(VpnState::Disconnected);
  let (mut task, lifecycle) = VpnTask::new(state, false, registry.clone());
  let reservation = registry
    .reserve_connection(session_id, 1, true, || lifecycle.reserve_connect())
    .unwrap();
  assert_eq!(
    registry.commit_connection(session_id, 1, true, || reservation.commit(
      prepared,
      None,
      None,
      Some(session_id)
    )),
    ServiceResult::Accepted
  );
  let drain = registry
    .stop_non_tunnel_attachment(session_id, 1, connection_id)
    .unwrap()
    .unwrap();
  let drain_waiter = tokio::spawn(async move {
    drain.wait().await;
  });
  let start = task.events.recv().await.unwrap();
  assert!(matches!(&start, LifecycleEvent::Start { .. }));
  task.handle_event(start).await;
  tokio::time::timeout(Duration::from_secs(3), arrival)
    .await
    .unwrap()
    .unwrap();
  assert!(lifecycle.has_attempt(0));
  assert!(!drain_waiter.is_finished());
  assert!(lifecycle.reserve_connect().is_err());
  assert!(!task.active.as_ref().unwrap().task.is_finished());
  assert!(matches!(*state_rx.borrow(), VpnState::Disconnected));
  release.send(()).unwrap();
  while task.active.is_some() {
    let event = tokio::time::timeout(Duration::from_secs(3), task.events.recv())
      .await
      .unwrap()
      .unwrap();
    task.handle_event(event).await;
  }
  assert!(!lifecycle.has_attempt(0));
  drain_waiter.await.unwrap();
  assert!(lifecycle.reserve_connect().is_ok());
  assert!(matches!(*state_rx.borrow(), VpnState::Disconnected));
  let request = server.await.unwrap();
  assert!(request.starts_with("POST /ssl-vpn/logout.esp "));
  let form = request.split_once("\r\n\r\n").unwrap().1;
  let fields = url_form(form);
  assert_eq!(fields.get("user").map(String::as_str), Some("fixture"));
  assert_eq!(fields.get("computer").map(String::as_str), Some("fixture"));
  assert_eq!(fields.get("clientos").map(String::as_str), Some("Linux"));
}

fn url_form(form: &str) -> std::collections::HashMap<String, String> {
  form
    .split('&')
    .map(|field| {
      let (key, value) = field.split_once('=').unwrap();
      (key.to_owned(), value.to_owned())
    })
    .collect()
}
