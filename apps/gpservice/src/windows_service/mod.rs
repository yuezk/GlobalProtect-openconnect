//! Service Control Manager hosting; the GUI remains in the user's desktop session.
mod admission;
mod logging;
use crate::{
  request_dispatcher::RequestDispatcher, session_registry::SessionRegistry, vpn_task::VpnTask, ws_server::WsServer,
};
use anyhow::{Context, ensure};
use gpapi::{
  service::vpn_state::VpnState,
  utils::{lock_file::LockFile, redact::Redaction},
};
use std::{
  ffi::OsString,
  io::Write,
  sync::{Arc, OnceLock, atomic::AtomicBool},
  time::Duration,
};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use windows_service::{
  define_windows_service,
  service::{ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType},
  service_control_handler::{self, ServiceControlHandlerResult, ServiceStatusHandle},
  service_dispatcher,
};
const NAME: &str = common::paths::SERVICE_NAME;
static LOG_LEVEL: OnceLock<log::Level> = OnceLock::new();
define_windows_service!(ffi_service_main, service_main);

pub(crate) fn dispatch(level: log::Level) -> anyhow::Result<()> {
  let _ = LOG_LEVEL.set(level);
  service_dispatcher::start(NAME, ffi_service_main).context("Start GP Connect through the Windows service manager")?;
  Ok(())
}
fn status(handle: &ServiceStatusHandle, state: ServiceState, failure: bool) -> windows_service::Result<()> {
  let running = state == ServiceState::Running;
  let pending = matches!(state, ServiceState::StartPending | ServiceState::StopPending);
  handle.set_service_status(ServiceStatus {
    service_type: ServiceType::OWN_PROCESS,
    current_state: state,
    controls_accepted: if running {
      ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
    } else {
      ServiceControlAccept::empty()
    },
    exit_code: if failure {
      ServiceExitCode::ServiceSpecific(1)
    } else {
      ServiceExitCode::Win32(0)
    },
    checkpoint: u32::from(pending),
    wait_hint: if pending {
      Duration::from_secs(30)
    } else {
      Duration::ZERO
    },
    process_id: None,
  })
}
fn service_main(_arguments: Vec<OsString>) {
  let stop = CancellationToken::new();
  let requested = stop.clone();
  let handle = match service_control_handler::register(NAME, move |control| match control {
    ServiceControl::Stop | ServiceControl::Shutdown => {
      requested.cancel();
      ServiceControlHandlerResult::NoError
    }
    ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
    _ => ServiceControlHandlerResult::NotImplemented,
  }) {
    Ok(handle) => handle,
    Err(_) => return,
  };
  let result = (|| -> anyhow::Result<()> {
    status(&handle, ServiceState::StartPending, false)?;
    ensure!(
      gpapi::windows::current_user_sid()? == "S-1-5-18",
      "GP Connect must run as LocalSystem"
    );
    let redaction = initialize_logger()?;
    log::info!("gpservice started: {}", crate::VERSION);
    crate::vpn_script::recover_network()?;
    let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    runtime.block_on(run(&handle, stop, redaction))
  })();
  if let Err(error) = &result {
    log::error!("Windows service stopped: {error:#}");
  }
  let _ = status(&handle, ServiceState::Stopped, result.is_err());
}
fn initialize_logger() -> anyhow::Result<Arc<Redaction>> {
  let state = common::paths::service_lock_file();
  let directory = state.parent().context("Service state directory is unavailable")?;
  // The installer provisions these directories and their machine-wide ACLs.
  // Starting the daemon must not create a user-writable fallback location.
  ensure!(
    directory.is_dir() && common::paths::service_private_dir().is_dir(),
    "Install the GP Connect Windows service before starting it"
  );
  let writer = logging::writer(&directory.join("gpservice.log"))?;
  let redaction = Arc::new(Redaction::new());
  let filter = redaction.clone();
  let logger = env_logger::builder()
    .filter_level(log::LevelFilter::Trace)
    .target(env_logger::Target::Pipe(Box::new(writer)))
    .format(move |buffer, record| {
      writeln!(
        buffer,
        "[{} {:<5} {}] {}",
        buffer.timestamp(),
        record.level(),
        record.module_path().unwrap_or_default(),
        filter.redact_str(&record.args().to_string())
      )
    })
    .build();
  gpapi::logger::init_with_logger(*LOG_LEVEL.get().unwrap_or(&log::Level::Info), logger);
  Ok(redaction)
}
async fn run(handle: &ServiceStatusHandle, stop: CancellationToken, redaction: Arc<Redaction>) -> anyhow::Result<()> {
  let lock = Arc::new(LockFile::new(common::paths::service_lock_file(), std::process::id()));
  let registry = Arc::new(SessionRegistry::new(uuid::Uuid::new_v4()));
  let admission = admission::Admission::new(registry.clone())?;
  let (state, observed) = watch::channel(VpnState::Disconnected);
  let (mut vpn, lifecycle) = VpnTask::new(state, true, registry.clone());
  let dispatcher = Arc::new(RequestDispatcher::new(
    lifecycle,
    Arc::new(AtomicBool::new(false)),
    redaction,
    false,
    true,
  ));
  let websocket = WsServer::new(env!("CARGO_PKG_VERSION"), registry, dispatcher, observed, lock.clone());
  let service_cancel = websocket.cancel_token();
  let vpn_cancel = vpn.cancel_token();
  let (shutdown_tx, mut shutdown_rx) = mpsc::channel(4);
  let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
  let vpn_stop = service_cancel.clone();
  let vpn_handle = tokio::spawn(async move { vpn.start(vpn_stop).await });
  let websocket_shutdown = shutdown_tx.clone();
  let websocket_handle = tokio::spawn(async move { websocket.start(websocket_shutdown, ready_tx).await });
  ready_rx.await.context("Desktop protocol listener failed to start")?;
  let lease_cancel = service_cancel.clone();
  let admission_handle = tokio::spawn(async move {
    if let Err(error) = admission.run(lease_cancel).await {
      log::error!("Desktop admission stopped: {error:#}");
      let _ = shutdown_tx.send(()).await;
    }
  });
  status(handle, ServiceState::Running, false)?;
  tokio::select! { _ = stop.cancelled() => {}, _ = shutdown_rx.recv() => {} }
  let _ = status(handle, ServiceState::StopPending, false);
  vpn_cancel.cancel();
  service_cancel.cancel();
  let _ = tokio::join!(vpn_handle, websocket_handle, admission_handle);
  lock.unlock()?;
  log::info!("gpservice stopped");
  Ok(())
}
