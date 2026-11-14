//! Supervision of an unprivileged script-tun consumer; payload semantics belong to the caller.
use super::{LifecycleEvent, LifecycleHandle};
use anyhow::{Context, bail, ensure};
use gpapi::script_tun::{Bootstrap, Identity, Message, receive, send as send_message};
use openconnect::{Vpn, tunnel::Event};
use std::{sync::Arc, time::Duration};
use tokio::{sync::mpsc, time::Instant};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
#[path = "script_tun/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "script_tun/windows.rs"]
mod platform;
pub(super) use platform::{PreparedScriptTun, builder};

#[derive(Debug)]
pub(super) struct NetworkChanged;
impl std::fmt::Display for NetworkChanged {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_str("VPN network configuration changed")
  }
}
impl std::error::Error for NetworkChanged {}

enum Native {
  Event(Event),
  Established(openconnect::VpnSessionInfo),
}

pub(super) async fn run(
  identity: Identity,
  vpn: Arc<Vpn>,
  mut prepared: PreparedScriptTun,
  approval: &crate::hip_source::HipApproval,
  lifecycle: &LifecycleHandle,
  cancellation: CancellationToken,
  events: mpsc::Sender<LifecycleEvent>,
  session_id: uuid::Uuid,
  registry: Arc<crate::session_registry::SessionRegistry>,
  sessions: &mut gpapi::session::desktop::GatewaySessions,
) -> anyhow::Result<()> {
  let channel = prepared.connect().await?;
  let mut control = ConsumerControl::new(channel);
  let attempt = identity.attempt;
  let (native_tx, native) = mpsc::channel(64);
  let observed = native_tx.clone();
  let overflow = cancellation.clone();
  let monitor = gptunnel::TunnelMonitor::attach_with_observer(vpn.clone(), move |event| {
    if !matches!(event, Event::Statistics(_)) && observed.try_send(Native::Event(event.clone())).is_err() {
      overflow.cancel();
    }
  });
  let event_result = events
    .send(LifecycleEvent::TunnelMonitor {
      attempt,
      monitor: monitor.clone(),
    })
    .await;
  if event_result.is_err() || cancellation.is_cancelled() {
    monitor.stop().await;
    return Ok(());
  }
  let worker_vpn = vpn.clone();
  std::mem::take(sessions).relinquish();
  let worker = tokio::task::spawn_blocking(move || {
    worker_vpn.connect(move |session| {
      let _ = native_tx.try_send(Native::Established(session));
    })
  });
  tokio::pin!(worker);
  let mut worker_done = false;
  let stop = cancellation.child_token();
  let result = {
    let supervisor = supervise(
      identity,
      &mut control,
      std::mem::take(&mut prepared.bootstrap),
      native,
      stop.clone(),
      events,
    );
    tokio::pin!(supervisor);
    let mut supervisor_done = false;
    let mut approval_poll = tokio::time::interval(Duration::from_millis(250));
    let result = loop {
      tokio::select! {
        biased;
        result = &mut supervisor => { supervisor_done = true; break result; }
        result = &mut worker => { worker_done = true; break Err(anyhow::anyhow!("VPN worker ended: {:?}", result)); }
        _ = approval_poll.tick() => {
          if !approval.approval_is_valid() || !registry.has_session(session_id) {
            super::runtime::stop_for_revoked_approval(lifecycle, &cancellation);
          }
        }
      }
    };
    stop.cancel();
    // Keep the consumer's control and packet endpoints alive until OpenConnect
    // consumes cancellation and logs out. Early EOF would interrupt that logout.
    vpn.disconnect();
    if !worker_done {
      let _ = worker.await;
    }
    if !supervisor_done {
      let _ = supervisor.await;
    }
    result
  };
  control.stop().await;
  let cleanup = prepared.stop_worker();
  monitor.stop().await;
  if result.is_ok() {
    cleanup?;
  } else if let Err(error) = cleanup {
    log::warn!("Could not stop packet worker: {error:#}");
  }
  result
}

#[derive(Debug, PartialEq, Eq)]
enum WorkerState {
  Preparing,
  Ready,
  Activating,
  Active,
  Suspending,
}

// Own the channel independently of its supervision future so run() can drain
// the native VPN before telling the consumer to stop or closing its endpoints.
struct ConsumerControl {
  output: tokio::io::WriteHalf<platform::ControlStream>,
  messages: mpsc::Receiver<std::io::Result<Message>>,
  reader: tokio::task::JoinHandle<()>,
}
impl ConsumerControl {
  fn new(channel: platform::ControlStream) -> Self {
    let (mut input, output) = tokio::io::split(channel);
    let (messages_tx, messages) = mpsc::channel(8);
    let reader = tokio::spawn(async move {
      loop {
        let message = receive(&mut input).await;
        let failed = message.is_err();
        if messages_tx.send(message).await.is_err() || failed {
          break;
        }
      }
    });
    Self {
      output,
      messages,
      reader,
    }
  }
  async fn stop(&mut self) {
    let _ = tokio::time::timeout(Duration::from_millis(250), send(&mut self.output, &Message::Stop)).await;
    self.reader.abort();
    let _ = (&mut self.reader).await;
  }
}
impl Drop for ConsumerControl {
  fn drop(&mut self) {
    self.reader.abort();
  }
}

// Production and generic-consumer tests exercise this same state machine.
async fn supervise(
  identity: Identity,
  control: &mut ConsumerControl,
  mut bootstrap: Bootstrap,
  mut native: mpsc::Receiver<Native>,
  cancellation: CancellationToken,
  events: mpsc::Sender<LifecycleEvent>,
) -> anyhow::Result<()> {
  let attempt = identity.attempt;
  let ConsumerControl { output, messages, .. } = control;
  let mut network = None;
  let mut session = None;
  let mut worker = WorkerState::Preparing;
  let mut online = false;
  let mut was_connected = false;
  let mut deadline: Option<Instant> = None;
  loop {
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => break,
      _ = async { if let Some(at) = deadline { tokio::time::sleep_until(at).await } else { std::future::pending().await } } => bail!("Script-tun worker activation timed out"),
      message = messages.recv() => match message.context("Script-tun control reader stopped")?? {
        Message::Ready if worker == WorkerState::Preparing && network.is_some() => {
          worker = WorkerState::Ready;
        }
        Message::Activated if worker == WorkerState::Activating => {
          ensure!(online, "Script-tun activation completed while VPN was offline");
          ensure!(!cancellation.is_cancelled(), "Script-tun startup cancelled");
          events.send(LifecycleEvent::Established { attempt, session: session.clone().context("Native session is missing")? }).await?;
          worker = WorkerState::Active; was_connected = true; deadline = None;
        }
        Message::Suspended if worker == WorkerState::Suspending => { worker = WorkerState::Ready; deadline = None; }
        Message::Error { message } => bail!("Script-tun worker: {message}"),
        _ => bail!("Unexpected script-tun consumer message"),
      },
      event = native.recv() => match event.context("Native event stream ended")? {
        Native::Established(value) => { session = Some(value); online = true; deadline.get_or_insert(Instant::now() + Duration::from_secs(10)); }
        Native::Event(Event::Configuration(config)) => {
          let observed = config.context("VPN configuration is absent").and_then(observed_network);
          if was_connected && (observed.is_err() || network.as_ref().is_some_and(|old| observed.as_ref().is_ok_and(|new| old != new))) { return Err(NetworkChanged.into()); }
          let observed = observed?;
          if let Some(old) = &network { ensure!(old == &observed, "VPN configuration changed during startup"); }
          else {
            send(output, &Message::Configure { identity, bootstrap: std::mem::take(&mut bootstrap), network: observed.clone() }).await?;
            network = Some(observed);
            deadline = Some(Instant::now() + Duration::from_secs(10));
          }
        }
        Native::Event(Event::Reconnecting) => {
          ensure!(worker != WorkerState::Activating, "VPN connection interrupted during script-tun activation; reconnect the VPN");
          online = false;
          events.send(LifecycleEvent::ScriptTunSuspended { attempt }).await?;
          if matches!(worker, WorkerState::Ready | WorkerState::Active) {
            send(output, &Message::Suspend).await?; worker = WorkerState::Suspending;
            deadline = Some(Instant::now() + Duration::from_secs(10));
          }
        }
        Native::Event(Event::Reconnected) => online = true,
        Native::Event(_) => {},
      }
    }
    if online && session.is_some() && worker == WorkerState::Ready && !cancellation.is_cancelled() {
      send(output, &Message::Activate).await?;
      worker = WorkerState::Activating;
      deadline = Some(Instant::now() + Duration::from_secs(10));
    }
  }
  Ok(())
}

fn observed_network(
  config: openconnect::tunnel::Configuration,
) -> anyhow::Result<gpapi::session::desktop::tunnel::NegotiatedNetwork> {
  ensure!(
    !config.truncated && !config.dns.truncated && !config.search_domains.truncated,
    "VPN configuration is truncated"
  );
  Ok(gpapi::session::desktop::tunnel::NegotiatedNetwork {
    addresses: [config.ipv4, config.ipv6.or(config.netmask6)]
      .into_iter()
      .flatten()
      .map(|a| a.split('/').next().unwrap().parse())
      .collect::<Result<_, _>>()?,
    dns: config.dns.items.iter().map(|a| a.parse()).collect::<Result<_, _>>()?,
    search: config.search_domains.items,
    mtu: config.mtu.context("VPN MTU is absent")? as usize,
  })
}

async fn send(writer: &mut (impl tokio::io::AsyncWrite + Unpin), message: &Message) -> anyhow::Result<()> {
  tokio::time::timeout(Duration::from_secs(10), send_message(writer, message))
    .await
    .context("Script-tun control write timed out")??;
  Ok(())
}

#[cfg(all(test, unix))]
mod tests;
