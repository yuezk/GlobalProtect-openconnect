use std::{path::PathBuf, process::Stdio};

use anyhow::bail;
use common::binary_paths;
use tokio::process::Command;

use crate::{auth::SamlAuthResult, credential::Credential, log_format::LogFormat, os_profile::OsProfile};

use super::command_traits::CommandExt;

pub struct SamlAuthLauncher<'a> {
  server: &'a str,
  auth_executable: Option<&'a str>,
  gateway: bool,
  saml_request: Option<&'a str>,
  os: Option<&'a str>,
  host_id: Option<&'a str>,
  client_version: Option<&'a str>,
  certificate: Option<&'a str>,
  sslkey: Option<&'a str>,
  key_password: Option<&'a str>,
  fix_openssl: bool,
  ignore_tls_errors: bool,
  #[cfg(feature = "webview-auth")]
  hidpi: bool,
  #[cfg(feature = "webview-auth")]
  clean: bool,
  #[cfg(feature = "webview-auth")]
  default_browser: bool,
  browser: Option<&'a str>,
  verbose: Option<&'a str>,
  log_format: Option<LogFormat>,
}

impl<'a> SamlAuthLauncher<'a> {
  pub fn new(server: &'a str) -> Self {
    Self {
      server,
      auth_executable: None,
      gateway: false,
      saml_request: None,
      os: None,
      host_id: None,
      client_version: None,
      certificate: None,
      sslkey: None,
      key_password: None,
      fix_openssl: false,
      ignore_tls_errors: false,
      #[cfg(feature = "webview-auth")]
      hidpi: false,
      #[cfg(feature = "webview-auth")]
      clean: false,
      #[cfg(feature = "webview-auth")]
      default_browser: false,
      browser: None,
      verbose: None,
      log_format: None,
    }
  }

  pub fn auth_executable(mut self, auth_executable: Option<&'a str>) -> Self {
    self.auth_executable = auth_executable;
    self
  }

  pub fn gateway(mut self, gateway: bool) -> Self {
    self.gateway = gateway;
    self
  }

  pub fn saml_request(mut self, saml_request: &'a str) -> Self {
    self.saml_request = Some(saml_request);
    self
  }

  pub fn os_profile(mut self, profile: &'a OsProfile) -> Self {
    self.os = Some(profile.client_os().as_str());
    self.host_id = Some(profile.host_identity().host_id());
    self.client_version = Some(profile.client_version());
    self
  }

  pub fn fix_openssl(mut self, fix_openssl: bool) -> Self {
    self.fix_openssl = fix_openssl;
    self
  }

  pub fn ignore_tls_errors(mut self, ignore_tls_errors: bool) -> Self {
    self.ignore_tls_errors = ignore_tls_errors;
    self
  }

  pub fn certificate(mut self, certificate: Option<&'a str>) -> Self {
    self.certificate = certificate;
    self
  }

  pub fn sslkey(mut self, sslkey: Option<&'a str>) -> Self {
    self.sslkey = sslkey;
    self
  }

  pub fn key_password(mut self, key_password: Option<&'a str>) -> Self {
    self.key_password = key_password;
    self
  }

  #[cfg(feature = "webview-auth")]
  pub fn hidpi(mut self, hidpi: bool) -> Self {
    self.hidpi = hidpi;
    self
  }

  #[cfg(feature = "webview-auth")]
  pub fn clean(mut self, clean: bool) -> Self {
    self.clean = clean;
    self
  }

  #[cfg(feature = "webview-auth")]
  pub fn default_browser(mut self, default_browser: bool) -> Self {
    self.default_browser = default_browser;
    self
  }

  pub fn browser(mut self, browser: Option<&'a str>) -> Self {
    self.browser = browser;
    self
  }

  /// Render the child's logs the same way as ours.
  ///
  /// gpauth inherits this process's stderr, so leaving it on the default would
  /// interleave text records into an otherwise machine-readable stream.
  ///
  /// The default format is deliberately *not* forwarded: a gpauth predating the
  /// flag rejects it as an unknown argument, and someone who never asked for
  /// JSON should not need matching binaries.
  pub fn log_format(mut self, log_format: LogFormat) -> Self {
    self.log_format = (log_format != LogFormat::Text).then_some(log_format);
    self
  }

  pub fn verbose(mut self, verbose: Option<&'a str>) -> Self {
    self.verbose = verbose;
    self
  }

  /// The command line this launcher will run.
  ///
  /// Split out from `launch` so a test can observe the argv the child actually
  /// receives: asserting on the builder's own fields would not catch an
  /// argument that never gets appended.
  fn build_command(&self, program: &PathBuf) -> Command {
    let mut auth_cmd = Command::new(program);
    auth_cmd.arg(self.server);

    if self.gateway {
      auth_cmd.arg("--gateway");
    }

    if let Some(saml_request) = self.saml_request {
      auth_cmd.arg("--saml-request").arg(saml_request);
    }

    if let Some(os) = self.os {
      auth_cmd.arg("--os").arg(os);
    }

    if let Some(host_id) = self.host_id {
      auth_cmd.arg("--host-id").arg(host_id);
    }

    if let Some(client_version) = self.client_version {
      auth_cmd.arg("--client-version").arg(client_version);
    }

    if self.fix_openssl {
      auth_cmd.arg("--fix-openssl");
    }

    if self.ignore_tls_errors {
      auth_cmd.arg("--ignore-tls-errors");
    }

    if let Some(certificate) = self.certificate {
      auth_cmd.arg("--certificate").arg(certificate);
    }

    if let Some(sslkey) = self.sslkey {
      auth_cmd.arg("--sslkey").arg(sslkey);
    }

    if let Some(key_password) = self.key_password {
      auth_cmd.arg("--key-password").arg(key_password);
    }

    #[cfg(feature = "webview-auth")]
    {
      if self.hidpi {
        auth_cmd.arg("--hidpi");
      }

      if self.clean {
        auth_cmd.arg("--clean");
      }

      if self.default_browser {
        auth_cmd.arg("--default-browser");
      }
    }

    if let Some(browser) = self.browser {
      auth_cmd.arg("--browser").arg(browser);
    }

    if let Some(log_format) = self.log_format {
      auth_cmd.arg("--log-format").arg(log_format.as_str());
    }

    if let Some(verbose) = self.verbose {
      auth_cmd.arg(verbose);
    }

    auth_cmd
  }

  /// Launch the authenticator binary as the current user or SUDO_USER if available.
  pub async fn launch(self) -> anyhow::Result<Credential> {
    let program = self
      .auth_executable
      .map(PathBuf::from)
      .unwrap_or_else(binary_paths::gpauth);
    let auth_cmd = self.build_command(&program);

    let mut non_root_cmd = auth_cmd.into_non_root()?;
    let child = non_root_cmd.kill_on_drop(true).stdout(Stdio::piped()).spawn();

    let child = match child {
      Ok(child) => child,
      Err(err) => {
        bail!("Failed to spawn {}: {}", program.display(), err);
      }
    };

    let output = child.wait_with_output().await?;

    let Ok(auth_result) = serde_json::from_slice::<SamlAuthResult>(&output.stdout) else {
      bail!("Failed to parse auth data")
    };

    Credential::try_from(auth_result)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::os_profile::{ClientOs, OsProfileBuilder};

  #[test]
  fn os_profile_sets_os_from_profile() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).client_version("6.0.0").build();

    let launcher = SamlAuthLauncher::new("portal.example.com").os_profile(&profile);

    assert_eq!(launcher.os, Some("Linux"));
  }

  #[test]
  fn os_profile_sets_host_id_from_profile_runtime_identity() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).client_version("6.0.0").build();

    let launcher = SamlAuthLauncher::new("portal.example.com").os_profile(&profile);

    assert_eq!(launcher.host_id, Some(profile.host_identity().host_id()));
  }

  #[test]
  fn os_profile_sets_client_version_from_profile() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).client_version("6.0.0").build();

    let launcher = SamlAuthLauncher::new("portal.example.com").os_profile(&profile);

    assert_eq!(launcher.client_version, Some("6.0.0"));
  }

  fn child_args(launcher: SamlAuthLauncher) -> Vec<String> {
    launcher
      .build_command(&PathBuf::from("gpauth"))
      .as_std()
      .get_args()
      .map(|arg| arg.to_string_lossy().into_owned())
      .collect()
  }

  /// gpauth writes to the same stderr as its parent, so a caller reading the
  /// parent's JSON would hit gpauth's text records mid-stream unless the format
  /// is passed down.
  #[test]
  fn json_is_forwarded_to_the_child() {
    let args = child_args(SamlAuthLauncher::new("portal.example.com").log_format(LogFormat::Json));

    assert!(
      args.windows(2).any(|pair| pair == ["--log-format", "json"]),
      "expected --log-format json in {args:?}"
    );
  }

  /// A gpauth predating the flag rejects it outright — clap treats an unknown
  /// argument as an error — so a caller who never asked for JSON must produce
  /// exactly the argv it produced before, or mismatched binaries stop working
  /// for people not using this feature at all.
  #[test]
  fn the_default_format_never_reaches_the_child() {
    for launcher in [
      SamlAuthLauncher::new("portal.example.com").log_format(LogFormat::Text),
      SamlAuthLauncher::new("portal.example.com"),
    ] {
      let args = child_args(launcher);

      assert!(
        !args.iter().any(|arg| arg == "--log-format"),
        "unexpected flag in {args:?}"
      );
    }
  }

  #[test]
  fn client_certificate_options_are_stored() {
    let launcher = SamlAuthLauncher::new("portal.example.com")
      .certificate(Some("/tmp/client.pem"))
      .sslkey(Some("/tmp/client.key"))
      .key_password(Some("secret"));

    assert_eq!(launcher.certificate, Some("/tmp/client.pem"));
    assert_eq!(launcher.sslkey, Some("/tmp/client.key"));
    assert_eq!(launcher.key_password, Some("secret"));
  }
}
