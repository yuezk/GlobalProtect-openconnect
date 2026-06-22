use anyhow::{Context, bail};
use gpapi::{
  auth::SamlAuthResult,
  clap::ToVerboseArg,
  credential::{Credential, PasswordCredential},
  portal::{Prelogin, StandardPrelogin},
  process::auth_launcher::SamlAuthLauncher,
};
use inquire::{Password, PasswordDisplayMode, Text};
use log::info;

use super::ConnectHandler;

#[derive(Default)]
pub(super) struct CleanAuthState {
  requested: bool,
}

impl CleanAuthState {
  pub(super) fn new(requested: bool) -> Self {
    Self { requested }
  }

  fn consume_for_webview(&mut self, webview_auth: bool) -> bool {
    if !webview_auth || !self.requested {
      return false;
    }

    self.requested = false;
    true
  }
}

impl ConnectHandler<'_> {
  pub(super) fn prepare_cookie_from_stdin(&self) -> anyhow::Result<()> {
    if !self.args.cookie_on_stdin || self.cookie_from_stdin.borrow().is_some() {
      return Ok(());
    }

    let auth_result = self.read_auth_result_from_stdin()?;
    log_stdin_host_id(auth_result.host_id());
    self.apply_stdin_host_id(auth_result.host_id());

    Ok(())
  }

  pub(super) async fn obtain_credential(
    &self,
    prelogin: &Prelogin,
    server: &str,
    gateway_external_browser_allowed: bool,
  ) -> anyhow::Result<Credential> {
    if should_read_stdin_credential(self.args.cookie_on_stdin, self.args.as_gateway, prelogin.is_gateway()) {
      return self.read_cookie_from_stdin();
    }

    let is_gateway = prelogin.is_gateway();

    match prelogin {
      Prelogin::Saml(prelogin) => {
        let external_browser_supported = default_browser_auth_allowed(
          prelogin.support_default_browser(),
          is_gateway,
          gateway_external_browser_allowed,
        );
        let browser = if external_browser_supported {
          self.args.browser.as_deref()
        } else if !cfg!(feature = "webview-auth") {
          bail!(
            "The server does not support authentication via the default browser and the gpclient is not built with the `webview-auth` feature"
          );
        } else {
          None
        };
        let verbose = self.shared_args.verbose.to_verbose_arg();
        let os_profile = self.os_profile.borrow().clone();
        let auth_launcher = SamlAuthLauncher::new(server)
          .gateway(is_gateway)
          .saml_request(prelogin.saml_request())
          .os_profile(&os_profile)
          .fix_openssl(self.shared_args.fix_openssl)
          .ignore_tls_errors(self.shared_args.ignore_tls_errors)
          .browser(browser)
          .verbose(verbose);

        #[cfg(feature = "webview-auth")]
        let use_default_browser = external_browser_supported && self.args.default_browser;
        #[cfg(feature = "webview-auth")]
        let browser_kind = if browser.is_some() || use_default_browser {
          "external"
        } else {
          "embedded"
        };
        #[cfg(not(feature = "webview-auth"))]
        let browser_kind = "external";

        let clean_auth = self
          .clean_auth_state
          .borrow_mut()
          .consume_for_webview(browser_kind == "embedded");

        info!(
          "SAML auth launch: gateway={}, clean={}, browser={}",
          is_gateway, clean_auth, browser_kind
        );

        #[cfg(feature = "webview-auth")]
        let auth_launcher = auth_launcher
          .hidpi(self.args.hidpi)
          .clean(clean_auth)
          .default_browser(use_default_browser);

        let cred = auth_launcher.launch().await?;
        Ok(cred)
      }

      Prelogin::Standard(prelogin) => {
        let prefix = if is_gateway { "Gateway" } else { "Portal" };
        println!("{} ({}: {})", prelogin.auth_message(), prefix, server);

        let user = self.args.user.as_ref().map_or_else(
          || Text::new(&format!("{}:", prelogin.label_username())).prompt(),
          |user| Ok(user.to_owned()),
        )?;

        let password = self.obtain_password(prelogin)?;
        let password_cred = PasswordCredential::new(&user, &password);

        Ok(password_cred.into())
      }
    }
  }

  fn read_cookie_from_stdin(&self) -> anyhow::Result<Credential> {
    if let Some(cookie) = self.cookie_from_stdin.borrow().as_ref() {
      info!("Reusing the cookie read from standard input");
      return Credential::try_from(serde_json::from_str::<SamlAuthResult>(cookie)?);
    }

    let auth_result = self.read_auth_result_from_stdin()?;
    log_stdin_host_id(auth_result.host_id());
    self.apply_stdin_host_id(auth_result.host_id());
    Credential::try_from(auth_result)
  }

  fn read_auth_result_from_stdin(&self) -> anyhow::Result<SamlAuthResult> {
    info!("Reading cookie from standard input");

    let mut cookie = String::new();
    std::io::stdin().read_line(&mut cookie)?;

    self.cookie_from_stdin.replace(Some(cookie.trim_end().to_owned()));

    let auth_result = serde_json::from_str::<SamlAuthResult>(cookie.trim_end()).context("Failed to parse auth data")?;
    Ok(auth_result)
  }

  fn obtain_password(&self, prelogin: &StandardPrelogin) -> anyhow::Result<String> {
    let password = if self.args.passwd_on_stdin {
      if let Some(password) = self.password_from_stdin.borrow().as_ref() {
        info!("Reusing the password read from standard input");
        return Ok(password.clone());
      }

      info!("Reading password from standard input");
      let mut input = String::new();
      std::io::stdin().read_line(&mut input)?;
      let password = input.trim_end().to_owned();
      self.password_from_stdin.replace(Some(password.clone()));

      password
    } else {
      Password::new(&format!("{}:", prelogin.label_password()))
        .without_confirmation()
        .with_display_mode(PasswordDisplayMode::Masked)
        .prompt()?
    };

    Ok(password)
  }
}

fn should_read_stdin_credential(cookie_on_stdin: bool, as_gateway: bool, prelogin_is_gateway: bool) -> bool {
  cookie_on_stdin && (!prelogin_is_gateway || as_gateway)
}

fn default_browser_auth_allowed(
  saml_support_default_browser: bool,
  is_gateway: bool,
  gateway_external_browser_allowed: bool,
) -> bool {
  saml_support_default_browser && (!is_gateway || gateway_external_browser_allowed)
}

fn log_stdin_host_id(host_id: Option<&str>) {
  match host_id {
    Some(host_id) => info!("stdin auth result host-id: {}", host_id),
    None => info!("stdin auth result host-id: <none>"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn clean_auth_state_is_consumed_once_for_webview() {
    let mut state = CleanAuthState::new(true);

    assert!(state.consume_for_webview(true));
    assert!(!state.consume_for_webview(true));
  }

  #[test]
  fn clean_auth_state_is_not_consumed_by_external_browser() {
    let mut state = CleanAuthState::new(true);

    assert!(!state.consume_for_webview(false));
    assert!(state.consume_for_webview(true));
  }

  #[test]
  fn clean_auth_state_remains_false_when_not_requested() {
    let mut state = CleanAuthState::new(false);

    assert!(!state.consume_for_webview(true));
    assert!(!state.consume_for_webview(false));
  }

  #[test]
  fn stdin_credential_is_allowed_for_portal_auth() {
    assert!(should_read_stdin_credential(true, false, false));
  }

  #[test]
  fn stdin_credential_is_allowed_for_direct_gateway_auth() {
    assert!(should_read_stdin_credential(true, true, true));
  }

  #[test]
  fn stdin_credential_is_not_reused_for_gateway_fallback_auth() {
    assert!(!should_read_stdin_credential(true, false, true));
  }

  #[test]
  fn stdin_credential_is_not_used_when_flag_is_disabled() {
    assert!(!should_read_stdin_credential(false, false, false));
    assert!(!should_read_stdin_credential(false, true, true));
  }

  #[test]
  fn portal_saml_can_use_default_browser_when_supported() {
    assert!(default_browser_auth_allowed(true, false, false));
  }

  #[test]
  fn gateway_saml_requires_portal_default_browser_policy() {
    assert!(!default_browser_auth_allowed(true, true, false));
    assert!(default_browser_auth_allowed(true, true, true));
  }

  #[test]
  fn default_browser_auth_requires_saml_support() {
    assert!(!default_browser_auth_allowed(false, false, true));
    assert!(!default_browser_auth_allowed(false, true, true));
  }
}
