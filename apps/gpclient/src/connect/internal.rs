use std::sync::Arc;

use anyhow::{Context, bail, ensure};
use gpapi::{
  credential::AuthCookieCredential,
  gateway::{Gateway, GatewayLoginContext, GatewaySelection},
  portal::PortalConfig,
  process::collection::CollectionControl,
  session::{
    MaintenanceState, SessionEndReason, SessionMode,
    network::PhysicalNetwork,
    non_tunnel::{SessionCallbacks, SessionExit, maintain_non_tunnel_sessions},
  },
};
use log::{info, warn};

use super::ConnectHandler;

impl ConnectHandler<'_> {
  pub(super) async fn connect_internal_gateways(
    &self,
    portal: &str,
    config: &PortalConfig,
    credential: &AuthCookieCredential,
    first_gateway: &Gateway,
    first_session: super::gateway::GatewayLoginSession,
    remaining: &[&Gateway],
  ) -> anyhow::Result<()> {
    ensure!(remaining.len() < 64, "Too many internal non-tunnel candidates");
    let mut hosts = std::collections::HashSet::new();
    for gateway in std::iter::once(first_gateway).chain(remaining.iter().copied()) {
      ensure!(
        gateway.kind() == gpapi::gateway::GatewayKind::Internal,
        "Non-tunnel candidates must be internal"
      );
      ensure!(
        hosts.insert(gpapi::utils::normalize_server(gateway.server())?),
        "Duplicate internal gateway"
      );
    }
    let mut authenticated = vec![(first_gateway, first_session)];
    for gateway in remaining.iter().copied() {
      self.check_cancelled()?;
      let context =
        GatewayLoginContext::new(gateway, GatewaySelection::Auto).with_connect_method(config.connect_method());
      match self
        .authenticate_gateway(
          portal,
          gateway.server(),
          credential,
          config.default_browser().unwrap_or(false),
          context,
        )
        .await
      {
        Ok(session) => authenticated.push((gateway, session)),
        Err(error) => {
          self.logout_gateway(gateway.server()).await;
          self.check_cancelled()?;
          warn!("Internal gateway {gateway} authentication failed: {error}");
        }
      }
    }
    self.check_cancelled()?;
    let modes = authenticated.iter().map(|(_, session)| session.authentication.mode());
    ensure!(
      internal_session_mode(modes)? == SessionMode::NonTunnel,
      "Expected non-tunnel authentication"
    );
    self.run_internal_session(config, &authenticated).await
  }

  async fn run_internal_session(
    &self,
    config: &PortalConfig,
    authenticated: &[(&gpapi::gateway::Gateway, super::gateway::GatewayLoginSession)],
  ) -> anyhow::Result<()> {
    let cancellation = self.cancellation.clone();
    let network = Arc::new(PhysicalNetwork::capture_controlled(&cancellation).await?);
    let detection = config.internal_detection().cloned();
    let bindings = authenticated
      .iter()
      .map(|(_, session)| {
        session
          .extension_auth
          .binding()
          .cloned()
          .context("Non-tunnel binding is missing")
      })
      .collect::<anyhow::Result<Vec<_>>>()?;
    for binding in &bindings {
      network
        .validate_internal_binding(binding, detection.as_ref(), &cancellation)
        .await?;
    }
    let policy = config
      .internal_session_policy()
      .context("Invalid internal session policy")?;
    let profile = self.os_profile.borrow().clone();
    let source = if policy.collect_hip_data() {
      self.hip_source(profile.clone())?
    } else {
      openconnect::HipSource::Disabled
    };
    let source = Arc::new(source);
    let validate_network = network.clone();
    let validate_detection = detection.clone();
    let callbacks = SessionCallbacks {
      produce_report: Arc::new(move |request, control| {
        control.check()?;
        if matches!(source.as_ref(), openconnect::HipSource::Disabled) {
          return gphip::generate_identity_report(&gphip::ReportInput {
            profile: profile.clone(),
            context: gphip::ReportContext::Connected {
              cookie: request.cookie.clone(),
              client_ip: request.client_ip.clone(),
              client_ipv6: request.client_ipv6.clone(),
              md5: request.md5.clone(),
            },
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
      report_submitted: Arc::new(|_, _| {}),
      status_changed: Arc::new(|member| match member.maintenance {
        MaintenanceState::Healthy => info!(
          "Internal network — authenticated: {} (IPv4: {:?}, IPv6: {:?})",
          member.gateway, member.client_ip, member.client_ipv6
        ),
        state => info!("Internal gateway {}: {state:?}", member.gateway),
      }),
      validate_network: Arc::new(move |cancellation| {
        let network = validate_network.clone();
        let detection = validate_detection.clone();
        let bindings = bindings.clone();
        Box::pin(async move {
          for binding in &bindings {
            network
              .validate_internal_binding(binding, detection.as_ref(), &cancellation)
              .await?;
          }
          Ok(())
        })
      }),
    };
    let monitor_cancel = cancellation.child_token();
    let monitor_token = monitor_cancel.clone();
    let (network_changed, network_signal) = tokio::sync::oneshot::channel();
    let monitor = tokio::spawn(async move {
      match network.wait_for_change(detection, monitor_token).await {
        Ok(false) => {}
        Ok(true) | Err(_) => {
          let _ = network_changed.send(SessionEndReason::NetworkChanged);
        }
      }
    });

    // The PID spans maintenance and cleanup, including initialization before the
    // first successful HIP exchange. The outer handler owns final session logout.
    self.pid_written.store(
      super::gateway::write_pid_file(self.shared_args.lock_file),
      std::sync::atomic::Ordering::SeqCst,
    );
    let mut sessions = self.sessions.take();
    let result = maintain_non_tunnel_sessions(&mut sessions, policy, callbacks, cancellation, async move {
      network_signal.await.unwrap_or(SessionEndReason::NetworkChanged)
    })
    .await;
    self.sessions.replace(sessions);
    monitor_cancel.cancel();
    monitor.await.context("Internal network monitor failed")?;
    match result? {
      SessionExit::Disconnected => Ok(()),
      SessionExit::Failed(reason) => bail!("Internal session ended: {reason:?}. Connect again to authenticate."),
    }
  }

  pub(super) async fn logout_gateway(&self, gateway: &str) {
    let mut sessions = self.sessions.take();
    sessions.logout_gateway(gateway).await;
    self.sessions.replace(sessions);
  }
}

fn internal_session_mode(modes: impl Iterator<Item = SessionMode>) -> anyhow::Result<SessionMode> {
  let mut modes = modes;
  let mode = modes.next().context("No internal gateway authenticated successfully")?;
  ensure!(
    modes.all(|candidate| candidate == mode),
    "Mixed tunnel and non-tunnel internal gateways are unsupported; select a gateway explicitly"
  );
  Ok(mode)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn internal_modes_reject_mixed_and_empty_sets() {
    assert!(internal_session_mode([].into_iter()).is_err());
    assert!(internal_session_mode([SessionMode::Tunnel, SessionMode::NonTunnel].into_iter()).is_err());
    assert_eq!(
      internal_session_mode([SessionMode::NonTunnel, SessionMode::NonTunnel].into_iter()).unwrap(),
      SessionMode::NonTunnel
    );
    assert_eq!(
      internal_session_mode([SessionMode::Tunnel].into_iter()).unwrap(),
      SessionMode::Tunnel
    );
  }
}
