mod args;
mod credential;
#[cfg(test)]
mod flow_tests;
mod gateway;
mod internal;

use std::cell::RefCell;
use std::sync::{
  Arc,
  atomic::{AtomicBool, Ordering},
};

use anyhow::bail;
use gpapi::{
  clap::report,
  error::PortalError,
  gateway::GatewaySelection,
  gp_params::GpParams,
  os_profile::OsProfile,
  portal::{PreloginOptions, prelogin, retrieve_config},
  session::GatewaySessions,
  utils::request::RequestIdentityError,
};
use inquire::{Password, PasswordDisplayMode, Select};
use log::{Level, info, warn};

use crate::cli::SharedArgs;

pub(crate) use args::ConnectArgs;
use args::{build_os_profile, build_os_profile_with_host_id, warn_deprecated_connect_args};
use credential::CleanAuthState;
use gateway::GatewayConnectError;

pub(crate) struct ConnectHandler<'a> {
  cancellation: tokio_util::sync::CancellationToken,
  args: &'a ConnectArgs,
  shared_args: &'a SharedArgs<'a>,
  os_profile: RefCell<OsProfile>,
  latest_key_password: RefCell<Option<String>>,
  password_from_stdin: RefCell<Option<String>>,
  cookie_from_stdin: RefCell<Option<String>>,
  clean_auth_state: RefCell<CleanAuthState>,
  sessions: RefCell<GatewaySessions>,
  pid_written: Arc<AtomicBool>,
}

impl<'a> ConnectHandler<'a> {
  pub(crate) fn new(args: &'a ConnectArgs, shared_args: &'a SharedArgs) -> Self {
    warn_deprecated_connect_args(args);

    #[cfg(feature = "webview-auth")]
    let clean_auth = args.clean;
    #[cfg(not(feature = "webview-auth"))]
    let clean_auth = false;

    Self {
      cancellation: tokio_util::sync::CancellationToken::new(),
      args,
      shared_args,
      os_profile: RefCell::new(build_os_profile(args)),
      latest_key_password: Default::default(),
      password_from_stdin: Default::default(),
      cookie_from_stdin: Default::default(),
      clean_auth_state: RefCell::new(CleanAuthState::new(clean_auth)),
      sessions: RefCell::default(),
      pid_written: Arc::default(),
    }
  }

  fn build_gp_params(&self) -> GpParams {
    let mut builder = GpParams::builder(self.os_profile.borrow().clone());
    builder
      .csc_mode(self.args.csc)
      .ignore_tls_errors(self.shared_args.ignore_tls_errors)
      .certificate(self.args.certificate.clone())
      .sslkey(self.args.sslkey.clone())
      .key_password(self.latest_key_password.borrow().clone());

    builder.build()
  }

  pub(super) fn prelogin_options(&self, gateway_browser_auth_allowed: bool) -> PreloginOptions {
    PreloginOptions::default()
      .external_browser_requested(self.external_browser_requested())
      .gateway_external_browser_allowed(gateway_browser_auth_allowed)
  }

  pub(super) fn direct_gateway_prelogin_options(&self) -> PreloginOptions {
    self.prelogin_options(true)
  }

  pub(super) fn gateway_browser_auth_allowed(&self, portal_config_default_browser: bool) -> bool {
    gateway_browser_auth_allowed(portal_config_default_browser, self.external_browser_requested())
  }

  fn external_browser_requested(&self) -> bool {
    #[cfg(feature = "webview-auth")]
    {
      self.args.default_browser || self.args.browser.is_some()
    }

    #[cfg(not(feature = "webview-auth"))]
    {
      self.args.browser.is_some()
    }
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    let cancellation = self.cancellation.clone();
    let signal = tokio::spawn(async move {
      gpapi::utils::shutdown_signal().await;
      cancellation.cancel();
    });
    let result = self.handle_attempt().await;
    let sessions = self.sessions.take();
    if self.args.cookie_only && result.is_ok() {
      sessions.relinquish();
    } else {
      sessions.logout().await;
    }
    if self.pid_written.load(Ordering::SeqCst) {
      if let Err(error) = std::fs::remove_file(self.shared_args.lock_file) {
        warn!("Failed to remove PID file: {error}");
      }
    }
    signal.abort();
    let _ = signal.await;
    result
  }

  fn check_cancelled(&self) -> anyhow::Result<()> {
    if self.cancellation.is_cancelled() {
      Err(gpapi::auth::AuthenticationCancelled.into())
    } else {
      Ok(())
    }
  }

  async fn handle_attempt(&self) -> anyhow::Result<()> {
    if self.args.browser_listen.is_some()
      && !matches!(self.args.browser.as_deref(), Some(browser) if browser.eq_ignore_ascii_case("remote"))
    {
      bail!("The '--browser-listen' option requires '--browser remote'");
    }

    #[cfg(feature = "webview-auth")]
    if self.args.default_browser && self.args.browser.is_some() {
      bail!("Cannot use `--default-browser` and `--browser` options at the same time");
    }

    self.latest_key_password.replace(self.args.key_password.clone());

    loop {
      self.check_cancelled()?;
      let Err(err) = self.handle_impl().await else {
        return Ok(());
      };

      let Some(root_cause) = err.root_cause().downcast_ref::<RequestIdentityError>() else {
        return Err(err);
      };

      match root_cause {
        RequestIdentityError::NoKey => {
          let format = self.shared_args.log_format;
          report(
            format,
            Level::Error,
            "ERROR: No private key found in the certificate file",
          );
          report(
            format,
            Level::Error,
            "ERROR: Please provide the private key file using the `-k` option",
          );
          return Ok(());
        }
        RequestIdentityError::NoPassphrase(cert_type) | RequestIdentityError::DecryptError(cert_type) => {
          let message = format!("Enter the {} passphrase:", cert_type);
          let password = Password::new(&message)
            .without_confirmation()
            .with_display_mode(PasswordDisplayMode::Masked)
            .prompt()?;

          self.latest_key_password.replace(Some(password));
        }
      }
    }
  }

  pub(crate) async fn handle_impl(&self) -> anyhow::Result<()> {
    self.check_cancelled()?;
    let server = self.args.server.as_str();
    let as_gateway = self.args.as_gateway;

    self.prepare_cookie_from_stdin()?;

    if as_gateway {
      info!("Treating the server as a gateway");
      return self.connect_gateway_with_prelogin(server, server, false, None).await;
    }

    if !self.args.cookie_on_stdin {
      if let Some(result) = self.try_cached_cookie(server).await {
        return result;
      }
    }
    self.check_cancelled()?;

    let Err(err) = self.connect_portal_with_prelogin(server).await else {
      return Ok(());
    };

    warn!("Failed to connect portal with prelogin: {}", err);
    if err.root_cause().downcast_ref::<PortalError>().is_some() {
      info!("Trying the gateway authentication workflow...");
      self.connect_gateway_with_prelogin(server, server, false, None).await?;

      let format = self.shared_args.log_format;
      report(
        format,
        Level::Warn,
        "\nNOTE: the server may be a gateway, not a portal.",
      );
      report(
        format,
        Level::Warn,
        "NOTE: try to use the `--as-gateway` option if you were authenticated twice.",
      );

      Ok(())
    } else {
      Err(err)
    }
  }

  async fn connect_portal_with_prelogin(&self, portal: &str) -> anyhow::Result<()> {
    let gp_params = self.build_gp_params();

    let prelogin = tokio::select! {
      biased;
      _ = self.cancellation.cancelled() => return Err(gpapi::auth::AuthenticationCancelled.into()),
      result = prelogin(portal, &gp_params, self.prelogin_options(false)) => result?,
    };

    let cached = self.cached_portal_credential(portal);
    let cached_config = match cached {
      Some(cred) => match retrieve_config(portal, &cred, &gp_params, &self.cancellation).await {
        Ok(config) => Some((cred, config)),
        Err(error) => {
          self.check_cancelled()?;
          warn!("Cached portal authentication failed: {error}");
          self.clear_cookie_cache();
          None
        }
      },
      None => None,
    };
    let (cred, mut portal_config) = match cached_config {
      Some(result) => result,
      None => {
        let cred = self.obtain_credential(&prelogin, portal, false).await?;
        self.check_cancelled()?;
        let config = retrieve_config(portal, &cred, &gp_params, &self.cancellation).await?;
        (cred, config)
      }
    };
    self.check_cancelled()?;

    portal_config.sort_gateways(prelogin.region());

    let auth_cookie = match cred.password() {
      Some(password) => portal_config.auth_cookie().clone().with_password(password),
      None => portal_config.auth_cookie().clone(),
    };
    let portal_config_default_browser = portal_config.default_browser().unwrap_or(false);
    info!("Portal config default-browser: {}", portal_config_default_browser);

    if self.args.auto_gateway {
      let gateways = portal_config.gateways();
      if gateways.is_empty() {
        bail!("No gateways available in portal config for auto-gateway selection");
      }

      info!(
        "Auto-gateway mode: trying {} gateway(s) in priority order",
        gateways.len()
      );

      let mut last_err: Option<anyhow::Error> = None;
      for (index, gateway) in gateways.iter().enumerate() {
        info!("Auto-gateway: attempting gateway {}", gateway);
        match self
          .connect_gateway_with_fallback(
            portal,
            gateway,
            &auth_cookie,
            &portal_config,
            GatewaySelection::Auto,
            &gateways[index + 1..],
          )
          .await
        {
          Ok(()) => return Ok(()),
          Err(err) => {
            self.logout_gateway(gateway.server()).await;
            if !err.is_before_establishment() {
              return Err(err.into_error());
            }
            warn!(
              "Auto-gateway: gateway {} failed before session establishment: {}",
              gateway,
              err.as_error()
            );
            last_err = Some(err.into_error());
          }
        }
      }

      let detail = last_err
        .map(|e| e.to_string())
        .unwrap_or_else(|| "unknown error".to_string());
      bail!(
        "Auto-gateway: all {} gateway(s) failed to connect; last error: {}",
        gateways.len(),
        detail
      );
    }

    let gateway_selection = if self.args.gateway.is_some() {
      GatewaySelection::Manual
    } else {
      GatewaySelection::Auto
    };
    let selected_gateway = match &self.args.gateway {
      Some(gateway) => portal_config
        .find_gateway(gateway)
        .ok_or_else(|| anyhow::anyhow!("Cannot find gateway specified: {}", gateway))?,
      None => {
        let gateways = portal_config.gateways();

        if gateways.is_empty() {
          bail!("No gateways available in portal configuration");
        }
        if gateways.len() > 1 {
          let gateway = Select::new("Which gateway do you want to connect to?", gateways)
            .with_vim_mode(true)
            .prompt()?;
          info!("Connecting to the selected gateway: {}", gateway);
          gateway
        } else {
          info!("Connecting to the only available gateway: {}", gateways[0]);
          gateways[0]
        }
      }
    };

    self
      .connect_gateway_with_fallback(
        portal,
        selected_gateway,
        &auth_cookie,
        &portal_config,
        gateway_selection,
        &[],
      )
      .await
      .map_err(GatewayConnectError::into_error)
  }

  fn apply_stdin_host_id(&self, host_id: Option<&str>) {
    let Some(host_id) = host_id else {
      return;
    };
    self
      .os_profile
      .replace(build_os_profile_with_host_id(self.args, Some(host_id)));
    info!(
      "connect profile host-id: {}",
      self.os_profile.borrow().host_identity().host_id()
    );
  }
}

fn gateway_browser_auth_allowed(portal_config_default_browser: bool, external_browser_requested: bool) -> bool {
  portal_config_default_browser || external_browser_requested
}

#[cfg(test)]
mod tests {
  use super::gateway_browser_auth_allowed;

  #[test]
  fn gateway_browser_auth_is_allowed_by_portal_config() {
    assert!(gateway_browser_auth_allowed(true, false));
  }

  #[test]
  fn gateway_browser_auth_is_allowed_by_explicit_browser_request() {
    assert!(gateway_browser_auth_allowed(false, true));
  }

  #[test]
  fn gateway_browser_auth_is_disabled_without_portal_config_or_user_request() {
    assert!(!gateway_browser_auth_allowed(false, false));
  }
}
