use std::{
  fs,
  path::Path,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
  },
};

use gpapi::{
  clap::report,
  cookie_store,
  credential::{AuthCookieCredential, Credential},
  gateway::{Gateway, GatewayLogin, GatewayLoginClient, GatewayLoginContext, GatewaySelection, SessionExtensionAuth},
  gp_params::GpParams,
  os_profile::OsProfile,
  portal::{PortalConfig, prelogin},
  process::users::get_user_by_name,
  session::{GatewayAuthentication, SessionMode},
};
use inquire::Text;
use log::{Level, info, warn};
use openconnect::{HipScript, HipSource, Vpn, VpnBuilder};
use tokio::{runtime::Handle, task::JoinHandle};

use crate::session::{
  SessionContextInput, build_session_context, session_info_from_vpn, spawn_session_runtime_with_info,
};

use super::{ConnectHandler, args::cookie_cache_path};

const OPENCONNECT_INTERRUPTED_EXIT_CODE: i32 = -4;

pub(super) struct GatewayLoginSession {
  pub(super) authentication: GatewayAuthentication,
  pub(super) extension_auth: SessionExtensionAuth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GatewayConnectFailureStage {
  BeforeEstablishment,
  AfterEstablishment,
}

#[derive(Debug)]
pub(super) struct GatewayConnectError {
  stage: GatewayConnectFailureStage,
  error: anyhow::Error,
}

impl GatewayConnectError {
  fn before_establishment(error: anyhow::Error) -> Self {
    Self {
      stage: GatewayConnectFailureStage::BeforeEstablishment,
      error,
    }
  }

  fn after_establishment(error: anyhow::Error) -> Self {
    Self {
      stage: GatewayConnectFailureStage::AfterEstablishment,
      error,
    }
  }

  pub(super) fn is_before_establishment(&self) -> bool {
    self.stage == GatewayConnectFailureStage::BeforeEstablishment
  }

  pub(super) fn as_error(&self) -> &anyhow::Error {
    &self.error
  }

  pub(super) fn into_error(self) -> anyhow::Error {
    self.error
  }
}

impl ConnectHandler<'_> {
  pub(super) async fn try_cached_cookie(&self, server: &str) -> Option<anyhow::Result<()>> {
    let path = cookie_cache_path(self.args)?;
    let host_id = self.os_profile.borrow().host_identity().host_id().to_string();
    let stored = cookie_store::load(&path, server, &host_id)?;
    if !stored.auth_cookie.can_authenticate_gateway() {
      cookie_store::clear(&path);
      return None;
    }
    let credential = (&stored.auth_cookie).into();
    let mut params = self.build_gp_params();
    params.set_is_gateway(true);
    let session = match self
      .login_gateway(&stored.last_gateway, &credential, &params, None)
      .await
    {
      Ok(session) => session,
      Err(error) => {
        if self.cancellation.is_cancelled()
          || error
            .downcast_ref::<gpapi::gateway::GatewayLoginProtocolError>()
            .is_some()
        {
          return Some(Err(error));
        }
        cookie_store::clear(&path);
        warn!("Cached gateway authentication failed: {error}");
        return None;
      }
    };
    if session.authentication.mode() == SessionMode::NonTunnel && !self.args.cookie_only {
      self.logout_gateway(&stored.last_gateway).await;
      info!("Cached non-tunnel login requires fresh portal discovery");
      return None;
    }
    let result = self
      .connect_gateway(
        server,
        &stored.last_gateway,
        &session.authentication,
        false,
        session.extension_auth,
      )
      .await;
    match result {
      Ok(()) => Some(Ok(())),
      Err(error) if self.cancellation.is_cancelled() => Some(Err(error.into_error())),
      Err(error) => {
        self.logout_gateway(&stored.last_gateway).await;
        warn!("Cached gateway connection failed: {}", error.as_error());
        None
      }
    }
  }

  pub(super) fn cached_portal_credential(&self, server: &str) -> Option<Credential> {
    if self.args.cookie_on_stdin {
      return None;
    }
    let path = cookie_cache_path(self.args)?;
    let host_id = self.os_profile.borrow().host_identity().host_id().to_string();
    let stored = cookie_store::load(&path, server, &host_id)?;
    if !stored.auth_cookie.can_authenticate_gateway() {
      cookie_store::clear(&path);
      return None;
    }
    info!("Using cached credential for fresh portal discovery");
    Some((&stored.auth_cookie).into())
  }

  pub(super) fn clear_cookie_cache(&self) {
    if let Some(path) = cookie_cache_path(self.args) {
      cookie_store::clear(&path);
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

    let gateway_browser_auth_allowed = true;
    let prelogin = tokio::select! {
      biased;
      _ = self.cancellation.cancelled() => return Err(gpapi::auth::AuthenticationCancelled.into()),
      result = prelogin(gateway, &gp_params, self.direct_gateway_prelogin_options()) => result?,
    };
    let cred = self
      .obtain_credential(&prelogin, gateway, gateway_browser_auth_allowed)
      .await?;

    let login_session = self
      .login_gateway(gateway, &cred, &gp_params, gateway_context.as_ref())
      .await?;

    self
      .connect_gateway(
        portal,
        gateway,
        &login_session.authentication,
        allow_extend_session,
        login_session.extension_auth,
      )
      .await
      .map_err(GatewayConnectError::into_error)
  }

  pub(super) async fn connect_gateway_with_fallback(
    &self,
    portal: &str,
    gateway: &Gateway,
    portal_cred: &AuthCookieCredential,
    config: &PortalConfig,
    selection: GatewaySelection,
    remaining: &[&Gateway],
  ) -> Result<(), GatewayConnectError> {
    let session = self
      .authenticate_gateway(
        portal,
        gateway.server(),
        portal_cred,
        config.default_browser().unwrap_or(false),
        GatewayLoginContext::new(gateway, selection).with_connect_method(config.connect_method()),
      )
      .await
      .map_err(GatewayConnectError::before_establishment)?;
    if !self.args.cookie_only && session.authentication.mode() == SessionMode::NonTunnel {
      if gateway.kind() != gpapi::gateway::GatewayKind::Internal {
        return Err(GatewayConnectError::after_establishment(anyhow::anyhow!(
          "Non-tunnel authentication requires freshly discovered internal gateways"
        )));
      }
      return self
        .connect_internal_gateways(portal, config, portal_cred, gateway, session, remaining)
        .await
        .map_err(GatewayConnectError::after_establishment);
    }
    self
      .connect_gateway(
        portal,
        gateway.server(),
        &session.authentication,
        config.allow_extend_session().unwrap_or(false),
        session.extension_auth,
      )
      .await
  }

  pub(super) async fn authenticate_gateway(
    &self,
    portal: &str,
    gateway: &str,
    portal_cred: &AuthCookieCredential,
    portal_config_default_browser: bool,
    gateway_context: GatewayLoginContext,
  ) -> anyhow::Result<GatewayLoginSession> {
    self.check_cancelled()?;
    let mut gp_params = self.build_gp_params();
    gp_params.set_is_gateway(true);
    let browser_allowed = self.gateway_browser_auth_allowed(portal_config_default_browser);
    let gateway_prelogin = tokio::select! {
      biased;
      _ = self.cancellation.cancelled() => return Err(gpapi::auth::AuthenticationCancelled.into()),
      result = prelogin(gateway, &gp_params, self.prelogin_options(browser_allowed)) => result?,
    };
    self.check_cancelled()?;

    if portal_cred.can_authenticate_gateway() {
      let credential: Credential = portal_cred.into();
      match self
        .login_gateway(gateway, &credential, &gp_params, Some(&gateway_context))
        .await
      {
        Ok(session) => {
          self.save_cookie_cache(portal, gateway, portal_cred);
          return Ok(session);
        }
        Err(error) => {
          self.check_cancelled()?;
          if error
            .downcast_ref::<gpapi::gateway::GatewayLoginProtocolError>()
            .is_some()
          {
            return Err(error);
          }
          info!("Portal cookie login failed; using gateway prelogin authentication: {error}");
        }
      }
    }

    let credential = self
      .obtain_credential(&gateway_prelogin, gateway, browser_allowed)
      .await
      .inspect_err(|_| self.print_direct_gateway_recommendation(gateway))?;
    self.check_cancelled()?;
    let result = self
      .login_gateway(gateway, &credential, &gp_params, Some(&gateway_context))
      .await;
    if result.is_err() {
      self.print_direct_gateway_recommendation(gateway);
    }
    result
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

    let format = self.shared_args.log_format;
    report(
      format,
      Level::Warn,
      "\nNOTE: Gateway authentication failed after portal login.",
    );
    report(
      format,
      Level::Warn,
      "NOTE: If this server also accepts direct gateway login, try:",
    );
    report(
      format,
      Level::Warn,
      &format!("NOTE: {}", direct_gateway_command(gateway)),
    );
  }

  async fn login_gateway(
    &self,
    gateway: &str,
    cred: &Credential,
    gp_params: &GpParams,
    gateway_context: Option<&GatewayLoginContext>,
  ) -> anyhow::Result<GatewayLoginSession> {
    let mut gp_params = gp_params.clone();
    gp_params.prepare_client_identity()?;
    let cancellation = self.cancellation.clone();
    let internal = gateway_context.is_some_and(|context| context.kind() == gpapi::gateway::GatewayKind::Internal);
    let mut client = GatewayLoginClient::new(gateway, gp_params)?;

    loop {
      self.check_cancelled()?;
      let login = client.login(cred, gateway_context, &cancellation).await?;

      match login {
        GatewayLogin::Authenticated(authentication) => {
          let descriptor = Gateway::new(gateway.to_string(), gateway.to_string());
          client.register_session(
            descriptor.clone(),
            authentication.clone(),
            &mut self.sessions.borrow_mut(),
          );
          self.check_cancelled()?;
          if internal && authentication.mode() == SessionMode::NonTunnel && !self.args.cookie_only {
            client
              .bind_non_tunnel(self.args.disable_ipv6, None, &cancellation)
              .await
              .map_err(|error| error.context(gpapi::gateway::GatewayLoginProtocolError))?;
            client.update_registered_session(&descriptor, &authentication, &mut self.sessions.borrow_mut());
          }
          return Ok(GatewayLoginSession {
            authentication,
            extension_auth: SessionExtensionAuth::new(cred.clone(), client),
          });
        }
        GatewayLogin::Mfa(message, input_str) => {
          let otp = Text::new(&message).prompt()?;
          client.respond_mfa(&input_str, &otp);

          info!("Retrying gateway login with MFA...");
        }
      }
    }
  }

  pub(super) async fn connect_gateway(
    &self,
    portal: &str,
    gateway: &str,
    authentication: &GatewayAuthentication,
    allow_extend_session: bool,
    extension_auth: SessionExtensionAuth,
  ) -> Result<(), GatewayConnectError> {
    let cookie = authentication.cookie();
    // --cookie-only: print the gateway cookie and exit without opening a tunnel.
    // No tun device is allocated, no root is required, and no logout is issued
    // against the gateway — the cookie remains valid for subsequent use.
    // Output format matches openconnect --authenticate for easy shell consumption.
    if self.args.cookie_only {
      println!("COOKIE='{}'", cookie);
      println!("HOST='{}'", gateway);
      return Ok(());
    }

    if authentication.mode() != SessionMode::Tunnel {
      return Err(GatewayConnectError::after_establishment(anyhow::anyhow!(
        "Non-tunnel authentication requires fresh internal portal discovery"
      )));
    }
    self
      .check_cancelled()
      .map_err(GatewayConnectError::after_establishment)?;
    let identity_files = extension_auth
      .client_identity()
      .map(|identity| identity.write_files(None))
      .transpose()
      .map_err(GatewayConnectError::before_establishment)?;
    let certificate = identity_files.as_ref().map(|files| files.certificate().to_owned());
    let sslkey = identity_files.as_ref().and_then(|files| files.key().map(str::to_owned));
    let key_password = extension_auth
      .client_identity()
      .and_then(|identity| identity.key_password())
      .map(str::to_owned);
    let mtu = self.args.mtu.unwrap_or(0);
    let os_profile = self.os_profile.borrow().clone();
    let hip_source = self
      .hip_source(os_profile.clone())
      .map_err(GatewayConnectError::before_establishment)?;

    let session_ctx = build_session_context(SessionContextInput {
      portal: portal.to_string(),
      gateway: gateway.to_string(),
      cookie: cookie.to_string(),
      os_profile: os_profile.clone(),
      certificate: certificate.clone(),
      sslkey: sslkey.clone(),
      key_password: key_password.clone(),
      disable_ipv6: self.args.disable_ipv6,
      extension_auth: Some(extension_auth),
    });
    let vpn_builder = Vpn::builder(gateway, cookie)
      .script(self.args.script.clone())
      .interface(self.args.interface.clone())
      .script_tun(self.args.script_tun)
      .certificate(certificate)
      .sslkey(sslkey)
      .key_password(key_password)
      .hip_source(hip_source)
      .reconnect_timeout(self.args.reconnect_timeout)
      .mtu(mtu)
      .disable_ipv6(self.args.disable_ipv6)
      .no_dtls(self.args.no_dtls)
      .local_hostname(self.args.local_hostname.clone())
      .dpd_interval(self.args.dpd_interval.unwrap_or(0))
      .no_xmlpost(self.args.no_xmlpost);
    let vpn = apply_os_profile(vpn_builder, &os_profile)
      .build()
      .map_err(|err| GatewayConnectError::before_establishment(err.into()))?;

    let vpn = Arc::new(vpn);
    let vpn_clone = vpn.clone();
    let runtime_handle = Handle::current();
    let session_ctx = Arc::new(Mutex::new(Some(session_ctx)));
    let session_task: Arc<Mutex<Option<JoinHandle<()>>>> = Arc::new(Mutex::new(None));
    let session_task_on_connect = Arc::clone(&session_task);
    let session_ctx_on_connect = Arc::clone(&session_ctx);
    let tunnel_established = Arc::new(AtomicBool::new(false));
    let tunnel_established_on_connect = Arc::clone(&tunnel_established);
    let disconnect_requested = Arc::new(AtomicBool::new(false));
    let disconnect_requested_on_signal = Arc::clone(&disconnect_requested);
    let cancellation = self.cancellation.clone();

    let disconnect_task = tokio::spawn(async move {
      cancellation.cancelled().await;
      info!("Received the interrupt signal, disconnecting...");
      disconnect_requested_on_signal.store(true, Ordering::SeqCst);
      vpn_clone.disconnect();
    });

    let lock_file_on_connect = self.shared_args.lock_file.to_path_buf();
    let pid_written = self.pid_written.clone();
    let log_format = self.shared_args.log_format;
    let session_cancellation = self.cancellation.clone();
    let (established_tx, established_rx) = tokio::sync::oneshot::channel();
    self.sessions.borrow_mut().relinquish_gateway(gateway);
    let worker = tokio::task::spawn_blocking(move || {
      vpn.connect(move |vpn_session_info| {
        if session_cancellation.is_cancelled() {
          return;
        }
        tunnel_established_on_connect.store(true, Ordering::SeqCst);
        pid_written.store(write_pid_file(&lock_file_on_connect), Ordering::SeqCst);
        let _ = established_tx.send(());

        let Some(session_ctx) = session_ctx_on_connect.lock().unwrap().take() else {
          return;
        };
        let session_info = session_info_from_vpn(vpn_session_info, allow_extend_session);
        info!("VPN session info: {}", session_info.log_summary());

        let task = spawn_session_runtime_with_info(&runtime_handle, session_ctx, session_info, log_format);
        session_task_on_connect.lock().unwrap().replace(task);
      })
    });
    tokio::pin!(worker);
    let connect_result = tokio::select! {
      result = &mut worker => result,
      established = established_rx => {
        if established.is_ok() {
          let unused = self.sessions.borrow().members().iter()
            .filter(|session| session.gateway.server() != gateway)
            .map(|session| session.gateway.server().to_owned())
            .collect::<Vec<_>>();
          for server in unused {
            self.logout_gateway(&server).await;
          }
        }
        (&mut worker).await
      }
    };
    let tunnel_established = tunnel_established.load(Ordering::SeqCst);
    disconnect_task.abort();
    let _ = disconnect_task.await;

    let session_task = { session_task.lock().unwrap().take() };
    if let Some(task) = session_task {
      task.abort();
      let _ = task.await;
    }

    let disconnect_requested = disconnect_requested.load(Ordering::SeqCst);
    let connect_result = connect_result.map_err(|error| GatewayConnectError::after_establishment(error.into()))?;
    classify_openconnect_result(connect_result, tunnel_established, disconnect_requested)
  }

  pub(super) fn hip_source(&self, profile: OsProfile) -> anyhow::Result<HipSource> {
    let user = self.determine_hip_user();
    let script = self.args.hip.as_ref().or(self.args.csd_wrapper.as_ref());
    match script {
      Some(path) if !path.is_empty() => Ok(HipSource::Script(HipScript::new(path.clone(), get_uid(&user)?)?)),
      Some(_) => {
        anyhow::ensure!(user.is_none(), "--hip-user requires an explicit HIP script");
        Ok(HipSource::Generator(Arc::new(move |request, control| {
          let input = gphip::ReportInput {
            profile: profile.clone(),
            context: gphip::ReportContext::Connected {
              cookie: request.cookie.clone(),
              client_ip: request.client_ip.clone(),
              client_ipv6: request.client_ipv6.clone(),
              md5: request.md5.clone(),
            },
          };
          gphip::generate_report_with_control(&input, &|| control.check())
            .map_err(|error| error.downcast::<std::io::Error>().unwrap_or_else(std::io::Error::other))
        })))
      }
      None => {
        anyhow::ensure!(user.is_none(), "--hip-user requires an explicit HIP script");
        Ok(HipSource::Disabled)
      }
    }
  }

  fn determine_hip_user(&self) -> Option<String> {
    if let Some(hip_user) = &self.args.hip_user {
      return Some(hip_user.clone());
    }

    self.args.csd_user.clone()
  }
}

fn classify_openconnect_result(
  exit_code: i32,
  tunnel_established: bool,
  disconnect_requested: bool,
) -> Result<(), GatewayConnectError> {
  if exit_code == 0 {
    return Ok(());
  }

  if disconnect_requested && exit_code == OPENCONNECT_INTERRUPTED_EXIT_CODE {
    return Ok(());
  }

  let error = anyhow::anyhow!("OpenConnect exited with status {}", exit_code);
  if tunnel_established {
    Err(GatewayConnectError::after_establishment(error))
  } else {
    Err(GatewayConnectError::before_establishment(error))
  }
}

fn direct_gateway_command(gateway: &str) -> String {
  format!("gpauth --gateway {gateway} | sudo gpclient connect {gateway} --as-gateway --cookie-on-stdin")
}

pub(super) fn write_pid_file(lock_file: &Path) -> bool {
  let pid = std::process::id();

  if let Err(err) = fs::write(lock_file, pid.to_string()) {
    warn!("Failed to write PID file: {}", err);
    false
  } else {
    info!("Wrote PID {} to {}", pid, lock_file.display());
    true
  }
}

fn get_uid(user: &Option<String>) -> anyhow::Result<Option<u32>> {
  user
    .as_ref()
    .map(|user| get_user_by_name(user).map(|user| user.uid()))
    .transpose()
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
  fn hip_script_defaults_to_inherited_process_identity() {
    assert_eq!(get_uid(&None).unwrap(), None);
  }

  #[test]
  fn explicit_hip_user_must_exist() {
    assert!(get_uid(&Some("gpclient-nonexistent-hip-user".to_string())).is_err());
  }

  #[test]
  fn explicit_hip_user_resolves_to_the_selected_account() {
    let current = uzers::get_user_by_uid(uzers::get_effective_uid()).unwrap();
    let name = current.name().to_string_lossy().into_owned();
    assert_eq!(get_uid(&Some(name)).unwrap(), Some(current.uid()));
  }

  #[test]
  fn openconnect_success_is_not_a_gateway_failure() {
    assert!(classify_openconnect_result(0, false, false).is_ok());
    assert!(classify_openconnect_result(0, true, false).is_ok());
  }

  #[test]
  fn openconnect_failure_before_callback_is_retryable_gateway_failure() {
    let err = classify_openconnect_result(1, false, false).expect_err("nonzero exit should fail");

    assert!(err.is_before_establishment());
  }

  #[test]
  fn openconnect_failure_after_callback_is_terminal_gateway_failure() {
    let err = classify_openconnect_result(1, true, false).expect_err("nonzero exit should fail");

    assert!(!err.is_before_establishment());
  }

  #[test]
  fn interrupted_exit_after_requested_disconnect_is_success() {
    assert!(classify_openconnect_result(OPENCONNECT_INTERRUPTED_EXIT_CODE, true, true).is_ok());
  }

  #[test]
  fn interrupted_exit_without_requested_disconnect_is_failure() {
    let err = classify_openconnect_result(OPENCONNECT_INTERRUPTED_EXIT_CODE, true, false)
      .expect_err("unexpected interrupt should fail");

    assert!(!err.is_before_establishment());
  }

  #[test]
  fn direct_gateway_recommendation_uses_gateway_server() {
    assert_eq!(
      direct_gateway_command("gateway.example.test"),
      "gpauth --gateway gateway.example.test | sudo gpclient connect gateway.example.test --as-gateway --cookie-on-stdin"
    );
  }
}
