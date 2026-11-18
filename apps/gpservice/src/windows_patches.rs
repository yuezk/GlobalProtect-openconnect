//! One service-owned local patch search, independent of report generation.
use gpapi::{
  collection::{CollectionBudget, CollectionControl},
  process::collection::CollectorCommands,
};
use std::{
  collections::HashSet,
  sync::{Arc, Mutex},
  time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const INTERVAL: Duration = Duration::from_secs(15 * 60);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(60);

enum Command {
  Interest(Uuid, bool),
  CheckIfDue,
  Refresh(Option<u64>, oneshot::Sender<bool>),
}

pub(crate) struct PatchChecks {
  tx: mpsc::UnboundedSender<Command>,
  rx: Mutex<Option<mpsc::UnboundedReceiver<Command>>>,
}

pub(crate) struct Consumer {
  checks: Arc<PatchChecks>,
  id: Uuid,
}
impl Drop for Consumer {
  fn drop(&mut self) {
    self.checks.set_interest(self.id, false);
  }
}

impl PatchChecks {
  pub(crate) fn new() -> Arc<Self> {
    let (tx, rx) = mpsc::unbounded_channel();
    Arc::new(Self {
      tx,
      rx: Mutex::new(Some(rx)),
    })
  }
  pub(crate) fn set_interest(&self, id: Uuid, enabled: bool) {
    let _ = self.tx.send(Command::Interest(id, enabled));
  }
  pub(crate) fn check_if_due(&self) {
    let _ = self.tx.send(Command::CheckIfDue);
  }
  pub(crate) fn consumer(self: &Arc<Self>) -> Consumer {
    let id = Uuid::new_v4();
    self.set_interest(id, true);
    Consumer {
      checks: self.clone(),
      id,
    }
  }
  pub(crate) async fn refresh(&self, after: Option<u64>) -> bool {
    let (tx, rx) = oneshot::channel();
    if self.tx.send(Command::Refresh(after, tx)).is_err() {
      return false;
    }
    rx.await.unwrap_or(false)
  }
  pub(crate) async fn run(&self, stop: CancellationToken) {
    let Some(mut commands) = self.rx.lock().unwrap().take() else {
      return;
    };
    let mut consumers = HashSet::new();
    let mut waiters = Vec::<oneshot::Sender<bool>>::new();
    let mut worker = None;
    // Reading status never searches Windows Update. Missing/unreadable data is due immediately.
    let status = tokio::task::spawn_blocking({
      let stop = stop.clone();
      move || helper("--patch-status", None, stop)
    })
    .await;
    let mut next = match status {
      Ok(Ok(Some(checked))) => checked.saturating_add(INTERVAL.as_secs()),
      _ => 0,
    };
    loop {
      tokio::select! {
        _ = stop.cancelled() => break,
        command = commands.recv() => {
          match command {
            Some(Command::Interest(id, enabled)) => {
              if enabled { consumers.insert(id); } else { consumers.remove(&id); }
            }
            Some(Command::CheckIfDue) => {
              if worker.is_none() && now() >= next { worker = Some(start(None, stop.clone())); }
            }
            Some(Command::Refresh(after, reply)) => {
              waiters.push(reply);
              if worker.is_none() { worker = Some(start(after, stop.clone())); }
            }
            None => break,
          }
        }
        result = async { match &mut worker { Some(task) => task.await, None => std::future::pending().await } } => {
          worker = None;
          let success = matches!(&result, Ok(Ok(Some(_))));
          if !success { log::warn!("Local Windows patch check failed: {result:?}"); }
          for reply in waiters.drain(..) { let _ = reply.send(success); }
          next = match result {
            Ok(Ok(Some(checked))) => checked.saturating_add(INTERVAL.as_secs()),
            _ => now().saturating_add(INTERVAL.as_secs()),
          };
        }
        _ = tokio::time::sleep(Duration::from_secs(next.saturating_sub(now()).min(30))),
          if worker.is_none() && !consumers.is_empty() => {
          if now() >= next { worker = Some(start(None, stop.clone())); }
        }
      }
    }
    if let Some(task) = worker {
      let _ = task.await;
    }
    for reply in waiters {
      let _ = reply.send(false);
    }
  }
}

fn now() -> u64 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs()
}
fn start(after: Option<u64>, stop: CancellationToken) -> tokio::task::JoinHandle<anyhow::Result<Option<u64>>> {
  tokio::task::spawn_blocking(move || helper("--refresh-patches", after, stop))
}
fn helper(command: &str, after: Option<u64>, stop: CancellationToken) -> anyhow::Result<Option<u64>> {
  let budget = CollectionBudget::new(SEARCH_TIMEOUT);
  let check = || {
    if stop.is_cancelled() {
      return Err(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "Patch check cancelled",
      ));
    }
    budget.check()
  };
  let path = std::env::current_exe()?.with_file_name("gp-posture.exe");
  let after = after.map(|after| after.to_string());
  let mut args = vec![command];
  if let Some(after) = &after {
    args.push(after);
  }
  let output = CollectorCommands::new(&check)
    .run_executable(&path, &args)?
    .ok_or_else(|| anyhow::anyhow!("Windows posture helper is unavailable"))?;
  anyhow::ensure!(output.status.success(), "Windows posture helper failed");
  Ok(serde_json::from_slice(&output.stdout)?)
}
