use std::{sync::Arc, time::Duration};

use anyhow::{Context, ensure};
use gpapi::{
  process::collection::{CollectionBudget, CollectionControl},
  service::request::ConnectRequest,
  session::SessionMode,
};
use log::warn;
use openconnect::Vpn;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{LifecycleEvent, LifecycleHandle, PreparedConnection, preparation::ConnectionResources};

pub(super) struct AttemptInput {
  pub attempt: u64,
  pub connection: PreparedConnection,
  pub desktop_uid: Option<u32>,
  pub edited_report: Option<Arc<str>>,
  pub session_id: Option<Uuid>,
  pub brokered_macos: bool,
  pub registry: Arc<crate::session_registry::SessionRegistry>,
  pub lifecycle: LifecycleHandle,
}

struct TunnelInput {
  request: ConnectRequest,
  resources: Arc<ConnectionResources>,
  source: Arc<openconnect::HipSource>,
  session_id: Option<Uuid>,
  registry: Arc<crate::session_registry::SessionRegistry>,
  desktop_uid: Option<u32>,
}

struct PreparedTunnel {
  vpn: Arc<Vpn>,
  script_tun: Option<super::script_tun::PreparedScriptTun>,
}

/// The accepted attempt owns cleanup through fallible setup. Non-tunnel workers
/// borrow that ledger; a native tunnel takes its cleanup ownership when started.
/// Cancellation joins execution before draining any remaining issued sessions.
pub(super) fn spawn(
  mut input: AttemptInput,
  cancellation: CancellationToken,
  events: mpsc::Sender<LifecycleEvent>,
) -> JoinHandle<()> {
  tokio::spawn(async move {
    let attempt = input.attempt;
    let mut failure = None;
    let mut approval = None;
    if !cancellation.is_cancelled() {
      let request = input.connection.request.clone();
      let resources = input.connection.resources.clone();
      let desktop_uid = input.desktop_uid;
      let brokered = input.brokered_macos;
      let edited = input.edited_report.clone();
      let setup_cancel = cancellation.clone();
      let execution = tokio::task::spawn_blocking(move || {
        prepare_execution(request, resources, desktop_uid, brokered, edited, &setup_cancel)
      })
      .await;
      match execution {
        Ok(Ok(execution)) => {
          let (source, guard) = execution.into_parts();
          approval = Some(guard);
          let source = Arc::new(source);
          let guard = approval.as_ref().expect("execution approval is retained");
          if input
            .connection
            .request
            .plan()
            .members()
            .iter()
            .all(|member| member.authentication.mode() == SessionMode::NonTunnel)
          {
            match super::non_tunnel::run(&mut input, source, guard, cancellation.clone(), events.clone()).await {
              Ok(gpapi::session::non_tunnel::SessionExit::Disconnected) => {}
              Ok(gpapi::session::non_tunnel::SessionExit::Failed(reason)) => {
                let message = match reason {
                  gpapi::session::SessionEndReason::InvalidCookie => {
                    "Internal gateway authentication expired; connect again"
                  }
                  gpapi::session::SessionEndReason::NetworkChanged => {
                    "Physical network changed or could not be verified; connect again"
                  }
                  gpapi::session::SessionEndReason::AllMembersFailed => {
                    "All internal gateway sessions failed; connect again"
                  }
                  gpapi::session::SessionEndReason::AuthorizationRevoked => "HIP execution authorization was revoked",
                };
                failure = Some(message.into());
              }
              Err(error) => {
                warn!("Non-tunnel maintenance failed: {error:#}");
                failure = Some("Internal session maintenance failed".into());
              }
            }
          } else {
            let tunnel_input = TunnelInput {
              request: input.connection.request.clone(),
              resources: input.connection.resources.clone(),
              source,
              session_id: input.session_id,
              registry: input.registry.clone(),
              desktop_uid: input.desktop_uid,
            };
            input.connection.request.release_script_tun_bootstrap();
            let setup_cancel = cancellation.clone();
            let prepared = tokio::task::spawn_blocking(move || prepare_tunnel(tunnel_input, &setup_cancel)).await;
            match prepared {
              Ok(Ok(mut prepared)) => {
                if let Some(script_tun) = prepared.script_tun.take() {
                  if let Some(session_id) = input.session_id {
                    match super::script_tun::run(
                      gpapi::script_tun::Identity {
                        service_instance: input.registry.service_instance_id(),
                        attempt,
                        launch_id: match input.connection.request.args().packet_delivery() {
                          gpapi::script_tun::PacketDelivery::ScriptTun(request) => request.launch_id,
                          _ => unreachable!(),
                        },
                      },
                      prepared.vpn.clone(),
                      script_tun,
                      guard,
                      &input.lifecycle,
                      cancellation.clone(),
                      events.clone(),
                      session_id,
                      input.registry.clone(),
                      &mut input.connection.sessions,
                    )
                    .await
                    {
                      Ok(()) => {}
                      Err(error) if error.is::<super::script_tun::NetworkChanged>() && !cancellation.is_cancelled() => {
                        let _ = events.send(LifecycleEvent::ReconnectRequired { attempt }).await;
                      }
                      Err(error) if !cancellation.is_cancelled() => {
                        failure = Some(error.to_string());
                      }
                      Err(_) => {}
                    }
                  }
                } else {
                  run_tunnel(
                    attempt,
                    &prepared,
                    guard,
                    &input.lifecycle,
                    &mut input.connection.sessions,
                    cancellation.clone(),
                    events.clone(),
                  )
                  .await;
                }
              }
              Ok(Err(error)) => {
                failure = Some(format!("Failed to prepare tunnel: {error:#}"));
              }
              Err(error) => {
                failure = Some(format!("Tunnel setup worker failed: {error}"));
              }
            }
          }
        }
        Ok(Err(error)) => {
          warn!("Failed to prepare HIP execution: {error:#}");
          failure = Some("HIP execution could not be prepared".into());
        }
        Err(error) => {
          warn!("HIP setup worker failed: {error}");
          failure = Some("HIP setup worker failed".into());
        }
      }
    }
    input.connection.sessions.logout().await;
    // Identity and approval remain alive through worker drain and logout. Ended
    // only releases admission after the actor also joins this task.
    drop(approval);
    if !cancellation.is_cancelled() {
      if let Some(reason) = failure {
        let _ = events.send(LifecycleEvent::Failed { attempt, reason }).await;
      }
    }
    let _ = events.send(LifecycleEvent::Ended { attempt }).await;
  })
}

fn prepare_execution(
  request: ConnectRequest,
  resources: Arc<ConnectionResources>,
  desktop_uid: Option<u32>,
  brokered: bool,
  edited: Option<Arc<str>>,
  cancellation: &CancellationToken,
) -> anyhow::Result<crate::hip_source::HipExecution> {
  let budget = CollectionBudget::new(Duration::from_secs(60));
  let check = || {
    if cancellation.is_cancelled() {
      return Err(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "HIP setup cancelled",
      ));
    }
    budget.check()
  };
  check()?;
  let args = request.args();
  let pure_non_tunnel = request
    .plan()
    .members()
    .iter()
    .all(|member| member.authentication.mode() == SessionMode::NonTunnel);
  let disabled = pure_non_tunnel && !request.plan().policy().collect_hip_data();
  let source = if disabled {
    &gpapi::hip::HipSource::Disabled
  } else {
    args.hip_source()
  };
  let execution = crate::hip_source::resolve(source, desktop_uid, brokered, edited, Some(resources.profile.clone()))?;
  check()?;
  Ok(execution)
}

fn prepare_tunnel(input: TunnelInput, cancellation: &CancellationToken) -> anyhow::Result<PreparedTunnel> {
  let args = input.request.args();
  let budget = CollectionBudget::new(Duration::from_secs(60));
  let check = || {
    if cancellation.is_cancelled() {
      return Err(std::io::Error::new(
        std::io::ErrorKind::Interrupted,
        "Tunnel setup cancelled",
      ));
    }
    budget.check()
  };
  check()?;
  ensure!(
    input.request.plan().members().len() == 1
      && input.request.plan().members()[0].authentication.mode() == SessionMode::Tunnel,
    "Tunnel execution requires one selected tunnel gateway"
  );
  let member = &input.request.plan().members()[0];
  let identity_files = &input.resources.identity_files;
  let (builder, script_tun) = match args.packet_delivery() {
    gpapi::script_tun::PacketDelivery::System => (
      crate::vpn_script::builder(member.gateway.server(), member.authentication.cookie(), args)?,
      None,
    ),
    gpapi::script_tun::PacketDelivery::ScriptTun(request) => {
      ensure!(input.session_id.is_some(), "Script-tun requires a desktop session");
      let (builder, script_tun) = super::script_tun::builder(
        member.gateway.server(),
        member.authentication.cookie(),
        request,
        input.desktop_uid.context("Desktop identity is unavailable")?,
      )?;
      (builder, Some(script_tun))
    }
  };
  let vpn = Arc::new(
    builder
      .user_agent(args.user_agent())
      .os(args.openconnect_os())
      .os_version(args.os_version())
      .client_version(args.client_version())
      .host_id(args.host_id())
      .certificate(identity_files.as_ref().map(|files| files.certificate().to_owned()))
      .sslkey(identity_files.as_ref().and_then(|files| files.key()).map(str::to_owned))
      .key_password(
        input
          .resources
          .identity
          .as_ref()
          .and_then(|identity| identity.key_password())
          .map(str::to_owned),
      )
      .hip_source(input.source)
      .reconnect_timeout(args.reconnect_timeout())
      .mtu(args.mtu())
      .disable_ipv6(args.disable_ipv6())
      .gateway_ip_preference(match args.gateway_ip_preference() {
        gpapi::gateway::GatewayIpPreference::Auto => openconnect::GatewayIpPreference::Auto,
        gpapi::gateway::GatewayIpPreference::Ipv4 => openconnect::GatewayIpPreference::Ipv4,
        gpapi::gateway::GatewayIpPreference::Ipv6 => openconnect::GatewayIpPreference::Ipv6,
      })
      .no_dtls(args.no_dtls())
      .local_hostname(args.local_hostname())
      .dpd_interval(args.force_dpd())
      .no_xmlpost(args.no_xmlpost())
      .build()
      .context("Failed to create tunnel worker")?,
  );
  check()?;
  if let Some(session_id) = input.session_id {
    let registry = input.registry;
    vpn.set_hip_report_callback(move |xml| {
      if let Err(error) = registry.store_successful_hip_report(session_id, xml) {
        warn!("Could not retain submitted HIP report: {error}");
      }
    });
  }
  Ok(PreparedTunnel { vpn, script_tun })
}

async fn run_tunnel(
  attempt: u64,
  prepared: &PreparedTunnel,
  approval: &crate::hip_source::HipApproval,
  lifecycle: &LifecycleHandle,
  sessions: &mut gpapi::session::GatewaySessions,
  cancellation: CancellationToken,
  events: mpsc::Sender<LifecycleEvent>,
) {
  if cancellation.is_cancelled() {
    return;
  }
  let monitor = gptunnel::TunnelMonitor::attach(prepared.vpn.clone());
  if events
    .send(LifecycleEvent::TunnelMonitor {
      attempt,
      monitor: monitor.clone(),
    })
    .await
    .is_err()
  {
    monitor.stop().await;
    return;
  }
  let vpn = prepared.vpn.clone();
  // The native tunnel keeps its reconnect endpoint and cookie current and owns
  // normal logout. The ledger remains responsible only before native execution.
  // Service admission permits exactly one tunnel member.
  std::mem::take(sessions).relinquish();
  let worker = tokio::task::spawn_blocking(move || {
    vpn.connect(move |session| {
      let _ = events.blocking_send(LifecycleEvent::Established { attempt, session });
    })
  });
  tokio::pin!(worker);
  let mut approval_poll = tokio::time::interval(Duration::from_secs(1));
  let result = loop {
    tokio::select! {
      biased;
      _ = cancellation.cancelled() => { prepared.vpn.disconnect(); break (&mut worker).await; },
      result = &mut worker => break result,
      _ = approval_poll.tick() => if !approval.approval_is_valid() {
        stop_for_revoked_approval(lifecycle, &cancellation);
        prepared.vpn.disconnect();
        break (&mut worker).await;
      },
    }
  };
  match result {
    Ok(0 | -4) => {}
    Ok(code) => warn!("Tunnel worker exited with status {code}"),
    Err(error) => warn!("Tunnel worker failed: {error}"),
  }
  monitor.stop().await;
}

pub(super) fn stop_for_revoked_approval(lifecycle: &LifecycleHandle, cancellation: &CancellationToken) {
  let result = lifecycle.request_disconnect();
  if !matches!(result, gpapi::service::transport::ServiceResult::Accepted) {
    warn!("Could not enqueue approval revocation stop: {result:?}");
  }
  cancellation.cancel();
}
