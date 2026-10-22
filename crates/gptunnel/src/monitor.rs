use std::{
  sync::{Arc, Mutex, Weak},
  time::Duration,
};

use gpapi::session::tunnel::{
  TunnelPhase, TunnelState, TunnelStatistics, TunnelStatisticsSnapshot, TunnelStatisticsState, TunnelStatisticsView,
  TunnelTransport,
};
use log::{info, warn};
use openconnect::{
  Vpn,
  tunnel::{Event, Statistics, Transport},
};
use tokio::{
  sync::{Notify, watch},
  task::JoinHandle,
  time::Instant,
};
use uuid::Uuid;

use crate::configuration::{log_configuration, normalize};

const SAMPLE_INTERVAL: Duration = Duration::from_secs(2);
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Clone, Debug)]
pub struct TunnelUpdate {
  pub state: TunnelState,
  pub statistics: TunnelStatisticsSnapshot,
}

struct RequestStamp {
  generation: u64,
  state_sequence: u64,
  issued: Instant,
}

struct RetainedSample {
  value: TunnelStatistics,
  acquired: Instant,
}

struct Observation {
  state: TunnelState,
  sequence: u64,
  statistics_sequence: u64,
  started: Option<Instant>,
  ended_elapsed: Option<u64>,
  configuration_observed: bool,
  enabled: bool,
  generation: u64,
  pending: Option<RequestStamp>,
  sample: Option<RetainedSample>,
  statistics_state: TunnelStatisticsState,
  finished: bool,
  request_failed: bool,
}

impl Observation {
  fn new() -> Self {
    Self {
      state: TunnelState {
        tunnel_id: Uuid::new_v4(),
        sequence: 1,
        phase: TunnelPhase::Establishing,
        configuration_revision: 0,
        configuration: None,
        transport: TunnelTransport::Negotiating,
        elapsed_ms: 0,
      },
      sequence: 1,
      statistics_sequence: 1,
      started: None,
      ended_elapsed: None,
      configuration_observed: false,
      enabled: false,
      generation: 0,
      pending: None,
      sample: None,
      statistics_state: TunnelStatisticsState::Disabled,
      finished: false,
      request_failed: false,
    }
  }

  fn next_sequence(&mut self) -> u64 {
    // A 64-bit sequence cannot exhaust in a practical tunnel lifetime. Never
    // wrap an ordering token into an earlier generation.
    self.sequence = self
      .sequence
      .checked_add(1)
      .expect("tunnel observation sequence exhausted");
    self.sequence
  }

  fn elapsed_ms(&self, now: Instant) -> u64 {
    if let Some(elapsed) = self.ended_elapsed {
      return elapsed;
    }
    self
      .started
      .map_or(0, |started| millis(now.saturating_duration_since(started)))
  }

  fn active(&self) -> bool {
    !self.finished && self.state.phase == TunnelPhase::Connected && self.state.transport.is_active()
  }

  fn project(&self, now: Instant) -> TunnelUpdate {
    let mut state = self.state.clone();
    state.elapsed_ms = self.elapsed_ms(now);
    let statistics = TunnelStatisticsSnapshot::Available(TunnelStatisticsView {
      sequence: self.statistics_sequence,
      tunnel_id: state.tunnel_id,
      state_sequence: state.sequence,
      phase: state.phase,
      transport: state.transport,
      state: self.statistics_state,
      elapsed_ms: state.elapsed_ms,
      sample_age_ms: self
        .sample
        .as_ref()
        .map(|sample| millis(now.saturating_duration_since(sample.acquired))),
      sample: self.sample.as_ref().map(|sample| sample.value.clone()),
    });
    TunnelUpdate { state, statistics }
  }

  fn invalidate_statistics(&mut self) {
    self.statistics_state = if self.finished {
      TunnelStatisticsState::Ended
    } else if !self.enabled {
      TunnelStatisticsState::Disabled
    } else if self.sample.is_some() || !self.active() {
      TunnelStatisticsState::Stale
    } else {
      TunnelStatisticsState::Waiting
    };
    self.statistics_sequence = self.next_sequence();
  }

  fn state_changed(&mut self) {
    self.state.sequence = self.next_sequence();
    self.invalidate_statistics();
  }

  fn enable(&mut self, enabled: bool) -> bool {
    if self.finished || self.enabled == enabled {
      return false;
    }
    self.enabled = enabled;
    self.generation = self
      .generation
      .checked_add(1)
      .expect("tunnel collection generation exhausted");
    self.statistics_state = if enabled {
      TunnelStatisticsState::Waiting
    } else {
      TunnelStatisticsState::Disabled
    };
    self.statistics_sequence = self.next_sequence();
    true
  }

  fn finish(&mut self, now: Instant) {
    if self.finished {
      return;
    }
    self.ended_elapsed = Some(self.elapsed_ms(now));
    self.finished = true;
    self.enabled = false;
    self.pending = None;
    self.state.phase = TunnelPhase::Ended;
    self.state.transport = TunnelTransport::Unavailable;
    self.state_changed();
  }

  /// Returns whether the configuration/transport should be logged. Logging is
  /// performed outside the observation lock and never includes traffic samples.
  fn apply(&mut self, event: Event, now: Instant) -> (bool, bool) {
    if self.finished {
      return (false, false);
    }
    match event {
      Event::Configuration(configuration) => {
        let configuration = configuration.map(normalize);
        if self.configuration_observed && self.state.configuration == configuration {
          return (false, false);
        }
        self.configuration_observed = true;
        self.state.configuration = configuration;
        self.state.configuration_revision = self
          .state
          .configuration_revision
          .checked_add(1)
          .expect("configuration revision exhausted");
        self.state_changed();
        (true, false)
      }
      Event::Established => {
        self.started.get_or_insert(now);
        if self.state.phase != TunnelPhase::Connected {
          self.state.phase = TunnelPhase::Connected;
          self.state_changed();
        }
        (false, false)
      }
      Event::Reconnected => {
        if self.started.is_some() && self.state.phase != TunnelPhase::Connected {
          self.state.phase = TunnelPhase::Connected;
          self.state_changed();
        }
        (false, false)
      }
      Event::Reconnecting => {
        let phase = if self.started.is_some() {
          TunnelPhase::Reconnecting
        } else {
          TunnelPhase::Establishing
        };
        if self.state.phase != phase {
          self.state.phase = phase;
          self.state_changed();
        }
        (false, false)
      }
      Event::Transport(transport) => {
        let transport = match transport {
          Transport::Negotiating => TunnelTransport::Negotiating,
          Transport::IpsecEsp => TunnelTransport::IpsecEsp,
          Transport::SslTls => TunnelTransport::SslTls,
          Transport::Unavailable => TunnelTransport::Unavailable,
        };
        if self.state.transport == transport {
          return (false, false);
        }
        self.state.transport = transport;
        self.state_changed();
        (false, true)
      }
      Event::Statistics(statistics) => {
        self.receive_statistics(statistics, now);
        (false, false)
      }
      Event::Ended => {
        self.finish(now);
        (false, false)
      }
    }
  }

  fn receive_statistics(&mut self, statistics: Statistics, now: Instant) {
    let Some(stamp) = self.pending.take() else {
      return;
    };
    if !self.enabled
      || !self.active()
      || stamp.generation != self.generation
      || stamp.state_sequence != self.state.sequence
    {
      return;
    }
    let sequence = self.next_sequence();
    self.sample = Some(RetainedSample {
      value: TunnelStatistics {
        tunnel_id: self.state.tunnel_id,
        sequence,
        state_sequence: self.state.sequence,
        configuration_revision: self.state.configuration_revision,
        elapsed_ms: self.elapsed_ms(now),
        received_bytes: statistics.received_bytes,
        sent_bytes: statistics.sent_bytes,
        received_packets: statistics.received_packets,
        sent_packets: statistics.sent_packets,
      },
      acquired: now,
    });
    self.statistics_sequence = sequence;
    self.statistics_state = TunnelStatisticsState::Fresh;
  }
}

/// One native tunnel's negotiated state and optional sampler. The connection
/// owner must join `Vpn::connect` before calling `stop`.
pub struct TunnelMonitor {
  vpn: Arc<Vpn>,
  observation: Mutex<Observation>,
  updates: watch::Sender<TunnelUpdate>,
  wake: Arc<Notify>,
  worker: Mutex<Option<JoinHandle<()>>>,
}

impl TunnelMonitor {
  pub fn attach(vpn: Arc<Vpn>) -> Arc<Self> {
    Self::attach_with_observer(vpn, |_| {})
  }

  /// Observe every native transition as well as the coalesced telemetry stream.
  /// The callback runs on the native worker and must not block.
  pub fn attach_with_observer(vpn: Arc<Vpn>, on_event: impl Fn(&Event) + Send + Sync + 'static) -> Arc<Self> {
    let observation = Observation::new();
    let (updates, _) = watch::channel(observation.project(Instant::now()));
    let monitor = Arc::new(Self {
      vpn,
      observation: Mutex::new(observation),
      updates,
      wake: Arc::new(Notify::new()),
      worker: Mutex::new(None),
    });
    let observer = Arc::downgrade(&monitor);
    monitor.vpn.set_tunnel_observer(move |event| {
      on_event(&event);
      if let Some(monitor) = observer.upgrade() {
        monitor.observe(event);
      }
    });
    let worker = tokio::spawn(sample(Arc::downgrade(&monitor), monitor.wake.clone()));
    *monitor.worker.lock().unwrap() = Some(worker);
    monitor
  }

  pub fn subscribe(&self) -> watch::Receiver<TunnelUpdate> {
    self.updates.subscribe()
  }

  pub fn state(&self) -> TunnelState {
    self.observation.lock().unwrap().project(Instant::now()).state
  }

  pub fn statistics(&self) -> TunnelStatisticsSnapshot {
    self.observation.lock().unwrap().project(Instant::now()).statistics
  }

  pub fn set_statistics_enabled(&self, enabled: bool) {
    let mut observation = self.observation.lock().unwrap();
    if observation.enable(enabled) {
      self.publish(&observation);
      self.wake.notify_one();
    }
  }

  /// End observation after native execution has returned. This does not request
  /// VPN disconnect or take over its credential/network cleanup.
  pub fn finish(&self) {
    let mut observation = self.observation.lock().unwrap();
    observation.finish(Instant::now());
    self.publish(&observation);
    drop(observation);
    self.vpn.clear_tunnel_observer();
    self.wake.notify_one();
  }

  pub async fn stop(&self) {
    self.finish();
    let worker = self.worker.lock().unwrap().take();
    if let Some(worker) = worker {
      let _ = worker.await;
    }
  }

  fn publish(&self, observation: &Observation) {
    self.updates.send_replace(observation.project(Instant::now()));
  }

  fn observe(&self, event: Event) {
    let is_statistics = matches!(event, Event::Statistics(_));
    let mut observation = self.observation.lock().unwrap();
    let (configuration_changed, transport_changed) = observation.apply(event, Instant::now());
    self.publish(&observation);
    let state = observation.project(Instant::now()).state;
    let retry = is_statistics
      && observation.sample.as_ref().is_none_or(|sample| {
        sample.value.state_sequence != observation.state.sequence
          || observation.statistics_state != TunnelStatisticsState::Fresh
      });
    drop(observation);
    if configuration_changed {
      log_configuration(&state);
    }
    if transport_changed {
      info!("VPN transport: {}", state.transport.label());
    }
    if !is_statistics || retry {
      self.wake.notify_one();
    }
  }

  fn request_sample(&self) -> bool {
    let mut observation = self.observation.lock().unwrap();
    if observation.finished {
      return false;
    }
    if !observation.enabled || !observation.active() {
      return true;
    }
    let now = Instant::now();
    if let Some(pending) = &observation.pending {
      if now.saturating_duration_since(pending.issued) >= SAMPLE_TIMEOUT
        && observation.statistics_state != TunnelStatisticsState::Stale
      {
        observation.statistics_state = TunnelStatisticsState::Stale;
        observation.statistics_sequence = observation.next_sequence();
        self.publish(&observation);
      }
      return true;
    }
    match self.vpn.request_statistics() {
      Ok(true) => {
        observation.pending = Some(RequestStamp {
          generation: observation.generation,
          state_sequence: observation.state.sequence,
          issued: now,
        });
        observation.request_failed = false;
      }
      Ok(false) => {}
      Err(error) => {
        if !observation.request_failed {
          warn!("VPN statistics request failed: {error}");
          observation.request_failed = true;
        }
        observation.statistics_state = TunnelStatisticsState::Stale;
        observation.statistics_sequence = observation.next_sequence();
        self.publish(&observation);
      }
    }
    true
  }
}

impl Drop for TunnelMonitor {
  fn drop(&mut self) {
    self.vpn.clear_tunnel_observer();
    if let Some(worker) = self.worker.get_mut().unwrap().take() {
      worker.abort();
    }
  }
}

async fn sample(monitor: Weak<TunnelMonitor>, wake: Arc<Notify>) {
  loop {
    let enabled = {
      let Some(monitor) = monitor.upgrade() else {
        return;
      };
      if !monitor.request_sample() {
        return;
      }
      monitor.observation.lock().unwrap().enabled
    };
    if enabled {
      tokio::select! {
        _ = wake.notified() => {}
        _ = tokio::time::sleep(SAMPLE_INTERVAL) => {}
      }
    } else {
      wake.notified().await;
    }
  }
}

fn millis(duration: Duration) -> u64 {
  duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
  use super::*;

  fn connected(now: Instant) -> Observation {
    let mut observation = Observation::new();
    observation.apply(Event::Established, now);
    observation.apply(Event::Transport(Transport::IpsecEsp), now);
    observation
  }

  fn request(observation: &mut Observation, now: Instant) {
    observation.pending = Some(RequestStamp {
      generation: observation.generation,
      state_sequence: observation.state.sequence,
      issued: now,
    });
  }

  #[test]
  fn retries_before_first_tun_establishment_do_not_start_session_elapsed_time() {
    let now = Instant::now();
    let mut observation = Observation::new();
    observation.apply(Event::Reconnecting, now);
    observation.apply(Event::Reconnected, now + Duration::from_secs(10));
    let update = observation.project(now + Duration::from_secs(60));
    assert_eq!(update.state.phase, TunnelPhase::Establishing);
    assert_eq!(update.state.elapsed_ms, 0);
    assert_eq!(update.statistics.view().unwrap().elapsed_ms, 0);
    observation.apply(Event::Established, now + Duration::from_secs(61));
    assert_eq!(
      observation.project(now + Duration::from_secs(62)).state.elapsed_ms,
      1000
    );
  }

  #[test]
  fn late_sample_after_disable_reenable_cannot_become_fresh() {
    let now = Instant::now();
    let mut observation = connected(now);
    observation.enable(true);
    request(&mut observation, now);
    observation.enable(false);
    observation.enable(true);
    observation.receive_statistics(
      Statistics {
        received_bytes: 5,
        ..Default::default()
      },
      now,
    );
    assert!(observation.pending.is_none());
    assert!(observation.sample.is_none());
    assert_eq!(observation.statistics_state, TunnelStatisticsState::Waiting);
    request(&mut observation, now);
    observation.receive_statistics(
      Statistics {
        received_bytes: 8,
        ..Default::default()
      },
      now,
    );
    assert_eq!(observation.sample.as_ref().unwrap().value.received_bytes, 8);
    assert_eq!(observation.statistics_state, TunnelStatisticsState::Fresh);
  }

  #[test]
  fn reconnect_preserves_elapsed_and_rejects_old_transport_sample() {
    let now = Instant::now();
    let mut observation = connected(now);
    observation.enable(true);
    request(&mut observation, now);
    observation.apply(Event::Reconnecting, now + Duration::from_secs(2));
    observation.apply(Event::Transport(Transport::Unavailable), now + Duration::from_secs(2));
    observation.apply(Event::Reconnected, now + Duration::from_secs(5));
    observation.apply(Event::Transport(Transport::SslTls), now + Duration::from_secs(5));
    observation.receive_statistics(Statistics::default(), now + Duration::from_secs(5));
    assert!(observation.sample.is_none());
    request(&mut observation, now + Duration::from_secs(5));
    observation.receive_statistics(
      Statistics {
        sent_bytes: 40,
        ..Default::default()
      },
      now + Duration::from_secs(6),
    );
    assert_eq!(observation.elapsed_ms(now + Duration::from_secs(6)), 6000);
    assert_eq!(observation.sample.as_ref().unwrap().value.sent_bytes, 40);
    assert_eq!(observation.statistics_state, TunnelStatisticsState::Fresh);
  }

  #[test]
  fn time_projection_does_not_poll_or_rewrite_acquisition_time() {
    let now = Instant::now();
    let mut observation = connected(now);
    observation.enable(true);
    request(&mut observation, now);
    observation.receive_statistics(Statistics::default(), now + Duration::from_secs(1));
    let later = observation.project(now + Duration::from_secs(600));
    assert_eq!(later.state.elapsed_ms, 600000);
    let view = later.statistics.view().unwrap();
    assert_eq!(view.sample_age_ms, Some(599000));
    assert_eq!(view.sample.as_ref().unwrap().elapsed_ms, 1000);
    assert!(observation.pending.is_none());
    observation.finish(now + Duration::from_secs(601));
    assert_eq!(
      observation.project(now + Duration::from_secs(700)).state.elapsed_ms,
      601000
    );
  }

  #[cfg(unix)]
  #[tokio::test(start_paused = true)]
  async fn sampling_is_demand_driven_and_coalesces_commands_on_the_native_pipe() {
    use std::{
      io::Read,
      os::{fd::AsRawFd, unix::net::UnixStream},
    };

    unsafe extern "C" {
      fn vpn_attach_command_pipe(vpn: *mut std::ffi::c_void, fd: std::ffi::c_int) -> std::ffi::c_int;
      fn vpn_detach_command_pipe(vpn: *mut std::ffi::c_void);
    }

    let vpn = Arc::new(
      Vpn::builder("vpn.example.com", "cookie")
        .network_configuration_handler(|_| Ok(()))
        .build()
        .unwrap(),
    );
    let (sender, mut receiver) = UnixStream::pair().unwrap();
    sender.set_nonblocking(true).unwrap();
    receiver.set_nonblocking(true).unwrap();
    let pointer = Arc::as_ptr(&vpn).cast_mut().cast();
    // SAFETY: this test owns the live Vpn and both socket endpoints, and detaches
    // the borrowed command descriptor before either endpoint or Vpn is dropped.
    assert_eq!(unsafe { vpn_attach_command_pipe(pointer, sender.as_raw_fd()) }, 0);
    let monitor = TunnelMonitor::attach(vpn.clone());
    monitor.observe(Event::Established);
    monitor.observe(Event::Transport(Transport::IpsecEsp));
    tokio::task::yield_now().await;
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    let mut command = [0];
    assert_eq!(
      receiver.read(&mut command).unwrap_err().kind(),
      std::io::ErrorKind::WouldBlock
    );
    assert!(monitor.observation.lock().unwrap().pending.is_none());
    assert_eq!(
      monitor.statistics().view().unwrap().state,
      TunnelStatisticsState::Disabled
    );

    monitor.set_statistics_enabled(true);
    tokio::task::yield_now().await;
    assert_eq!(receiver.read(&mut command).unwrap(), 1);
    assert_eq!(command, [b's']);
    tokio::time::advance(Duration::from_secs(6)).await;
    tokio::task::yield_now().await;
    assert_eq!(
      receiver.read(&mut command).unwrap_err().kind(),
      std::io::ErrorKind::WouldBlock
    );
    assert_eq!(monitor.statistics().view().unwrap().state, TunnelStatisticsState::Stale);
    monitor.set_statistics_enabled(false);
    tokio::time::advance(Duration::from_secs(60)).await;
    tokio::task::yield_now().await;
    assert_eq!(
      receiver.read(&mut command).unwrap_err().kind(),
      std::io::ErrorKind::WouldBlock
    );

    monitor.stop().await;
    // SAFETY: pointer still identifies the Vpn retained by this test.
    unsafe { vpn_detach_command_pipe(pointer) };
  }
}
