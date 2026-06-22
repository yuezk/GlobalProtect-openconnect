use std::{
  fs,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
};

use gpapi::{
  cookie_store,
  credential::{AuthCookieCredential, Credential},
  gateway::{GatewayLogin, GatewayLoginContext, SessionExtensionAuth, gateway_login, gateway_login_with_context},
  gp_params::GpParams,
  os_profile::OsProfile,
  portal::prelogin,
  process::users::{get_non_root_user, get_user_by_name},
  utils::shutdown_signal,
};
use inquire::Text;
use log::{info, warn};
use openconnect::{Vpn, VpnBuilder};
use tokio::{runtime::Handle, task::JoinHandle};

use crate::{
  GP_CLIENT_LOCK_FILE,
  session::{SessionContextInput, build_session_context, session_info_from_vpn, spawn_session_runtime_with_info},
};

use super::{ConnectHandler, args::cookie_cache_path};

struct GatewayLoginSession {
  cookie: String,
  extension_auth: SessionExtensionAuth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayConnectFailureStage {
  BeforeTunnel,
  AfterTunnel,
}

#[derive(Debug)]
pub(super) struct GatewayConnectError {
  stage: GatewayConnectFailureStage,
  error: anyhow::Error,
}

impl GatewayConnectError {
  fn before_tunnel(error: anyhow::Error) -> Self {
    Self {
      stage: GatewayConnectFailureStage::BeforeTunnel,
      error,
    }
  }

  fn after_tunnel(error: anyhow::Error) -> Self {
    Self {
      stage: GatewayConnectFailureStage::AfterTunnel,
      error,
    }
  }

  pub(super) fn is_before_tunnel(&self) -> bool {
    self.stage == GatewayConnectFailureStage::BeforeTunnel
  }

  pub(super) fn as_error(&self) -> &anyhow::Error {
    &self.error
  }

  pub(super) fn into_error(self) -> anyhow::Error {
    self.error
  }
}

impl ConnectHandler<'_> {
  pub(super) async fn try_cached_cookie(&self, server: &str) -> Option<()> {
    let path = cookie_cache_path(self.args)?;
    let host_id = self.os_profile.borrow().host_identity().host_id().to_string();
    let stored = cookie_store::load(&path, server, &host_id)?;

    if !stored.auth_cookie.can_authenticate_gateway() {
      warn!(
        "Cached portal cookie for {} is not usable for gateway authentication. Clearing cache.",
        stored.server
      );
      cookie_store::clear(&path);
      return None;
    }

    info!(
      "Using cached portal cookie for {} (saved_at={}, gateway={})",
      stored.server, stored.saved_at, stored.last_gateway
    );

    let cred: Credential = (&stored.auth_cookie).into();
    let mut gp_params = self.build_gp_params();
    gp_params.set_is_gateway(true);

    let login_session = match self.login_gateway(&stored.last_gateway, &cred, &gp_params, None).await {
      Ok(session) => session,
      Err(err) => {
        warn!(
          "Cached portal cookie rejected by gateway {}: {}. Clearing cache and falling back to portal auth.",
          stored.last_gateway, err
        );
        cookie_store::clear(&path);
        return None;
      }
    };

    match self
      .connect_gateway(
        server,
        &stored.last_gateway,
        &login_session.cookie,
        false,
        login_session.extension_auth,
      )
      .await
    {
      Ok(()) => Some(()),
      Err(err) => {
        warn!("Gateway connect failed after cached-cookie login: {}", err.as_error());
        None
      }
    }
  }

  pub(super) async fn connect_gateway_with_prelogin(
    &self,
    portal: &str,
    gateway: &str,
    allow_extend_session: bool,
    gateway_context: Option<GatewayLoginContext>,
  ) -> anyhow::Result<()> {
    info!("Performing the gateway authentication...");

    let mut gp_params = self.build_gp_params();
    gp_params.set_is_gateway(true);

    let gateway_external_browser_allowed = true;
    let prelogin = prelogin(gateway, &gp_params, self.direct_gateway_prelogin_options()).await?;
    let cred = self
      .obtain_credential(&prelogin, gateway, gateway_external_browser_allowed)
      .await?;

    let login_session = self
      .login_gateway(gateway, &cred, &gp_params, gateway_context.as_ref())
      .await?;

    self
      .connect_gateway(
        portal,
        gateway,
        &login_session.cookie,
        allow_extend_session,
        login_session.extension_auth,
      )
      .await
      .map_err(GatewayConnectError::into_error)
  }

  /// Connect to a gateway using the official client's auth flow:
  ///
  /// 1. Call gateway prelogin and retain the response (SAML data is held as a fallback).
  /// 2. If the portal provided auth cookies, attempt gateway login using them.
  /// 3. If that login succeeds, proceed directly to the VPN connection.
  /// 4. If no portal auth cookies exist or portal-cookie login fails, use the retained gateway prelogin response to authenticate
  ///    against the gateway (SAML or username/password) and obtain a gateway-issued
  ///    credential.
  /// 5. Retry gateway login with the gateway credential (now carrying the gateway's
  ///    `prelogin-cookie`) and connect on success.
  pub(super) async fn connect_gateway_with_fallback(
    &self,
    portal: &str,
    gateway: &str,
    portal_cred: &AuthCookieCredential,
    allow_extend_session: bool,
    portal_default_browser_enabled: bool,
    gateway_context: GatewayLoginContext,
  ) -> Result<(), GatewayConnectError> {
    info!("Connecting to gateway with portal-cookie first, gateway prelogin fallback...");

    let mut gp_params = self.build_gp_params();
    gp_params.set_is_gateway(true);

    let gateway_prelogin = prelogin(
      gateway,
      &gp_params,
      self.prelogin_options(portal_default_browser_enabled),
    )
    .await
    .map_err(GatewayConnectError::before_tunnel)?;

    if portal_cred.can_authenticate_gateway() {
      let portal_cred_for_login: Credential = portal_cred.into();
      match self
        .login_gateway(gateway, &portal_cred_for_login, &gp_params, Some(&gateway_context))
        .await
      {
        Ok(login_session) => {
          info!("Gateway login with portal auth cookies succeeded");
          self.save_cookie_cache(portal, gateway, portal_cred);
          return self
            .connect_gateway(
              portal,
              gateway,
              &login_session.cookie,
              allow_extend_session,
              login_session.extension_auth,
            )
            .await;
        }
        Err(err) => {
          info!("Gateway login with portal auth cookies failed: {}", err);
          info!("Falling back to gateway prelogin authentication...");
        }
      }
    } else {
      info!("Portal config did not provide gateway auth cookies; using gateway prelogin flow");
    }

    let gateway_cred = match self
      .obtain_credential(&gateway_prelogin, gateway, portal_default_browser_enabled)
      .await
    {
      Ok(cred) => cred,
      Err(err) => {
        self.print_direct_gateway_recommendation(gateway);
        return Err(GatewayConnectError::before_tunnel(err));
      }
    };

    let login_session = match self
      .login_gateway(gateway, &gateway_cred, &gp_params, Some(&gateway_context))
      .await
    {
      Ok(login_session) => login_session,
      Err(err) => {
        self.print_direct_gateway_recommendation(gateway);
        return Err(GatewayConnectError::before_tunnel(err));
      }
    };

    self
      .connect_gateway(
        portal,
        gateway,
        &login_session.cookie,
        allow_extend_session,
        login_session.extension_auth,
      )
      .await
  }

  fn save_cookie_cache(&self, portal: &str, gateway: &str, auth_cookie: &AuthCookieCredential) {
    let Some(path) = cookie_cache_path(self.args) else {
      return;
    };

    if !auth_cookie.can_authenticate_gateway() {
      return;
    }

    let stored = cookie_store::StoredCookie::new(
      portal.to_string(),
      auth_cookie.username().to_string(),
      self.os_profile.borrow().host_identity().host_id().to_string(),
      gateway.to_string(),
      auth_cookie.clone(),
    );

    match cookie_store::save(&path, &stored) {
      Ok(()) => info!("Saved portal cookie cache to {}", path.display()),
      Err(err) => warn!("Failed to save portal cookie cache to {}: {}", path.display(), err),
    }
  }

  fn print_direct_gateway_recommendation(&self, gateway: &str) {
    if !self.args.cookie_on_stdin {
      return;
    }

    eprintln!("\nNOTE: Gateway authentication failed after portal login.");
    eprintln!("NOTE: If this server also accepts direct gateway login, try:");
    eprintln!("NOTE: {}", direct_gateway_command(gateway));
  }

  async fn login_gateway(
    &self,
    gateway: &str,
    cred: &Credential,
    gp_params: &GpParams,
    gateway_context: Option<&GatewayLoginContext>,
  ) -> anyhow::Result<GatewayLoginSession> {
    let mut gp_params = gp_params.clone();

    loop {
      let login = match gateway_context {
        Some(context) => gateway_login_with_context(gateway, cred, &gp_params, context).await?,
        None => gateway_login(gateway, cred, &gp_params).await?,
      };

      match login {
        GatewayLogin::Cookie(cookie) => {
          return Ok(GatewayLoginSession {
            cookie,
            extension_auth: SessionExtensionAuth::new(cred.clone(), gp_params),
          });
        }
        GatewayLogin::Mfa(message, input_str) => {
          let otp = Text::new(&message).prompt()?;
          gp_params.set_input_str(&input_str);
          gp_params.set_otp(&otp);

          info!("Retrying gateway login with MFA...");
        }
      }
    }
  }

  async fn connect_gateway(
    &self,
    portal: &str,
    gateway: &str,
    cookie: &str,
    allow_extend_session: bool,
    extension_auth: SessionExtensionAuth,
  ) -> Result<(), GatewayConnectError> {
    let mtu = self.args.mtu.unwrap_or(0);
    let (hip, csd_wrapper) = self.determine_hip_script();
    let hip_user = self.determine_hip_user();
    let csd_uid = get_uid(&hip_user).map_err(GatewayConnectError::before_tunnel)?;
    let os_profile = self.os_profile.borrow().clone();

    let session_ctx = build_session_context(SessionContextInput {
      portal: portal.to_string(),
      gateway: gateway.to_string(),
      cookie: cookie.to_string(),
      os_profile: os_profile.clone(),
      certificate: self.args.certificate.clone(),
      sslkey: self.args.sslkey.clone(),
      key_password: self.latest_key_password.borrow().clone(),
      disable_ipv6: self.args.disable_ipv6,
      extension_auth: Some(extension_auth),
    });
    let vpn_builder = Vpn::builder(gateway, cookie)
      .script(self.args.script.clone())
      .interface(self.args.interface.clone())
      .script_tun(self.args.script_tun)
      .certificate(self.args.certificate.clone())
      .sslkey(self.args.sslkey.clone())
      .key_password(self.latest_key_password.borrow().clone())
      .hip(hip)
      .csd_uid(csd_uid)
      .csd_wrapper(csd_wrapper)
      .reconnect_timeout(self.args.reconnect_timeout)
      .mtu(mtu)
      .disable_ipv6(self.args.disable_ipv6)
      .no_dtls(self.args.no_dtls)
      .local_hostname(self.args.local_hostname.clone())
      .dpd_interval(self.args.dpd_interval.unwrap_or(0))
      .no_xmlpost(self.args.no_xmlpost);
    let vpn = apply_os_profile(vpn_builder, &os_profile)
      .build()
      .map_err(|err| GatewayConnectError::before_tunnel(err.into()))?;

    let vpn = Arc::new(vpn);
    let vpn_clone = vpn.clone();
    let runtime_handle = Handle::current();
    let session_ctx = Arc::new(Mutex::new(Some(session_ctx)));
    let session_task: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let session_task_on_connect = Arc::clone(&session_task);
    let session_ctx_on_connect = Arc::clone(&session_ctx);
    let tunnel_established = Arc::new(AtomicBool::new(false));
    let tunnel_established_on_connect = Arc::clone(&tunnel_established);

    tokio::spawn(async move {
      shutdown_signal().await;
      info!("Received the interrupt signal, disconnecting...");
      vpn_clone.disconnect();
    });

    let connect_result = vpn.connect(move |vpn_session_info| {
      tunnel_established_on_connect.store(true, Ordering::SeqCst);
      write_pid_file();

      let Some(session_ctx) = session_ctx_on_connect.lock().unwrap().take() else {
        return;
      };
      let session_info = session_info_from_vpn(vpn_session_info, allow_extend_session);
      info!("VPN session info: {}", session_info.log_summary());

      let task = spawn_session_runtime_with_info(&runtime_handle, session_ctx, session_info);
      session_task_on_connect.lock().unwrap().replace(task);
    });
    let tunnel_established = tunnel_established.load(Ordering::SeqCst);

    if let Some(task) = session_task.lock().unwrap().take() {
      task.abort();
    }

    if fs::metadata(GP_CLIENT_LOCK_FILE).is_ok() {
      info!("Removing PID file");
      fs::remove_file(GP_CLIENT_LOCK_FILE).map_err(|err| GatewayConnectError::after_tunnel(err.into()))?;
    }

    classify_openconnect_result(connect_result, tunnel_established)
  }

  fn determine_hip_script(&self) -> (bool, Option<String>) {
    if let Some(hip) = &self.args.hip {
      return if hip.is_empty() {
        (true, None)
      } else {
        (true, Some(hip.clone()))
      };
    }

    (self.args.csd_wrapper.is_some(), self.args.csd_wrapper.clone())
  }

  fn determine_hip_user(&self) -> Option<String> {
    if let Some(hip_user) = &self.args.hip_user {
      return Some(hip_user.clone());
    }

    self.args.csd_user.clone()
  }
}

fn classify_openconnect_result(exit_code: i32, tunnel_established: bool) -> Result<(), GatewayConnectError> {
  if exit_code == 0 {
    return Ok(());
  }

  let error = anyhow::anyhow!("OpenConnect exited with status {}", exit_code);
  if tunnel_established {
    Err(GatewayConnectError::after_tunnel(error))
  } else {
    Err(GatewayConnectError::before_tunnel(error))
  }
}

fn direct_gateway_command(gateway: &str) -> String {
  format!("gpauth --gateway {gateway} | sudo gpclient connect {gateway} --as-gateway --cookie-on-stdin")
}

fn write_pid_file() {
  let pid = std::process::id();

  if let Err(err) = fs::write(GP_CLIENT_LOCK_FILE, pid.to_string()) {
    warn!("Failed to write PID file: {}", err);
  } else {
    info!("Wrote PID {} to {}", pid, GP_CLIENT_LOCK_FILE);
  }
}

fn get_uid(user: &Option<String>) -> anyhow::Result<u32> {
  if let Some(user) = user {
    get_user_by_name(user).map(|user| user.uid())
  } else {
    get_non_root_user().map_or_else(|_| Ok(0), |user| Ok(user.uid()))
  }
}

fn apply_os_profile(builder: VpnBuilder, profile: &OsProfile) -> VpnBuilder {
  builder
    .os(Some(profile.client_os().to_openconnect_os().to_string()))
    .os_version(Some(profile.os_version().to_string()))
    .client_version(Some(profile.client_version().to_string()))
    .host_id(Some(profile.host_identity().host_id().to_string()))
    .user_agent(Some(profile.user_agent().to_string()))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn openconnect_success_is_not_a_gateway_failure() {
    assert!(classify_openconnect_result(0, false).is_ok());
    assert!(classify_openconnect_result(0, true).is_ok());
  }

  #[test]
  fn openconnect_failure_before_callback_is_retryable_gateway_failure() {
    let err = classify_openconnect_result(1, false).expect_err("nonzero exit should fail");

    assert!(err.is_before_tunnel());
  }

  #[test]
  fn openconnect_failure_after_callback_is_terminal_gateway_failure() {
    let err = classify_openconnect_result(1, true).expect_err("nonzero exit should fail");

    assert!(!err.is_before_tunnel());
  }

  #[test]
  fn direct_gateway_recommendation_uses_gateway_server() {
    assert_eq!(
      direct_gateway_command("gateway.example.test"),
      "gpauth --gateway gateway.example.test | sudo gpclient connect gateway.example.test --as-gateway --cookie-on-stdin"
    );
  }
}
