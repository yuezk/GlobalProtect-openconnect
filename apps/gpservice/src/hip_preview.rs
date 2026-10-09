use std::{collections::HashMap, io, sync::Arc, time::Duration};

use anyhow::{Context, bail, ensure};
use gpapi::{
  hip::HipSource,
  os_profile::HostIdentity,
  service::{
    request::PreviewHipReportRequest,
    transport::{ServiceErrorCode, ServiceResult},
  },
};
use openconnect::HipRequest;
use tokio::{
  sync::{Mutex, Semaphore},
  task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{session_registry::SessionRegistry, ws_connection::ConnectionControl};

const PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_ACTIVE_PREVIEWS: usize = 4;

struct PreviewTask {
  connection_id: Uuid,
  cancellation: CancellationToken,
  worker: JoinHandle<()>,
}

pub(crate) struct PreviewJob {
  pub session_id: Uuid,
  pub request_id: Uuid,
  pub request: PreviewHipReportRequest,
  pub edited_report: Option<Arc<str>>,
  pub desktop_uid: Option<u32>,
  pub identity: HostIdentity,
  pub registry: Arc<SessionRegistry>,
  pub connection: ConnectionControl,
}

pub(crate) struct HipPreviews {
  tasks: Mutex<HashMap<Uuid, PreviewTask>>,
  slots: Arc<Semaphore>,
  shutdown: CancellationToken,
}

impl HipPreviews {
  pub(crate) fn new() -> Self {
    Self {
      tasks: Mutex::new(HashMap::new()),
      slots: Arc::new(Semaphore::new(MAX_ACTIVE_PREVIEWS)),
      shutdown: CancellationToken::new(),
    }
  }

  pub(crate) async fn start(&self, job: PreviewJob) -> bool {
    let PreviewJob {
      session_id,
      request_id,
      request,
      edited_report,
      desktop_uid,
      identity,
      registry,
      connection,
    } = job;
    let mut tasks = self.tasks.lock().await;
    if self.shutdown.is_cancelled() || tasks.get(&session_id).is_some_and(|task| !task.worker.is_finished()) {
      return false;
    }
    if let Some(completed) = tasks.remove(&session_id) {
      let _ = completed.worker.await;
    }
    let Ok(slot) = Arc::clone(&self.slots).try_acquire_owned() else {
      return false;
    };
    let cancellation = self.shutdown.child_token();
    let worker_cancellation = cancellation.clone();
    let connection_id = connection.connection_id();
    let worker = tokio::spawn(async move {
      let _slot = slot;
      let generated = tokio::task::spawn_blocking(move || {
        generate(request, edited_report, desktop_uid, identity, worker_cancellation)
      })
      .await;
      let result = match generated {
        Ok(Ok(xml)) => preview_result(&registry, session_id, xml),
        Ok(Err(error)) => {
          log::warn!("HIP preview failed: {error:#}");
          ServiceResult::rejected(ServiceErrorCode::Internal, "HIP preview failed")
        }
        Err(error) => {
          log::warn!("HIP preview worker failed: {error}");
          ServiceResult::rejected(ServiceErrorCode::Internal, "HIP preview failed")
        }
      };
      // A bounded connection queue must not prevent preview teardown.
      let _ = connection.send(crate::ws_connection::ConnectionCommand::Reply { id: request_id, result });
    });
    tasks.insert(
      session_id,
      PreviewTask {
        connection_id,
        cancellation,
        worker,
      },
    );
    true
  }

  pub(crate) async fn cancel_connection(&self, session_id: Uuid, connection_id: Uuid) {
    let mut tasks = self.tasks.lock().await;
    if !tasks
      .get(&session_id)
      .is_some_and(|task| task.connection_id == connection_id)
    {
      return;
    }
    if let Some(task) = tasks.remove(&session_id) {
      task.cancellation.cancel();
      let _ = task.worker.await;
    }
  }

  pub(crate) async fn shutdown(&self) {
    self.shutdown.cancel();
    let tasks = std::mem::take(&mut *self.tasks.lock().await);
    for task in tasks.into_values() {
      let _ = task.worker.await;
    }
  }
}

fn preview_result(registry: &SessionRegistry, session_id: Uuid, xml: String) -> ServiceResult {
  let result = registry.store_hip_preview(session_id, xml).and_then(|preview_id| {
    registry
      .hip_preview_chunk(session_id, preview_id, 0)
      .map(|(xml, complete)| ServiceResult::HipPreviewChunk {
        preview_id: preview_id.to_string(),
        offset: 0,
        xml,
        complete,
      })
  });
  result.unwrap_or_else(|_| ServiceResult::rejected(ServiceErrorCode::Internal, "HIP preview is unavailable"))
}

fn generate(
  request: PreviewHipReportRequest,
  edited_report: Option<Arc<str>>,
  desktop_uid: Option<u32>,
  identity: HostIdentity,
  cancellation: CancellationToken,
) -> anyhow::Result<String> {
  ensure!(request.client_version.len() <= 64, "Invalid HIP client version");
  ensure!(
    request.host_id.as_ref().is_none_or(|id| id.len() <= 256),
    "Invalid HIP host ID"
  );
  let budget = gpapi::process::collection::CollectionBudget::new(PREVIEW_TIMEOUT);
  let check = || {
    if cancellation.is_cancelled() {
      return Err(io::Error::new(io::ErrorKind::Interrupted, "HIP preview cancelled"));
    }
    gpapi::process::collection::CollectionControl::check(&budget)
  };
  check()?;
  let profile = crate::hip_source::profile(
    &identity,
    request.client_os,
    Some(request.client_version.clone()),
    None,
    request.host_id.clone(),
    None,
    &check,
  )?;
  let input = HipRequest {
    cookie: "user=preview&domain=preview&computer=preview".into(),
    client_ip: Some("192.0.2.1".into()),
    client_ipv6: None,
    md5: "00000000000000000000000000000000".into(),
    client_version: request.client_version,
    client_os: request.client_os.as_str().into(),
    os_version: profile.os_version().into(),
    host_id: request.host_id,
    local_hostname: Some(profile.computer().into()),
  };
  let xml = match &request.source {
    HipSource::Disabled => bail!("HIP is disabled"),
    HipSource::Generated => crate::hip_source::generate(&profile, None, &input, &check, true)?,
    HipSource::Edited { .. } => {
      let xml = edited_report.context("Edited HIP report is unavailable")?;
      crate::hip_source::generate(&profile, Some(&xml), &input, &check, true)?
    }
    source => {
      let execution = crate::hip_source::resolve(source, desktop_uid, false, None, Some(profile))?;
      let openconnect::HipSource::Script(script) = execution.source else {
        bail!("HIP preview executable is unavailable");
      };
      let xml = script.collect(&input, &check)?;
      gphip::validate_edited_report(&xml).context("Custom HIP script did not emit a valid HIP report")?;
      xml
    }
  };
  check()?;
  Ok(xml)
}

#[cfg(test)]
mod tests {
  use super::*;
  use gpapi::os_profile::ClientOs;
  use std::sync::atomic::{AtomicBool, Ordering};

  fn job(session_id: Uuid) -> PreviewJob {
    let (tx, _) = tokio::sync::mpsc::channel(1);
    let (connection, _) = ConnectionControl::new(Uuid::new_v4(), tx);
    PreviewJob {
      session_id,
      request_id: Uuid::new_v4(),
      request: PreviewHipReportRequest {
        source: HipSource::Generated,
        client_os: ClientOs::Linux,
        client_version: "6.3.3".into(),
        host_id: None,
      },
      edited_report: None,
      desktop_uid: None,
      identity: HostIdentity::from_parts(
        "device".into(),
        "host".into(),
        "serial".into(),
        "00:11:22:33:44:55".into(),
      ),
      registry: Arc::new(SessionRegistry::new(Uuid::new_v4())),
      connection,
    }
  }

  #[tokio::test]
  async fn preview_admission_rejects_duplicate_session_and_global_capacity() {
    let previews = HipPreviews::new();
    let session_id = Uuid::new_v4();
    let cancellation = previews.shutdown.child_token();
    let worker_cancellation = cancellation.clone();
    let worker = tokio::spawn(async move { worker_cancellation.cancelled().await });
    let connection_id = Uuid::new_v4();
    previews.tasks.lock().await.insert(
      session_id,
      PreviewTask {
        connection_id,
        cancellation,
        worker,
      },
    );
    assert!(!previews.start(job(session_id)).await);
    previews.cancel_connection(session_id, connection_id).await;
    let _all_slots = Arc::clone(&previews.slots)
      .acquire_many_owned(MAX_ACTIVE_PREVIEWS as u32)
      .await
      .unwrap();
    assert!(!previews.start(job(Uuid::new_v4())).await);
    previews.shutdown().await;
  }

  #[tokio::test]
  async fn closing_preview_owner_waits_for_worker_teardown() {
    let previews = HipPreviews::new();
    let session_id = Uuid::new_v4();
    let connection_id = Uuid::new_v4();
    let cancellation = previews.shutdown.child_token();
    let worker_cancellation = cancellation.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stopped = Arc::clone(&stopped);
    let worker = tokio::spawn(async move {
      worker_cancellation.cancelled().await;
      tokio::task::yield_now().await;
      worker_stopped.store(true, Ordering::SeqCst);
    });
    previews.tasks.lock().await.insert(
      session_id,
      PreviewTask {
        connection_id,
        cancellation,
        worker,
      },
    );
    previews.cancel_connection(session_id, Uuid::new_v4()).await;
    assert!(!stopped.load(Ordering::SeqCst));
    previews.cancel_connection(session_id, connection_id).await;
    assert!(stopped.load(Ordering::SeqCst));
    assert!(previews.tasks.lock().await.is_empty());
    previews.shutdown().await;
    assert!(!previews.start(job(session_id)).await);
  }

  #[test]
  fn cancelled_preview_does_not_start_collection() {
    let preview = job(Uuid::new_v4());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let error = generate(preview.request, None, None, preview.identity, cancelled).unwrap_err();
    assert_eq!(
      error.downcast_ref::<io::Error>().unwrap().kind(),
      io::ErrorKind::Interrupted
    );
  }
}
