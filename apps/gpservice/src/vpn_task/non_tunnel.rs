use std::{sync::Arc, time::Duration};

use anyhow::Context;
use gpapi::process::collection::CollectionControl;
use gpapi::session::{
  GatewaySessionSummary, MaintenanceState, SessionEndReason,
  non_tunnel::{SessionCallbacks, SessionExit, maintain_non_tunnel_sessions},
};
use log::warn;
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;

use super::{
  LifecycleEvent,
  runtime::{AttemptInput, stop_for_revoked_approval},
};

/// Borrow the accepted ledger. The outer attempt owner retains identity/source
/// resources and joins this runtime before draining the remaining members.
pub(super) async fn run(
  input: &mut AttemptInput,
  source: Arc<openconnect::HipSource>,
  approval: &crate::hip_source::HipApproval,
  cancellation: CancellationToken,
  events: mpsc::Sender<LifecycleEvent>,
) -> anyhow::Result<SessionExit> {
  let request = &input.connection.request;
  let args = request.args();
  let resources = &input.connection.resources;
  let sessions = &mut input.connection.sessions;
  let network = resources
    .network
    .as_ref()
    .context("Physical network context is missing")?;
  let posture_enabled =
    request.plan().policy().collect_hip_data() && !matches!(args.hip_source(), gpapi::hip::HipSource::Disabled);
  let profile = resources.profile.clone();
  let initial = sessions
    .members()
    .iter()
    .map(|session| GatewaySessionSummary {
      gateway: session.gateway.clone(),
      mode: gpapi::session::SessionMode::NonTunnel,
      client_ip: session.addresses.ipv4.map(|ip| ip.to_string()),
      client_ipv6: session.addresses.ipv6.map(|ip| ip.to_string()),
      maintenance: MaintenanceState::Authenticated,
    })
    .collect::<Vec<_>>();
  let (status, mut changed) = watch::channel(initial);
  let registry = input.registry.clone();
  let session_id = input.session_id;
  let validation_network = network.clone();
  let validation_detection = request.plan().internal_detection().cloned();
  let callbacks = SessionCallbacks {
    validate_network: Arc::new(move |cancel| {
      let network = validation_network.clone();
      let detection = validation_detection.clone();
      Box::pin(async move { network.validate_internal(detection.as_ref(), &cancel).await })
    }),
    produce_report: Arc::new(move |request, control| {
      control.check()?;
      let context = gphip::ReportContext::Connected {
        cookie: request.cookie.clone(),
        client_ip: request.client_ip.clone(),
        client_ipv6: request.client_ipv6.clone(),
        md5: request.md5.clone(),
      };
      if !posture_enabled {
        return gphip::generate_identity_report(&gphip::ReportInput {
          profile: profile.clone(),
          context,
        })
        .map_err(std::io::Error::other);
      }
      source.collect(
        &openconnect::HipRequest {
          cookie: request.cookie.clone(),
          client_ip: request.client_ip.clone(),
          client_ipv6: request.client_ipv6.clone(),
          md5: request.md5.clone(),
          client_version: profile.client_version().into(),
          client_os: profile.client_os().as_str().into(),
          os_version: profile.os_version().into(),
          host_id: Some(profile.host_id().into()),
          local_hostname: Some(profile.computer().into()),
        },
        &|| control.check(),
      )
    }),
    report_submitted: Arc::new(move |_, xml| {
      if let Some(session_id) = session_id {
        if let Err(error) = registry.store_successful_hip_report(session_id, xml) {
          warn!("Could not retain submitted HIP report: {error}");
        }
      }
    }),
    status_changed: Arc::new(move |summary| {
      status.send_modify(|members| {
        if let Some(member) = members
          .iter_mut()
          .find(|member| member.gateway.server() == summary.gateway.server())
        {
          *member = summary;
        }
      });
    }),
  };
  let monitor_cancel = cancellation.child_token();
  let monitor_token = monitor_cancel.clone();
  let network = network.clone();
  let detection = request.plan().internal_detection().cloned();
  let (network_changed, network_signal) = tokio::sync::oneshot::channel();
  let monitor = tokio::spawn(async move {
    match network.wait_for_change(detection, monitor_token).await {
      Ok(false) => {}
      Ok(true) => {
        let _ = network_changed.send(SessionEndReason::NetworkChanged);
      }
      Err(error) => {
        warn!("Physical network inspection failed: {error}");
        let _ = network_changed.send(SessionEndReason::NetworkChanged);
      }
    }
  });
  let mut approval_revoked = false;
  let result = {
    let lifecycle_cancel = cancellation.clone();
    let maintenance = maintain_non_tunnel_sessions(
      sessions,
      request.plan().policy(),
      callbacks,
      lifecycle_cancel,
      async move { network_signal.await.unwrap_or(SessionEndReason::NetworkChanged) },
    );
    tokio::pin!(maintenance);
    let mut approval_poll = tokio::time::interval(Duration::from_secs(1));
    let mut publish = true;
    let result = loop {
      if publish && !cancellation.is_cancelled() {
        let members = changed.borrow_and_update().clone();
        let _ = events
          .send(LifecycleEvent::MembersChanged {
            attempt: input.attempt,
            members,
            failures: request.plan().failures().to_vec(),
          })
          .await;
        publish = false;
      }
      tokio::select! {
        biased;
        result = &mut maintenance => break result,
        _ = approval_poll.tick() => if !approval.approval_is_valid() {
          approval_revoked = true;
          stop_for_revoked_approval(&input.lifecycle, &cancellation);
        },
        result = changed.changed() => { if result.is_ok() { publish = true; } },
      }
    };
    // Maintenance can invalidate every member and finish in the same poll.
    // Forward that last snapshot before monitor/remote cleanup, even when the
    // completed maintenance branch won over the watch notification.
    if !cancellation.is_cancelled() {
      let members = changed.borrow_and_update().clone();
      let _ = events
        .send(LifecycleEvent::MembersChanged {
          attempt: input.attempt,
          members,
          failures: request.plan().failures().to_vec(),
        })
        .await;
    }
    result
  };
  monitor_cancel.cancel();
  monitor.await.context("Network monitor worker failed")?;
  if approval_revoked {
    let _ = events
      .send(LifecycleEvent::Failed {
        attempt: input.attempt,
        reason: "HIP script approval was revoked".into(),
      })
      .await;
  }
  // resources and approval remain owned until maintenance/logout and monitor
  // inspection are drained. Deliberate disconnect wins throughout cleanup.
  if cancellation.is_cancelled() {
    Ok(SessionExit::Disconnected)
  } else {
    result
  }
}
