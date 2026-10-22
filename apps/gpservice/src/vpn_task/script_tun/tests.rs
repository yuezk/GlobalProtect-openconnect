use super::*;
use openconnect::{VpnSessionInfo, tunnel::Configuration};
use tokio::io::AsyncWriteExt;

// A generic consumer, with no proxy settings or private worker protocol.
struct Consumer {
  control: UnixStream,
  native: mpsc::Sender<Native>,
  events: mpsc::Receiver<LifecycleEvent>,
  cancel: CancellationToken,
  task: tokio::task::JoinHandle<anyhow::Result<()>>,
  identity: Identity,
}
impl Consumer {
  fn new() -> Self {
    let (control, child) = std::os::unix::net::UnixStream::pair().unwrap();
    child.set_nonblocking(true).unwrap();
    let (native, observations) = mpsc::channel(64);
    let (events, notifications) = mpsc::channel(8);
    let identity = Identity {
      service_instance: uuid::Uuid::new_v4(),
      attempt: 7,
      launch_id: uuid::Uuid::new_v4(),
    };
    let cancel = CancellationToken::new();
    let task = tokio::spawn(supervise(
      identity,
      PreparedScriptTun {
        control,
        bootstrap: Bootstrap::new(b"consumer bootstrap").unwrap(),
      },
      observations,
      cancel.clone(),
      events,
    ));
    Self {
      control: UnixStream::from_std(child).unwrap(),
      native,
      events: notifications,
      cancel,
      task,
      identity,
    }
  }
  async fn message(&mut self) -> Message {
    tokio::time::timeout(Duration::from_secs(2), receive(&mut self.control))
      .await
      .unwrap()
      .unwrap()
  }
  async fn event(&mut self) -> LifecycleEvent {
    tokio::time::timeout(Duration::from_secs(2), self.events.recv())
      .await
      .unwrap()
      .unwrap()
  }
  async fn configure(&mut self) {
    self
      .native
      .send(Native::Event(Event::Configuration(Some(configuration()))))
      .await
      .unwrap();
    let Message::Configure {
      identity,
      network,
      bootstrap,
    } = self.message().await
    else {
      panic!("expected Configure")
    };
    assert_eq!(identity, self.identity);
    assert_eq!(network.addresses, vec!["10.0.0.2".parse::<std::net::IpAddr>().unwrap()]);
    assert_eq!(network.mtu, 1400);
    assert_eq!(bootstrap.decode().unwrap(), b"consumer bootstrap");
  }
  async fn activate(&mut self) {
    self.configure().await;
    self
      .native
      .send(Native::Established(VpnSessionInfo::default()))
      .await
      .unwrap();
    send_message(&mut self.control, &Message::Ready).await.unwrap();
    assert!(matches!(self.message().await, Message::Activate));
    send_message(&mut self.control, &Message::Activated).await.unwrap();
    assert!(matches!(
      self.event().await,
      LifecycleEvent::Established { attempt: 7, .. }
    ));
  }
  async fn finish(&mut self) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(2), &mut self.task)
      .await
      .unwrap()
      .unwrap()
  }
}
impl Drop for Consumer {
  fn drop(&mut self) {
    self.cancel.cancel();
    self.task.abort();
  }
}
fn configuration() -> Configuration {
  Configuration {
    ipv4: Some("10.0.0.2".into()),
    mtu: Some(1400),
    ..Default::default()
  }
}

#[tokio::test]
async fn connected_requires_native_establishment_and_consumer_acknowledgment() {
  let mut consumer = Consumer::new();
  consumer.configure().await;
  send_message(&mut consumer.control, &Message::Ready).await.unwrap();
  assert!(
    tokio::time::timeout(Duration::from_millis(30), receive(&mut consumer.control))
      .await
      .is_err()
  );
  assert!(consumer.events.try_recv().is_err());
  let session = VpnSessionInfo {
    lifetime_secs: Some(3600),
    ..Default::default()
  };
  consumer
    .native
    .send(Native::Established(session.clone()))
    .await
    .unwrap();
  assert!(matches!(consumer.message().await, Message::Activate));
  assert!(consumer.events.try_recv().is_err());
  send_message(&mut consumer.control, &Message::Activated).await.unwrap();
  let LifecycleEvent::Established {
    attempt,
    session: reported,
  } = consumer.event().await
  else {
    panic!("expected Established")
  };
  assert_eq!(attempt, consumer.identity.attempt);
  assert_eq!(reported, session);
  consumer.cancel.cancel();
  assert!(matches!(consumer.message().await, Message::Stop));
  consumer.finish().await.unwrap();
  assert!(receive(&mut consumer.control).await.is_err());
}

#[tokio::test]
async fn unchanged_reconnect_waits_for_suspension_before_reactivation() {
  let mut consumer = Consumer::new();
  consumer.activate().await;
  consumer.native.send(Native::Event(Event::Reconnecting)).await.unwrap();
  assert!(matches!(
    consumer.event().await,
    LifecycleEvent::ScriptTunSuspended { attempt: 7 }
  ));
  assert!(matches!(consumer.message().await, Message::Suspend));
  consumer
    .native
    .send(Native::Event(Event::Configuration(Some(configuration()))))
    .await
    .unwrap();
  consumer.native.send(Native::Event(Event::Reconnected)).await.unwrap();
  assert!(
    tokio::time::timeout(Duration::from_millis(30), receive(&mut consumer.control))
      .await
      .is_err()
  );
  send_message(&mut consumer.control, &Message::Suspended).await.unwrap();
  assert!(matches!(consumer.message().await, Message::Activate));
  assert!(consumer.events.try_recv().is_err());
  send_message(&mut consumer.control, &Message::Activated).await.unwrap();
  assert!(matches!(
    consumer.event().await,
    LifecycleEvent::Established { attempt: 7, .. }
  ));
  consumer.cancel.cancel();
  assert!(matches!(consumer.message().await, Message::Stop));
  consumer.finish().await.unwrap();
}

#[tokio::test]
async fn changed_reconnect_stops_consumer_and_returns_network_changed() {
  let mut consumer = Consumer::new();
  consumer.activate().await;
  consumer.native.send(Native::Event(Event::Reconnecting)).await.unwrap();
  assert!(matches!(
    consumer.event().await,
    LifecycleEvent::ScriptTunSuspended { .. }
  ));
  assert!(matches!(consumer.message().await, Message::Suspend));
  send_message(&mut consumer.control, &Message::Suspended).await.unwrap();
  let mut changed = configuration();
  changed.ipv4 = Some("10.0.0.3".into());
  consumer
    .native
    .send(Native::Event(Event::Configuration(Some(changed))))
    .await
    .unwrap();
  assert!(matches!(consumer.message().await, Message::Stop));
  assert!(consumer.finish().await.unwrap_err().is::<NetworkChanged>());
  assert!(
    !consumer.cancel.is_cancelled(),
    "restart must not look like explicit cancellation"
  );
  assert!(consumer.events.try_recv().is_err());
  assert!(receive(&mut consumer.control).await.is_err());
}

#[tokio::test]
async fn cancellation_during_activation_prevents_connected() {
  let mut consumer = Consumer::new();
  consumer.configure().await;
  consumer
    .native
    .send(Native::Established(VpnSessionInfo::default()))
    .await
    .unwrap();
  send_message(&mut consumer.control, &Message::Ready).await.unwrap();
  assert!(matches!(consumer.message().await, Message::Activate));
  consumer.cancel.cancel();
  // A late acknowledgment must not publish Connected after cancellation.
  let _ = send_message(&mut consumer.control, &Message::Activated).await;
  assert!(matches!(consumer.message().await, Message::Stop));
  consumer.finish().await.unwrap();
  assert!(consumer.events.try_recv().is_err());
}

#[tokio::test]
async fn consumer_eof_ends_startup_and_active_sessions() {
  for active in [false, true] {
    let mut consumer = Consumer::new();
    if active {
      consumer.activate().await;
    } else {
      consumer.configure().await;
    }
    consumer.control.shutdown().await.unwrap();
    assert!(consumer.finish().await.is_err());
    assert!(consumer.events.try_recv().is_err());
  }
}

#[tokio::test]
async fn unexpected_acknowledgment_fails_before_connected() {
  let mut consumer = Consumer::new();
  consumer.configure().await;
  send_message(&mut consumer.control, &Message::Activated).await.unwrap();
  assert!(matches!(consumer.message().await, Message::Stop));
  assert!(consumer.finish().await.unwrap_err().to_string().contains("Unexpected"));
  assert!(consumer.events.try_recv().is_err());
}

#[test]
fn prepares_worker_for_current_desktop_user_without_enumerating_groups() {
  let uid = unsafe { nix::libc::getuid() };
  if uid == 0 {
    return;
  }
  let request = ScriptTunRequest {
    launch_id: uuid::Uuid::new_v4(),
    program: "/usr/bin/true".into(),
    args: vec![],
    bootstrap: Bootstrap::default(),
  };
  builder("https://vpn.example", "test", &request, uid).unwrap();
}

#[tokio::test]
async fn reconnect_during_activation_returns_clear_error_without_connected() {
  let mut consumer = Consumer::new();
  consumer.configure().await;
  consumer
    .native
    .send(Native::Established(VpnSessionInfo::default()))
    .await
    .unwrap();
  send_message(&mut consumer.control, &Message::Ready).await.unwrap();
  assert!(matches!(consumer.message().await, Message::Activate));
  consumer.native.send(Native::Event(Event::Reconnecting)).await.unwrap();
  assert!(matches!(consumer.message().await, Message::Stop));
  assert!(
    consumer
      .finish()
      .await
      .unwrap_err()
      .to_string()
      .contains("during script-tun activation")
  );
  assert!(consumer.events.try_recv().is_err());
}
