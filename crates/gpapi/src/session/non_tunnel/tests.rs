use std::{
  future::pending,
  sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
  },
};

use tokio::sync::Notify;

use super::*;
use crate::session::transport::GatewayTransport;
use crate::session::{
  ClientAddresses, GatewaySessions,
  transport::tests::{gateway, response},
};

async fn run_owned_sessions(
  members: Vec<GatewaySession>,
  policy: InternalSessionPolicy,
  callbacks: SessionCallbacks,
  cancellation: CancellationToken,
  network_change: impl Future<Output = SessionEndReason>,
) -> anyhow::Result<SessionExit> {
  let mut sessions = GatewaySessions::new(members);
  let result =
    maintain_non_tunnel_sessions(&mut sessions, policy, callbacks, cancellation.clone(), network_change).await;
  sessions.logout().await;
  if cancellation.is_cancelled() {
    Ok(SessionExit::Disconnected)
  } else {
    result
  }
}

fn member(name: &str, transport: GatewayTransport) -> GatewaySession {
  GatewaySession {
    gateway: Gateway::new(name.into(), format!("{name}.example")),
    transport,
    addresses: ClientAddresses {
      ipv4: Some("192.0.2.7".parse().unwrap()),
      ipv6: None,
    },
  }
}

fn callbacks() -> SessionCallbacks {
  SessionCallbacks {
    produce_report: Arc::new(|_, _| Ok("<hip-report/>".into())),
    report_submitted: Arc::new(|_, _| {}),
    status_changed: Arc::new(|_| {}),
    validate_network: Arc::new(|_| Box::pin(async { Ok(()) })),
  }
}

#[tokio::test]
async fn network_uncertainty_prevents_hip_and_healthy_status() {
  let (transport, server) = gateway(vec![response("<response status='success'/>")]).await;
  let mut callbacks = callbacks();
  callbacks.validate_network = Arc::new(|_| Box::pin(async { anyhow::bail!("Network changed") }));
  callbacks.status_changed = Arc::new(|summary| assert_ne!(summary.maintenance, MaintenanceState::Healthy));
  let exit = run_owned_sessions(
    vec![member("internal", transport)],
    InternalSessionPolicy::default(),
    callbacks,
    CancellationToken::new(),
    pending(),
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::NetworkChanged));
  assert_eq!(server.await.unwrap()[0].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn revoked_report_authorization_terminates_the_attempt() {
  let (transport, server) = gateway(vec![
    response("<response><hip-report-needed>yes</hip-report-needed></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let mut callbacks = callbacks();
  callbacks.produce_report = Arc::new(|_, _| Err(io::ErrorKind::PermissionDenied.into()));
  let exit = run_owned_sessions(
    vec![member("internal", transport)],
    InternalSessionPolicy::default(),
    callbacks,
    CancellationToken::new(),
    pending(),
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::AuthorizationRevoked));
  assert_eq!(server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn invalid_cookie_terminates_only_after_logout() {
  let (transport, server) = gateway(vec![
    response("<response status='error'><error>Invalid authentication cookie</error></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let exit = run_owned_sessions(
    vec![member("internal", transport)],
    InternalSessionPolicy::default(),
    callbacks(),
    CancellationToken::new(),
    pending(),
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::InvalidCookie));
  let requests = server.await.unwrap();
  assert_eq!(requests.len(), 2);
  assert_eq!(requests[1].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn rejection_preserves_healthy_members_and_cleanup_owns_all_members() {
  let (rejected, rejected_server) = gateway(vec![
    response("<response status='error'><error>Denied</error></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let (healthy, healthy_server) = gateway(vec![
    response("<response><hip-report-needed>no</hip-report-needed></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let ready = Arc::new(Notify::new());
  let states = Arc::new(Mutex::new(Vec::new()));
  let mut callbacks = callbacks();
  let states_copy = states.clone();
  let ready_copy = ready.clone();
  callbacks.status_changed = Arc::new(move |summary| {
    if summary.maintenance != MaintenanceState::Authenticated {
      let mut states = states_copy.lock().unwrap();
      states.push((summary.gateway.name().to_owned(), summary.maintenance));
      if states.len() == 2 {
        ready_copy.notify_one();
      }
    }
  });
  let exit = run_owned_sessions(
    vec![member("rejected", rejected), member("healthy", healthy)],
    InternalSessionPolicy::default(),
    callbacks,
    CancellationToken::new(),
    async {
      ready.notified().await;
      SessionEndReason::NetworkChanged
    },
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::NetworkChanged));
  let states = states.lock().unwrap();
  assert!(states.contains(&("rejected".into(), MaintenanceState::Rejected)));
  assert!(states.contains(&("healthy".into(), MaintenanceState::Healthy)));
  assert_eq!(rejected_server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
  assert_eq!(healthy_server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn observes_reports_only_after_successful_submission() {
  let (transport, server) = gateway(vec![
    response("<response><hip-report-needed>yes</hip-report-needed></response>"),
    response("<response status='success'/>"),
    response("<response status='success'/>"),
  ])
  .await;
  let cancellation = CancellationToken::new();
  let submitted = Arc::new(AtomicUsize::new(0));
  let observed = submitted.clone();
  let stop = cancellation.clone();
  let mut callbacks = callbacks();
  callbacks.report_submitted = Arc::new(move |request, report| {
    assert_eq!(request.client_ip.as_deref(), Some("192.0.2.7"));
    assert_eq!(report, "<hip-report/>");
    observed.fetch_add(1, Ordering::SeqCst);
    stop.cancel();
  });
  let exit = run_owned_sessions(
    vec![member("internal", transport)],
    InternalSessionPolicy::default(),
    callbacks,
    cancellation,
    pending(),
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Disconnected);
  assert_eq!(submitted.load(Ordering::SeqCst), 1);
  assert_eq!(server.await.unwrap().len(), 3);
}

#[tokio::test]
async fn cancelled_collection_is_joined_before_runtime_returns() {
  let (transport, server) = gateway(vec![
    response("<response><hip-report-needed>yes</hip-report-needed></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let cancellation = CancellationToken::new();
  let entered = Arc::new(Notify::new());
  let stopped = Arc::new(AtomicBool::new(false));
  let mut callbacks = callbacks();
  let entered_copy = entered.clone();
  let stopped_copy = stopped.clone();
  callbacks.produce_report = Arc::new(move |_, control| {
    entered_copy.notify_one();
    loop {
      if let Err(error) = control.check() {
        stopped_copy.store(true, Ordering::SeqCst);
        return Err(error);
      }
      std::thread::sleep(Duration::from_millis(10));
    }
  });
  callbacks.report_submitted = Arc::new(|_, _| panic!("Cancelled report must not be observed"));
  let stop = cancellation.clone();
  let exit = tokio::time::timeout(
    Duration::from_secs(2),
    run_owned_sessions(
      vec![member("internal", transport)],
      InternalSessionPolicy::default(),
      callbacks,
      cancellation,
      async {
        entered.notified().await;
        stop.cancel();
        pending().await
      },
    ),
  )
  .await
  .unwrap()
  .unwrap();
  assert_eq!(exit, SessionExit::Disconnected);
  assert!(stopped.load(Ordering::SeqCst));
  assert_eq!(server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn all_rejected_members_are_logged_out_before_terminal_failure() {
  let (transport, server) = gateway(vec![
    response("<response status='error'><error>Denied</error></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let mut callbacks = callbacks();
  callbacks.produce_report = Arc::new(|_, _| panic!("Rejected member cannot collect a report"));
  let exit = run_owned_sessions(
    vec![member("rejected", transport)],
    InternalSessionPolicy::default(),
    callbacks,
    CancellationToken::new(),
    pending(),
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::AllMembersFailed));
  assert_eq!(server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn invalid_member_cookie_cancels_and_joins_another_members_collector() {
  let arrived = Arc::new(Notify::new());
  let release = Arc::new(Notify::new());
  let collecting = Arc::new(Notify::new());
  let stopped = Arc::new(AtomicBool::new(false));
  let (expired, expired_server) = crate::session::transport::tests::gateway_with_first_response_gate(
    vec![
      response("<response status='error'><error>Invalid authentication cookie</error></response>"),
      response("<response status='success'/>"),
    ],
    arrived.clone(),
    release.clone(),
  )
  .await;
  let (healthy, healthy_server) = gateway(vec![
    response("<response><hip-report-needed>yes</hip-report-needed></response>"),
    response("<response status='success'/>"),
  ])
  .await;
  let mut callbacks = callbacks();
  let collecting_copy = collecting.clone();
  let stopped_copy = stopped.clone();
  callbacks.produce_report = Arc::new(move |request, control| {
    assert_eq!(request.gateway.name(), "collecting");
    collecting_copy.notify_one();
    loop {
      if let Err(error) = control.check() {
        stopped_copy.store(true, Ordering::SeqCst);
        return Err(error);
      }
      std::thread::sleep(Duration::from_millis(10));
    }
  });
  callbacks.report_submitted = Arc::new(|_, _| panic!("Interrupted collection must not submit"));
  let exit = tokio::time::timeout(
    Duration::from_secs(2),
    run_owned_sessions(
      vec![member("expired", expired), member("collecting", healthy)],
      InternalSessionPolicy::default(),
      callbacks,
      CancellationToken::new(),
      async {
        arrived.notified().await;
        collecting.notified().await;
        release.notify_one();
        pending().await
      },
    ),
  )
  .await
  .unwrap()
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::InvalidCookie));
  assert!(stopped.load(Ordering::SeqCst));
  assert_eq!(expired_server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
  assert_eq!(healthy_server.await.unwrap()[1].0, "/ssl-vpn/logout.esp");
}

#[tokio::test]
async fn failed_submission_is_terminal_without_retry_or_observation() {
  let (transport, server) = gateway(vec![
    response("<response><hip-report-needed>yes</hip-report-needed></response>"),
    "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned(),
    response("<response status='success'/>"),
  ])
  .await;
  let mut callbacks = callbacks();
  callbacks.report_submitted = Arc::new(|_, _| panic!("Failed report must not be retained"));
  let exit = run_owned_sessions(
    vec![member("internal", transport)],
    InternalSessionPolicy::default(),
    callbacks,
    CancellationToken::new(),
    pending(),
  )
  .await
  .unwrap();
  assert_eq!(exit, SessionExit::Failed(SessionEndReason::AllMembersFailed));
  let requests = server.await.unwrap();
  assert_eq!(
    requests.iter().map(|request| request.0.as_str()).collect::<Vec<_>>(),
    [
      "/ssl-vpn/hipreportcheck.esp",
      "/ssl-vpn/hipreport.esp",
      "/ssl-vpn/logout.esp"
    ]
  );
}

#[tokio::test]
async fn schedules_from_send_time_plus_interval_and_server_delay() {
  let arrived = Arc::new(Notify::new());
  let release = Arc::new(Notify::new());
  let (transport, server) = crate::session::transport::tests::gateway_with_first_response_gate(
    vec![
      response("<response><hip-report-needed>no</hip-report-needed><delay>7</delay></response>"),
      response("<response><hip-report-needed>no</hip-report-needed></response>"),
      response("<response status='success'/>"),
    ],
    arrived.clone(),
    release.clone(),
  )
  .await;
  let cancellation = CancellationToken::new();
  let stop = cancellation.clone();
  let (sent, mut received) = tokio::sync::mpsc::unbounded_channel();
  let mut callbacks = callbacks();
  callbacks.status_changed = Arc::new(move |summary| {
    if summary.maintenance == MaintenanceState::Healthy {
      sent.send(tokio::time::Instant::now()).unwrap();
    }
  });
  let runtime = tokio::spawn(run_owned_sessions(
    vec![member("internal", transport)],
    InternalSessionPolicy::new(10, false).unwrap(),
    callbacks,
    cancellation,
    pending(),
  ));
  arrived.notified().await;
  // Simulate three seconds spent waiting for the first response. The interval
  // must be anchored before that latency, rather than after completion.
  tokio::time::pause();
  tokio::time::advance(Duration::from_secs(3)).await;
  tokio::time::resume();
  release.notify_one();
  let first = received.recv().await.unwrap();
  tokio::time::pause();
  tokio::time::advance(Duration::from_secs(13)).await;
  tokio::time::resume();
  tokio::time::sleep(Duration::from_millis(20)).await;
  assert!(received.try_recv().is_err(), "Server delay must extend the interval");
  tokio::time::pause();
  tokio::time::advance(Duration::from_secs(1)).await;
  tokio::time::resume();
  let second = tokio::time::timeout(Duration::from_millis(500), received.recv())
    .await
    .unwrap()
    .unwrap();
  let elapsed = second.duration_since(first);
  assert!(
    elapsed >= Duration::from_millis(13_900) && elapsed < Duration::from_millis(14_500),
    "Unexpected anchored interval: {elapsed:?}"
  );
  stop.cancel();
  assert_eq!(runtime.await.unwrap().unwrap(), SessionExit::Disconnected);
  assert_eq!(server.await.unwrap().len(), 3);
}
