use std::time::{Duration, SystemTime, UNIX_EPOCH};

use common::constants::GP_CLIENT_VERSION;
use gpapi::{
  gateway::{SessionContext, SessionExtensionAuth, extend_session},
  gp_params::ClientOs,
  session::{SessionInfo, SessionRequestArgs, SessionWarning},
};
use log::{info, warn};
use openconnect::VpnSessionInfo;
use tokio::{runtime::Handle, task::JoinHandle};

#[derive(Debug, PartialEq, Eq)]
struct SessionWarningSchedule {
  delay: Duration,
  message: String,
  should_auto_extend: bool,
}

pub(crate) struct SessionContextInput {
  pub(crate) portal: String,
  pub(crate) gateway: String,
  pub(crate) cookie: String,
  pub(crate) user_agent: String,
  pub(crate) os: ClientOs,
  pub(crate) os_version: String,
  pub(crate) client_version: Option<String>,
  pub(crate) certificate: Option<String>,
  pub(crate) sslkey: Option<String>,
  pub(crate) key_password: Option<String>,
  pub(crate) disable_ipv6: bool,
  pub(crate) extension_auth: Option<SessionExtensionAuth>,
}

pub(crate) fn build_session_context(input: SessionContextInput) -> SessionContext {
  let session_args = SessionRequestArgs::new(input.cookie)
    .with_user_agent(Some(input.user_agent))
    .with_os(Some(input.os))
    .with_os_version(Some(input.os_version))
    .with_client_version(Some(
      input.client_version.unwrap_or_else(|| GP_CLIENT_VERSION.to_string()),
    ))
    .with_certificate(input.certificate)
    .with_sslkey(input.sslkey)
    .with_key_password(input.key_password)
    .with_disable_ipv6(input.disable_ipv6);

  let ctx = SessionContext::new(input.gateway, input.portal, session_args);
  match input.extension_auth {
    Some(extension_auth) => ctx.with_extension_auth(extension_auth),
    None => ctx,
  }
}

pub(crate) fn session_info_from_vpn(vpn_session_info: VpnSessionInfo, allow_extend_session: bool) -> SessionInfo {
  SessionInfo::from_vpn_session_fields(
    vpn_session_info.lifetime_secs,
    vpn_session_info.user_expires,
    vpn_session_info.lifetime_warning.map(|warning| SessionWarning {
      prior_secs: warning.prior_secs,
      message: warning.message,
    }),
    allow_extend_session,
  )
}

pub(crate) fn spawn_session_runtime_with_info(
  handle: &Handle,
  session_ctx: SessionContext,
  session_info: SessionInfo,
) -> JoinHandle<()> {
  handle.spawn(run_session_runtime(session_ctx, session_info))
}

async fn run_session_runtime(session_ctx: SessionContext, mut session_info: SessionInfo) {
  loop {
    let Some(schedule) = build_session_warning_schedule(&session_info) else {
      info!("No session warning schedule provided by the gateway");
      return;
    };

    tokio::time::sleep(schedule.delay).await;

    eprintln!("\nWARNING: {}", schedule.message);

    if !schedule.should_auto_extend {
      info!("Session extension is not allowed by the gateway");
      return;
    }

    info!("Attempting to extend the session");
    match extend_session(&session_ctx).await {
      Ok(()) => {
        let Some(next_session_info) = session_info.rescheduled_after_extension() else {
          info!("Session extended, but no lifetime is available to schedule another warning");
          return;
        };

        eprintln!("Session extended.");
        session_info = next_session_info;
      }
      Err(err) => {
        warn!("Failed to extend session: {}", err);
        eprintln!("WARNING: Failed to extend session: {}", err);
        return;
      }
    }
  }
}

fn build_session_warning_schedule(session_info: &SessionInfo) -> Option<SessionWarningSchedule> {
  let warning = session_info.lifetime_warning.as_ref()?;
  let warning_secs = if let Some(user_expires) = session_info.user_expires {
    let now = unix_timestamp();
    user_expires.saturating_sub(warning.prior_secs).saturating_sub(now)
  } else if let Some(lifetime_secs) = session_info.lifetime_secs {
    lifetime_secs.saturating_sub(warning.prior_secs)
  } else {
    return None;
  };

  Some(SessionWarningSchedule {
    delay: Duration::from_secs(warning_secs as u64),
    message: warning.message.clone(),
    should_auto_extend: session_info.allow_extend_session,
  })
}

fn unix_timestamp() -> u32 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as u32
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample_session_info(user_expires: Option<u32>, allow_extend_session: bool) -> SessionInfo {
    SessionInfo {
      user_expires,
      lifetime_warning: Some(SessionWarning {
        prior_secs: 1800,
        message: "Session expires soon".to_string(),
      }),
      allow_extend_session,
      ..Default::default()
    }
  }

  #[test]
  fn builds_warning_schedule_for_auto_extend() {
    let session_info = sample_session_info(Some(unix_timestamp() + 1830), true);

    let schedule = build_session_warning_schedule(&session_info).unwrap();

    assert_eq!(schedule.message, "Session expires soon");
    assert!(schedule.delay <= Duration::from_secs(30));
    assert!(schedule.should_auto_extend);
  }

  #[test]
  fn builds_warning_schedule_without_auto_extend() {
    let session_info = sample_session_info(Some(unix_timestamp() + 1830), false);

    let schedule = build_session_warning_schedule(&session_info).unwrap();

    assert!(!schedule.should_auto_extend);
  }

  #[test]
  fn schedules_next_warning_after_extension_from_lifetime() {
    let mut session_info = sample_session_info(Some(unix_timestamp() + 1830), true);
    session_info.lifetime_secs = Some(7200);

    let next_session_info = session_info.rescheduled_after_extension().unwrap();
    let schedule = build_session_warning_schedule(&next_session_info).unwrap();

    assert!(schedule.delay <= Duration::from_secs(5400));
  }

  #[test]
  fn builds_session_context_from_cli_runtime_values() {
    let ctx = build_session_context(SessionContextInput {
      portal: "portal.example.com".to_string(),
      gateway: "vpn.example.com".to_string(),
      cookie: "authcookie=AUTH".to_string(),
      user_agent: "UA".to_string(),
      os: ClientOs::Mac,
      os_version: "macOS 15.0".to_string(),
      client_version: Some("6.3.1-12".to_string()),
      certificate: Some("/tmp/client.pem".to_string()),
      sslkey: Some("/tmp/client.key".to_string()),
      key_password: Some("secret".to_string()),
      disable_ipv6: true,
      extension_auth: None,
    });

    assert_eq!(ctx.portal(), "portal.example.com");
    assert_eq!(ctx.server(), "vpn.example.com");
    assert_eq!(ctx.session_args().cookie(), "authcookie=AUTH");
    assert_eq!(ctx.session_args().user_agent().as_deref(), Some("UA"));
    assert!(matches!(ctx.session_args().os(), Some(ClientOs::Mac)));
    assert_eq!(ctx.session_args().os_version().as_deref(), Some("macOS 15.0"));
    assert_eq!(ctx.session_args().client_version().as_deref(), Some("6.3.1-12"));
    assert_eq!(ctx.session_args().certificate().as_deref(), Some("/tmp/client.pem"));
    assert_eq!(ctx.session_args().sslkey().as_deref(), Some("/tmp/client.key"));
    assert_eq!(ctx.session_args().key_password().as_deref(), Some("secret"));
    assert!(ctx.session_args().disable_ipv6());
  }

  #[test]
  fn maps_openconnect_session_metadata_to_runtime_session_info() {
    let info = session_info_from_vpn(
      VpnSessionInfo {
        lifetime_secs: Some(43_200),
        user_expires: Some(1_776_828_409),
        lifetime_warning: Some(openconnect::VpnSessionWarning {
          prior_secs: 1_800,
          message: "Session expires soon".to_string(),
        }),
      },
      true,
    );

    assert_eq!(info.lifetime_secs, Some(43_200));
    assert_eq!(info.user_expires, Some(1_776_828_409));
    assert_eq!(info.lifetime_warning.unwrap().prior_secs, 1_800);
    assert!(info.allow_extend_session);
  }

  #[test]
  fn direct_gateway_session_info_does_not_allow_extension_without_portal_policy() {
    let info = session_info_from_vpn(
      VpnSessionInfo {
        lifetime_secs: Some(43_200),
        user_expires: Some(1_776_828_409),
        lifetime_warning: Some(openconnect::VpnSessionWarning {
          prior_secs: 1_800,
          message: "Session expires soon".to_string(),
        }),
      },
      false,
    );

    assert_eq!(info.lifetime_secs, Some(43_200));
    assert_eq!(info.user_expires, Some(1_776_828_409));
    assert_eq!(info.lifetime_warning.unwrap().prior_secs, 1_800);
    assert!(!info.allow_extend_session);
  }
}
