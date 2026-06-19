mod args;
mod credential;
mod gateway;

use std::cell::RefCell;

use anyhow::bail;
use gpapi::{
  error::PortalError,
  gateway::{GatewayLoginContext, GatewaySelection},
  gp_params::GpParams,
  os_profile::OsProfile,
  portal::{PreloginOptions, prelogin, retrieve_config},
  utils::request::RequestIdentityError,
};
use inquire::{Password, PasswordDisplayMode, Select};
use log::{info, warn};

use crate::cli::SharedArgs;

pub(crate) use args::ConnectArgs;
use args::{build_os_profile, build_os_profile_with_host_id, warn_deprecated_connect_args};
use credential::CleanAuthState;
use gateway::GatewayConnectError;

pub(crate) struct ConnectHandler<'a> {
  args: &'a ConnectArgs,
  shared_args: &'a SharedArgs<'a>,
  os_profile: RefCell<OsProfile>,
  latest_key_password: RefCell<Option<String>>,
  password_from_stdin: RefCell<Option<String>>,
  cookie_from_stdin: RefCell<Option<String>>,
  clean_auth_state: RefCell<CleanAuthState>,
}

impl<'a> ConnectHandler<'a> {
  pub(crate) fn new(args: &'a ConnectArgs, shared_args: &'a SharedArgs) -> Self {
    warn_deprecated_connect_args(args);

    #[cfg(feature = "webview-auth")]
    let clean_auth = args.clean;
    #[cfg(not(feature = "webview-auth"))]
    let clean_auth = false;

    Self {
      args,
      shared_args,
      os_profile: RefCell::new(build_os_profile(args)),
      latest_key_password: Default::default(),
      password_from_stdin: Default::default(),
      cookie_from_stdin: Default::default(),
      clean_auth_state: RefCell::new(CleanAuthState::new(clean_auth)),
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

  pub(super) fn prelogin_options(&self, portal_default_browser_enabled: bool) -> PreloginOptions {
    PreloginOptions::default()
      .external_browser_requested(self.external_browser_requested())
      .portal_default_browser_enabled(portal_default_browser_enabled)
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
    #[cfg(feature = "webview-auth")]
    if self.args.default_browser && self.args.browser.is_some() {
      bail!("Cannot use `--default-browser` and `--browser` options at the same time");
    }

    self.latest_key_password.replace(self.args.key_password.clone());

    loop {
      let Err(err) = self.handle_impl().await else {
        return Ok(());
      };

      let Some(root_cause) = err.root_cause().downcast_ref::<RequestIdentityError>() else {
        return Err(err);
      };

      match root_cause {
        RequestIdentityError::NoKey => {
          eprintln!("ERROR: No private key found in the certificate file");
          eprintln!("ERROR: Please provide the private key file using the `-k` option");
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
    let server = self.args.server.as_str();
    let as_gateway = self.args.as_gateway;

    self.prepare_cookie_from_stdin()?;

    if as_gateway {
      info!("Treating the server as a gateway");
      return self.connect_gateway_with_prelogin(server, server, false, None).await;
    }

    if !self.args.cookie_on_stdin {
      if let Some(()) = self.try_cached_cookie(server).await {
        return Ok(());
      }
    }

    let Err(err) = self.connect_portal_with_prelogin(server).await else {
      return Ok(());
    };

    warn!("Failed to connect portal with prelogin: {}", err);
    if err.root_cause().downcast_ref::<PortalError>().is_some() {
      info!("Trying the gateway authentication workflow...");
      self.connect_gateway_with_prelogin(server, server, false, None).await?;

      eprintln!("\nNOTE: the server may be a gateway, not a portal.");
      eprintln!("NOTE: try to use the `--as-gateway` option if you were authenticated twice.");

      Ok(())
    } else {
      Err(err)
    }
  }

  async fn connect_portal_with_prelogin(&self, portal: &str) -> anyhow::Result<()> {
    let gp_params = self.build_gp_params();

    let prelogin = prelogin(portal, &gp_params, self.prelogin_options(false)).await?;

    let cred = self.obtain_credential(&prelogin, portal).await?;
    let mut portal_config = retrieve_config(portal, &cred, &gp_params).await?;

    portal_config.sort_gateways(prelogin.region());

    let auth_cookie = match cred.password() {
      Some(password) => portal_config.auth_cookie().clone().with_password(password),
      None => portal_config.auth_cookie().clone(),
    };
    let allow_extend_session = portal_config.allow_extend_session().unwrap_or(false);
    let portal_default_browser_enabled = portal_config.default_browser().unwrap_or(false);

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
      for gateway in &gateways {
        info!("Auto-gateway: attempting gateway {}", gateway);
        let gateway_context =
          GatewayLoginContext::new(gateway, GatewaySelection::Auto).with_connect_method(portal_config.connect_method());

        match self
          .connect_gateway_with_fallback(
            portal,
            gateway.server(),
            &auth_cookie,
            allow_extend_session,
            portal_default_browser_enabled,
            gateway_context,
          )
          .await
        {
          Ok(()) => return Ok(()),
          Err(err) => {
            if !err.is_before_tunnel() {
              return Err(err.into_error());
            }
            warn!(
              "Auto-gateway: gateway {} failed before tunnel setup: {}",
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

    let gateway = selected_gateway.server();
    let gateway_context =
      GatewayLoginContext::new(selected_gateway, gateway_selection).with_connect_method(portal_config.connect_method());

    self
      .connect_gateway_with_fallback(
        portal,
        gateway,
        &auth_cookie,
        allow_extend_session,
        portal_default_browser_enabled,
        gateway_context,
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
