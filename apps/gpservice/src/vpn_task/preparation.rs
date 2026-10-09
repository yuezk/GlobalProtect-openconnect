use std::{sync::Arc, time::Duration};

use anyhow::Context;
use gpapi::{
  gp_params::GpParams,
  os_profile::OsProfile,
  process::collection::{CollectionBudget, CollectionControl},
  service::request::ConnectRequest,
  session::{GatewaySession, GatewaySessions, network::PhysicalNetwork, transport::GatewayTransport},
};
use tokio_util::sync::CancellationToken;

pub(crate) struct PreparedConnection {
  pub(super) request: ConnectRequest,
  pub(super) resources: Arc<ConnectionResources>,
  pub(super) sessions: GatewaySessions,
}

pub(super) struct ConnectionResources {
  pub profile: OsProfile,
  pub identity: Option<gpapi::utils::request::ClientIdentity>,
  pub identity_files: Option<gpapi::utils::request::ClientIdentityFiles>,
  pub network: Option<PhysicalNetwork>,
}

/// Preparation sends no authenticated gateway requests. Rejection leaves the
/// authentication owner responsible for logout; only queued admission transfers
/// the complete ledger and its retained identity resources to the service.
pub(crate) async fn prepare_connection(
  request: ConnectRequest,
  brokered_macos: bool,
  cancellation: CancellationToken,
) -> anyhow::Result<PreparedConnection> {
  let args = request.args();
  let non_tunnel = request
    .plan()
    .members()
    .iter()
    .all(|member| member.authentication.mode() == gpapi::session::SessionMode::NonTunnel);
  anyhow::ensure!(
    non_tunnel || request.plan().members().len() == 1,
    "Only one selected tunnel gateway can execute"
  );
  for member in request.plan().members() {
    if non_tunnel {
      let binding = member.binding.as_ref().context("Internal gateway binding is missing")?;
      anyhow::ensure!(
        !args.disable_ipv6() || (binding.endpoint().is_ipv4() && binding.addresses().ipv6.is_none()),
        "IPv6 gateway transport or identity is disabled"
      );
    }
  }
  let setup_request = request.clone();
  let setup_cancel = cancellation.clone();
  let resources = tokio::task::spawn_blocking(move || {
    let budget = CollectionBudget::new(Duration::from_secs(60));
    let check = || {
      if setup_cancel.is_cancelled() {
        return Err(std::io::Error::new(
          std::io::ErrorKind::Interrupted,
          "Connection preparation cancelled",
        ));
      }
      budget.check()
    };
    check()?;
    let args = setup_request.args();
    let identity = super::prepare_identity(args, brokered_macos || non_tunnel)?;
    let directory = brokered_macos.then(|| std::path::Path::new("/var/run/com.yuezk.gpgui"));
    let identity_files = identity
      .as_ref()
      .map(|identity| identity.write_files(directory))
      .transpose()?;
    let host = args.host_identity().context("Authenticated host identity is missing")?;
    let profile = crate::hip_source::profile(
      host,
      args.os().context("Authenticated client OS is missing")?,
      args.client_version(),
      args.os_version(),
      args.host_id(),
      args.local_hostname(),
      &check,
    )?;
    let network = if non_tunnel {
      Some(PhysicalNetwork::capture(&check)?)
    } else {
      None
    };
    check()?;
    Ok::<_, anyhow::Error>(Arc::new(ConnectionResources {
      profile,
      identity,
      identity_files,
      network,
    }))
  })
  .await??;
  let args = request.args();
  let mut builder = GpParams::builder(resources.profile.clone());
  builder
    .ignore_tls_errors(args.ignore_tls_errors())
    .client_identity(resources.identity.clone());
  let params = builder.build();
  let mut sessions = Vec::new();
  for member in request.plan().members() {
    let binding = if non_tunnel {
      Some(member.binding.as_ref().context("Internal gateway binding is missing")?)
    } else {
      None
    };
    if let Some(network) = &resources.network {
      network
        .validate_internal_binding(
          binding.context("Internal gateway binding is missing")?,
          request.plan().internal_detection(),
          &cancellation,
        )
        .await?;
    }
    let transport = GatewayTransport::new(member.gateway.server(), member.authentication.clone(), &params, binding)?;
    sessions.push(GatewaySession {
      gateway: member.gateway.clone(),
      transport,
      addresses: binding.map(|binding| binding.addresses()).unwrap_or_default(),
    });
  }
  if let Some(network) = &resources.network {
    network.ensure_current(&cancellation).await?;
  }
  Ok(PreparedConnection {
    request,
    resources,
    sessions: GatewaySessions::new(sessions),
  })
}

#[cfg(test)]
impl PreparedConnection {
  /// Lifecycle-only fixtures own no issued server sessions. Protocol ownership
  /// checks use actual gateway transports rather than this actor-state fixture.
  pub(crate) fn lifecycle_fixture(request: ConnectRequest) -> Self {
    let profile = OsProfile::builder(gpapi::os_profile::ClientOs::Linux)
      .host_identity(gpapi::os_profile::HostIdentity::from_parts(
        "fixture".into(),
        "fixture-host".into(),
        "fixture-serial".into(),
        "02:00:00:00:00:01".into(),
      ))
      .build();
    Self {
      request,
      resources: Arc::new(ConnectionResources {
        profile,
        identity: None,
        identity_files: None,
        network: None,
      }),
      sessions: GatewaySessions::new(vec![]),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use gpapi::{gateway::Gateway, service::vpn_state::ConnectInfo};

  #[tokio::test]
  async fn brokered_macos_and_non_tunnel_identity_reject_path_handoff() {
    for (brokered_macos, non_tunnel) in [(true, false), (false, true), (true, true)] {
      let gateway = Gateway::new("fixture".into(), "gateway.example.com".into());
      let request = super::super::test_connect_request(ConnectInfo::new("portal.example.com".into(), gateway, vec![]))
        .with_certificate(Some("/identity.pem".into()));
      let mut wire = serde_json::to_value(request).unwrap();
      if non_tunnel {
        wire["plan"]["members"][0]["gateway"]["kind"] = serde_json::json!("internal");
        wire["plan"]["members"][0]["authentication"]["connection_type"] = serde_json::json!("internal");
      }
      let request = serde_json::from_value(wire).unwrap();
      let error = prepare_connection(request, brokered_macos, CancellationToken::new())
        .await
        .err()
        .unwrap();
      assert_eq!(error.to_string(), "Client identity must be a protected snapshot");
    }
  }

  #[tokio::test]
  async fn disabled_ipv6_rejects_ipv6_report_identity_before_setup() {
    let gateway = Gateway::new("fixture".into(), "gateway.example.com".into());
    let request = super::super::test_connect_request(ConnectInfo::new("portal.example.com".into(), gateway, vec![]))
      .with_disable_ipv6(true);
    let mut wire = serde_json::to_value(request).unwrap();
    wire["plan"]["members"][0]["gateway"]["kind"] = serde_json::json!("internal");
    wire["plan"]["members"][0]["authentication"]["connection_type"] = serde_json::json!("internal");
    wire["plan"]["members"][0]["binding"]["addresses"]["ipv6"] = serde_json::json!("2001:db8::5");
    let request: ConnectRequest = serde_json::from_value(wire).unwrap();
    // The fixture has no host identity or usable physical interface. Policy
    // rejection must happen before those collectors or transport setup run.
    let error = prepare_connection(request, false, CancellationToken::new())
      .await
      .err()
      .unwrap();
    assert_eq!(error.to_string(), "IPv6 gateway transport or identity is disabled");
  }

  #[tokio::test]
  async fn ordinary_tunnel_preparation_needs_no_physical_binding_or_resolver() {
    let gateway = Gateway::new("fixture".into(), "does-not-resolve.invalid".into());
    let profile = OsProfile::builder(gpapi::os_profile::ClientOs::Linux)
      .host_identity(gpapi::os_profile::HostIdentity::from_parts(
        "fixture".into(),
        "fixture-host".into(),
        "fixture-serial".into(),
        "02:00:00:00:00:01".into(),
      ))
      .build();
    let request = super::super::test_connect_request(ConnectInfo::new("portal.example.com".into(), gateway, vec![]))
      .with_os_profile(&profile)
      .with_disable_ipv6(true);
    let mut wire = serde_json::to_value(request).unwrap();
    wire["plan"]["members"][0]["binding"] = serde_json::Value::Null;
    let request: ConnectRequest = serde_json::from_value(wire).unwrap();
    let prepared = prepare_connection(request, false, CancellationToken::new())
      .await
      .unwrap();
    assert!(prepared.resources.network.is_none());
    assert_eq!(prepared.sessions.members().len(), 1);
    assert!(prepared.sessions.members()[0].addresses.ipv4.is_none());
    assert!(prepared.sessions.members()[0].addresses.ipv6.is_none());
  }
}
