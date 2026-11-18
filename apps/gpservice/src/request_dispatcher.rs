use std::sync::{Arc, atomic::AtomicBool};

use gpapi::{
  service::{
    request::{UpdateGuiRequest, UpdateLogLevelRequest, WsRequest},
    transport::{ServiceErrorCode, ServiceResult},
  },
  utils::redact::Redaction,
};
use log::{info, warn};
#[cfg(unix)]
use tokio::sync::Mutex;

pub(crate) struct SessionAuthority {
  pub registry: Arc<crate::session_registry::SessionRegistry>,
  pub session_id: uuid::Uuid,
  pub generation: u64,
}

pub(crate) enum PendingDispatch {
  Connect {
    preparation: ConnectionPreparation,
    request: Box<gpapi::service::request::ConnectRequest>,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
  },
  Drain(Option<crate::vpn_task::AttemptDrain>),
  Other(WsRequest),
}

pub(crate) struct ConnectionPreparation {
  authority: SessionAuthority,
  non_tunnel: bool,
  reservation: Option<crate::vpn_task::ConnectReservation>,
}

impl ConnectionPreparation {
  fn cancellation(&self) -> tokio_util::sync::CancellationToken {
    self
      .reservation
      .as_ref()
      .expect("preparation retains reservation")
      .cancellation()
  }

  fn commit(
    mut self,
    connection: crate::vpn_task::PreparedConnection,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
  ) -> ServiceResult {
    let reservation = self.reservation.take().expect("preparation retains reservation");
    self.authority.registry.commit_connection(
      self.authority.session_id,
      self.authority.generation,
      self.non_tunnel,
      || reservation.commit(connection, desktop_uid, edited_report, Some(self.authority.session_id)),
    )
  }
}

pub struct RequestDispatcher {
  lifecycle: crate::vpn_task::LifecycleHandle,
  gui_restart_requested: Arc<AtomicBool>,
  redaction: Arc<Redaction>,
  #[cfg(unix)]
  install_lock: Mutex<()>,
  gui_management_enabled: bool,
  restricted_desktop: bool,
}

impl RequestDispatcher {
  #[cfg(windows)]
  pub(crate) fn patch_checks(&self) -> Arc<crate::windows_patches::PatchChecks> {
    self.lifecycle.patch_checks.clone()
  }
  pub fn subscribe_tunnel(&self) -> tokio::sync::watch::Receiver<crate::vpn_task::TunnelBinding> {
    self.lifecycle.subscribe_tunnel()
  }
  pub fn new(
    lifecycle: crate::vpn_task::LifecycleHandle,
    gui_restart_requested: Arc<AtomicBool>,
    redaction: Arc<Redaction>,
    gui_management_enabled: bool,
    restricted_desktop: bool,
  ) -> Self {
    Self {
      lifecycle,
      gui_restart_requested,
      redaction,
      #[cfg(unix)]
      install_lock: Mutex::new(()),
      gui_management_enabled,
      restricted_desktop,
    }
  }

  pub fn dispatch_lifecycle(
    &self,
    request: WsRequest,
    desktop_uid: Option<u32>,
    edited_report: Option<Arc<str>>,
    authority: Option<SessionAuthority>,
  ) -> Result<PendingDispatch, ServiceResult> {
    match request {
      WsRequest::Connect(request) => {
        if let Err(message) = self.validate_connect_request(&request) {
          return Err(ServiceResult::rejected(ServiceErrorCode::InvalidRequest, message));
        }
        let redaction_values = request
          .plan()
          .members()
          .iter()
          .flat_map(|member| [member.gateway.server(), member.authentication.cookie()])
          .collect::<Vec<_>>();
        if let Err(err) = self.redaction.add_values(&redaction_values) {
          warn!("Failed to update log redaction: {err}");
          return Err(ServiceResult::rejected(
            ServiceErrorCode::Internal,
            "Request could not be accepted",
          ));
        }
        let authority = authority
          .ok_or_else(|| ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Connection session is missing"))?;
        let non_tunnel = request
          .plan()
          .members()
          .iter()
          .all(|member| member.authentication.mode() == gpapi::session::SessionMode::NonTunnel);
        let reservation =
          authority
            .registry
            .reserve_connection(authority.session_id, authority.generation, non_tunnel, || {
              self
                .lifecycle
                .reserve_connect(request.restart_of().map(|attempt| (attempt, authority.session_id)))
            })?;
        let preparation = ConnectionPreparation {
          authority,
          non_tunnel,
          reservation: Some(reservation),
        };
        let edited_report = if let gpapi::hip::HipSource::Edited { report_id } = request.args().hip_source() {
          let report_id = uuid::Uuid::parse_str(report_id)
            .map_err(|_| ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Invalid HIP report ID"))?;
          Some(
            preparation
              .authority
              .registry
              .edited_hip_report(preparation.authority.session_id, report_id)
              .map_err(|_| {
                ServiceResult::rejected(
                  ServiceErrorCode::InvalidRequest,
                  "HIP report is unavailable for this session",
                )
              })?,
          )
        } else {
          edited_report
        };
        Ok(PendingDispatch::Connect {
          preparation,
          request,
          desktop_uid,
          edited_report,
        })
      }
      WsRequest::StopNonTunnelAttachment { connection_id } => {
        let authority = authority
          .ok_or_else(|| ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "Connection session is missing"))?;
        let drain =
          authority
            .registry
            .stop_non_tunnel_attachment(authority.session_id, authority.generation, connection_id)?;
        Ok(PendingDispatch::Drain(drain))
      }
      WsRequest::Disconnect(_) => Err(self.lifecycle.request_disconnect()),
      request => Ok(PendingDispatch::Other(request)),
    }
  }

  #[cfg(test)]
  pub async fn dispatch(&self, request: WsRequest) -> ServiceResult {
    match self.dispatch_lifecycle(request, None, None, None) {
      Ok(request) => self.dispatch_pending(request).await,
      Err(result) => result,
    }
  }

  pub(crate) async fn dispatch_pending(&self, request: PendingDispatch) -> ServiceResult {
    let request = match request {
      PendingDispatch::Connect {
        preparation,
        request,
        desktop_uid,
        edited_report,
      } => {
        let prepared =
          crate::vpn_task::prepare_connection(*request, self.restricted_desktop, preparation.cancellation()).await;
        return match prepared {
          Ok(connection) => preparation.commit(connection, desktop_uid, edited_report),
          Err(error) => {
            warn!("Connection preparation failed: {error:#}");
            ServiceResult::rejected(
              ServiceErrorCode::InvalidRequest,
              "Connection resources could not be prepared",
            )
          }
        };
      }
      PendingDispatch::Drain(drain) => {
        if let Some(drain) = drain {
          drain.wait().await;
        }
        return ServiceResult::Accepted;
      }
      PendingDispatch::Other(request) => request,
    };

    match request {
      WsRequest::RestartGui => {
        if !self.gui_management_enabled {
          return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "GUI restart is unavailable on macOS");
        }
        info!("GUI restart requested");
        self
          .gui_restart_requested
          .store(true, std::sync::atomic::Ordering::SeqCst);
        ServiceResult::Accepted
      }
      WsRequest::UpdateGui(request) => {
        if !self.gui_management_enabled {
          return ServiceResult::rejected(ServiceErrorCode::InvalidRequest, "GUI update is unavailable on macOS");
        }
        self.update_gui(request).await
      }
      WsRequest::GetVpncScriptMetadata => {
        #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
        {
          match gpservice::vpnc_script::metadata() {
            Ok(metadata) => ServiceResult::VpncScriptMetadata(metadata),
            Err(err) => {
              warn!("Failed to read VPNC script metadata: {err}");
              ServiceResult::rejected(ServiceErrorCode::Internal, "Failed to read VPNC script metadata")
            }
          }
        }
        #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
        {
          ServiceResult::VpncScriptMetadata(None)
        }
      }
      WsRequest::UpdateLogLevel(UpdateLogLevelRequest(level)) => {
        let level = level.parse().unwrap_or(log::Level::Info);
        info!("Updating log level to: {level}");
        if let Err(err) = gpapi::logger::set_max_level(level) {
          warn!("Failed to update log level: {err}");
        }
        ServiceResult::Accepted
      }
      WsRequest::Connect(_)
      | WsRequest::StopNonTunnelAttachment { .. }
      | WsRequest::Disconnect(_)
      | WsRequest::StoreEditedHipReportChunk(_)
      | WsRequest::PreviewHipReport(_)
      | WsRequest::GetHipApprovalStatus { .. }
      | WsRequest::ReadHipPreviewChunk(_)
      | WsRequest::GetLastSubmittedHipReport
      | WsRequest::SetTunnelStatisticsEnabled { .. }
      | WsRequest::SetWindowsPatchInterest { .. }
      | WsRequest::RefreshWindowsPatches { .. } => {
        unreachable!("session-bound request was already dispatched")
      }
    }
  }

  fn validate_connect_request(&self, request: &gpapi::service::request::ConnectRequest) -> Result<(), &'static str> {
    if !self.restricted_desktop {
      return Ok(());
    }
    if request.args().vpnc_script().is_some() {
      return Err("Custom VPN scripts are unavailable on this platform");
    }
    if matches!(
      request.args().hip_source(),
      gpapi::hip::HipSource::UserScript { .. } | gpapi::hip::HipSource::ApprovedRootScript { .. }
    ) {
      return Err("Custom HIP scripts are unavailable on this platform");
    }
    if request.args().certificate().is_some() || request.args().sslkey().is_some() {
      return Err("Client identity must be provided as certificate data");
    }
    Ok(())
  }

  #[cfg(unix)]
  async fn update_gui(&self, request: UpdateGuiRequest) -> ServiceResult {
    if request.variant == gpapi::gui::GuiVariant::Nsh && !cfg!(target_os = "linux") {
      return ServiceResult::rejected(
        ServiceErrorCode::InvalidRequest,
        "Native Shell is available only on Linux",
      );
    }
    let _install = self.install_lock.lock().await;
    let path = request.path;
    let checksum = request.checksum;
    let target = request.variant.update_target();
    let installed = tokio::task::spawn_blocking(move || {
      crate::gui_installation::install(std::path::Path::new(&path), &checksum, &target)
    })
    .await;
    match installed {
      Ok(Ok(())) => ServiceResult::Accepted,
      Ok(Err(crate::gui_installation::InstallError::Checksum(err))) => {
        warn!("Failed to verify GUI update checksum: {err}");
        ServiceResult::rejected(ServiceErrorCode::ChecksumFailed, "GUI update checksum failed")
      }
      Ok(Err(crate::gui_installation::InstallError::Install(err))) => {
        warn!("Failed to install GUI update: {err}");
        ServiceResult::rejected(ServiceErrorCode::InstallFailed, "GUI update installation failed")
      }
      Err(err) => {
        warn!("GUI update task failed: {err}");
        ServiceResult::rejected(ServiceErrorCode::InstallFailed, "GUI update installation failed")
      }
    }
  }
  #[cfg(windows)]
  async fn update_gui(&self, _request: UpdateGuiRequest) -> ServiceResult {
    ServiceResult::rejected(
      ServiceErrorCode::InvalidRequest,
      "Use the Windows installer to update GP Connect",
    )
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::vpn_task::test_connect_request as test_request;
  use gpapi::gateway::Gateway;
  #[tokio::test]
  async fn brokered_mode_rejects_gui_management_and_root_readable_identity_paths() {
    let dispatcher = RequestDispatcher::new(
      crate::vpn_task::LifecycleHandle::for_tests(),
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      false,
      true,
    );
    let ServiceResult::Rejected(rejection) = dispatcher.dispatch(WsRequest::RestartGui).await else {
      panic!("brokered mode accepted GUI restart");
    };
    assert_eq!(rejection.code(), ServiceErrorCode::InvalidRequest);

    let gateway = Gateway::new("Gateway".into(), "vpn.example.com".into());
    let info = gpapi::service::vpn_state::ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![gateway]);
    let request = test_request(info).with_certificate(Some("/Users/example/client.pem".to_string()));
    let ServiceResult::Rejected(rejection) = dispatcher.dispatch(WsRequest::Connect(Box::new(request))).await else {
      panic!("brokered mode accepted a root-readable identity path");
    };
    assert_eq!(rejection.code(), ServiceErrorCode::InvalidRequest);
  }

  #[test]
  fn brokered_mode_rejects_script_overrides() {
    let dispatcher = RequestDispatcher::new(
      crate::vpn_task::LifecycleHandle::for_tests(),
      Arc::new(AtomicBool::new(false)),
      Arc::new(Redaction::new()),
      false,
      true,
    );
    let gateway = Gateway::new("Gateway".into(), "vpn.example.com".into());
    let info = gpapi::service::vpn_state::ConnectInfo::new("portal.example.com".into(), gateway.clone(), vec![gateway]);
    let embedded = test_request(info.clone());
    assert_eq!(dispatcher.validate_connect_request(&embedded), Ok(()));

    for script in [
      "/app/Contents/Resources/Scripts/vpnc-script",
      "/tmp/custom-vpnc-script",
      "echo untrusted",
      "",
    ] {
      let custom = test_request(info.clone()).with_vpnc_script(Some(script.to_string()));
      assert_eq!(
        dispatcher.validate_connect_request(&custom),
        Err("Custom VPN scripts are unavailable on this platform")
      );
    }

    let custom = test_request(info).with_hip_source(gpapi::hip::HipSource::UserScript {
      path: "/tmp/hip.sh".into(),
    });
    assert_eq!(
      dispatcher.validate_connect_request(&custom),
      Err("Custom HIP scripts are unavailable on this platform")
    );
  }
}
