use std::{
  net::IpAddr,
  path::{Path, PathBuf},
};

use anyhow::bail;
use common::binary_paths;
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

#[cfg(feature = "webview-auth")]
use crate::auth::AuthWindowTheme;
use crate::{auth::SamlAuthResult, credential::Credential, log_format::LogFormat, os_profile::OsProfile};

use super::{
  command_runner::{ChildInput, run_observed},
  command_traits::CommandExt,
};

use super::rendering_recovery::{RenderingCrash, StderrDiagnostic, eligible, is_crash_exit};

const MAX_AUTH_RESULT_BYTES: usize = 256 * 1024;

pub struct SamlAuthLauncher<'a> {
  cancellation: CancellationToken,
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
  #[cfg(not(target_os = "macos"))]
  fix_openssl: bool,
  ignore_tls_errors: bool,
  #[cfg(feature = "webview-auth")]
  hidpi: bool,
  #[cfg(feature = "webview-auth")]
  clean: bool,
  #[cfg(feature = "webview-auth")]
  default_browser: bool,
  #[cfg(feature = "webview-auth")]
  window_title: Option<&'a str>,
  #[cfg(feature = "webview-auth")]
  window_theme: Option<AuthWindowTheme>,
  browser: Option<&'a str>,
  browser_listen: Option<IpAddr>,
  verbose: Option<&'a str>,
  log_format: LogFormat,
  dmabuf_disabled: bool,
}

impl<'a> SamlAuthLauncher<'a> {
  pub fn new(server: &'a str) -> Self {
    Self {
      cancellation: CancellationToken::new(),
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
      #[cfg(not(target_os = "macos"))]
      fix_openssl: false,
      ignore_tls_errors: false,
      #[cfg(feature = "webview-auth")]
      hidpi: false,
      #[cfg(feature = "webview-auth")]
      clean: false,
      #[cfg(feature = "webview-auth")]
      default_browser: false,
      #[cfg(feature = "webview-auth")]
      window_title: None,
      #[cfg(feature = "webview-auth")]
      window_theme: None,
      browser: None,
      browser_listen: None,
      verbose: None,
      log_format: LogFormat::Text,
      dmabuf_disabled: false,
    }
  }

  pub fn dmabuf_disabled(mut self, disabled: bool) -> Self {
    self.dmabuf_disabled = disabled;
    self
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

  #[cfg(not(target_os = "macos"))]
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

  #[cfg(feature = "webview-auth")]
  pub fn window_title(mut self, window_title: &'a str) -> Self {
    self.window_title = Some(window_title);
    self
  }

  #[cfg(feature = "webview-auth")]
  pub fn window_theme(mut self, window_theme: AuthWindowTheme) -> Self {
    self.window_theme = Some(window_theme);
    self
  }

  pub fn browser(mut self, browser: Option<&'a str>) -> Self {
    self.browser = browser;
    self
  }

  pub fn browser_listen(mut self, browser_listen: Option<IpAddr>) -> Self {
    self.browser_listen = browser_listen;
    self
  }

  /// Render the child's logs the same way as ours.
  ///
  /// gpauth inherits this process's stderr, so both processes must use the same
  /// format to keep the stream consistent.
  pub fn log_format(mut self, log_format: LogFormat) -> Self {
    self.log_format = log_format;
    self
  }

  pub fn verbose(mut self, verbose: Option<&'a str>) -> Self {
    self.verbose = verbose;
    self
  }

  pub fn cancellation(mut self, cancellation: &CancellationToken) -> Self {
    self.cancellation = cancellation.clone();
    self
  }

  /// The command line this launcher will run.
  ///
  /// Split out from `launch` so a test can observe the argv the child actually
  /// receives: asserting on the builder's own fields would not catch an
  /// argument that never gets appended.
  fn build_command(&self, program: &Path) -> Command {
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

    #[cfg(not(target_os = "macos"))]
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

      if let Some(window_title) = self.window_title {
        auth_cmd.arg("--window-title").arg(window_title);
      }

      if let Some(window_theme) = self.window_theme {
        auth_cmd.arg("--window-theme").arg(window_theme.as_str());
      }
    }

    if let Some(browser) = self.browser {
      auth_cmd.arg("--browser").arg(browser);
    }

    if let Some(browser_listen) = self.browser_listen {
      auth_cmd.arg("--browser-listen").arg(browser_listen.to_string());
    }

    auth_cmd.arg("--log-format").arg(self.log_format.as_str());

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

    let mut command = auth_cmd.into_non_root()?.into_std();
    let embedded = cfg!(feature = "webview-auth") && self.browser.is_none();
    #[cfg(feature = "webview-auth")]
    let embedded = embedded && !self.default_browser;
    let bundled = is_bundled_authenticator(&program, &binary_paths::gpauth());
    let can_recover = embedded && bundled && eligible(&command);
    if embedded && bundled && self.dmabuf_disabled {
      command.env("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
    let stdin = auth_stdin(self.browser);
    let cancellation = self.cancellation.clone();
    let output = tokio::task::spawn_blocking(move || {
      let check = || {
        if cancellation.is_cancelled() {
          Err(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "Authentication cancelled",
          ))
        } else {
          Ok(())
        }
      };
      let mut diagnostic = StderrDiagnostic::default();
      let output = run_observed(
        command,
        &check,
        Some(MAX_AUTH_RESULT_BYTES),
        &mut |bytes| diagnostic.push(bytes),
        &mut std::io::stderr(),
        stdin,
      )?;
      Ok::<_, std::io::Error>((output.output, diagnostic.matched()))
    })
    .await?;
    let (output, rendering_diagnostic) = match output {
      Ok(output) => output,
      Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
        return Err(crate::auth::AuthenticationCancelled.into());
      }
      Err(error) => return Err(error.into()),
    };

    if self.cancellation.is_cancelled() {
      return Err(crate::auth::AuthenticationCancelled.into());
    }
    decode_auth_output(output, can_recover && rendering_diagnostic)
  }
}

/// `gpauth --browser manual` reads the pasted `globalprotectcallback:` data from the user's terminal.
fn auth_stdin(browser: Option<&str>) -> ChildInput<'static> {
  match browser {
    Some(browser) if browser.eq_ignore_ascii_case("manual") || browser.eq_ignore_ascii_case("remote") => {
      ChildInput::Terminal
    }
    _ => ChildInput::Null,
  }
}

fn is_bundled_authenticator(program: &Path, bundled: &Path) -> bool {
  match (program.canonicalize(), bundled.canonicalize()) {
    (Ok(program), Ok(bundled)) => program == bundled,
    _ => false,
  }
}

fn decode_auth_output(output: std::process::Output, rendering_failure: bool) -> anyhow::Result<Credential> {
  if !output.status.success() {
    if rendering_failure && is_crash_exit(output.status) {
      return Err(RenderingCrash { status: output.status }.into());
    }
    bail!("Authenticator exited unsuccessfully: {}", output.status);
  }
  let Ok(auth_result) = serde_json::from_slice::<SamlAuthResult>(&output.stdout) else {
    bail!("Failed to parse auth data")
  };
  Credential::try_from(auth_result)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::os_profile::{ClientOs, OsProfileBuilder};

  #[test]
  fn server_tls_result_survives_process_decoding_as_a_typed_error() {
    use crate::error::{ServerTlsError, ServerTlsErrorKind};
    use std::os::unix::process::ExitStatusExt;
    let output = std::process::Output {
      status: std::process::ExitStatus::from_raw(0),
      stdout: br#"{"serverTlsError":{"server":"https://vpn.example.com:8443","kind":"certificate"}}"#.to_vec(),
      stderr: Vec::new(),
    };
    let error = decode_auth_output(output, false).unwrap_err();
    assert_eq!(
      error.downcast_ref::<ServerTlsError>(),
      Some(&ServerTlsError {
        server: "https://vpn.example.com:8443".into(),
        kind: ServerTlsErrorKind::Certificate,
      })
    );
  }

  #[test]
  fn client_certificate_required_result_survives_process_decoding() {
    use crate::error::{ServerTlsError, ServerTlsErrorKind};
    use std::os::unix::process::ExitStatusExt;
    let output = std::process::Output {
      status: std::process::ExitStatus::from_raw(0),
      stdout: br#"{"serverTlsError":{"server":"https://vpn.example.com","kind":"clientCertificateRequired"}}"#.to_vec(),
      stderr: Vec::new(),
    };
    let error = decode_auth_output(output, false).unwrap_err();
    let tls = error.downcast_ref::<ServerTlsError>().unwrap();
    assert_eq!(tls.kind, ServerTlsErrorKind::ClientCertificateRequired);
    assert_eq!(tls.server, "https://vpn.example.com");
  }

  #[test]
  fn launches_with_platform_tls_options() {
    let launcher = SamlAuthLauncher::new("vpn.example.com").ignore_tls_errors(true);
    #[cfg(not(target_os = "macos"))]
    let launcher = launcher.fix_openssl(true);
    let args = child_args(launcher);
    assert_eq!(
      args.iter().any(|arg| arg == "--fix-openssl"),
      !cfg!(target_os = "macos")
    );
    assert!(args.iter().any(|arg| arg == "--ignore-tls-errors"));
  }

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
      .build_command(Path::new("gpauth"))
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

  /// The launcher always passes the selected format so the parent and child
  /// cannot silently drift onto different output formats.
  #[test]
  fn text_is_forwarded_to_the_child() {
    for launcher in [
      SamlAuthLauncher::new("portal.example.com").log_format(LogFormat::Text),
      SamlAuthLauncher::new("portal.example.com"),
    ] {
      let args = child_args(launcher);

      assert!(
        args.windows(2).any(|pair| pair == ["--log-format", "text"]),
        "expected --log-format text in {args:?}"
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

  #[test]
  fn browser_listen_address_is_stored() {
    let listen_ip = "192.168.107.15".parse().unwrap();
    let launcher = SamlAuthLauncher::new("portal.example.com").browser_listen(Some(listen_ip));

    assert_eq!(launcher.browser_listen, Some(listen_ip));
  }

  #[test]
  fn only_a_manual_browser_reads_the_terminal() {
    assert!(matches!(auth_stdin(Some("manual")), ChildInput::Terminal));
    assert!(matches!(auth_stdin(Some("MANUAL")), ChildInput::Terminal));
    assert!(matches!(auth_stdin(Some("remote")), ChildInput::Terminal));
    assert!(matches!(auth_stdin(Some("REMOTE")), ChildInput::Terminal));
    assert!(matches!(auth_stdin(Some("firefox")), ChildInput::Null));
    assert!(matches!(auth_stdin(None), ChildInput::Null));
  }

  #[test]
  fn failed_authenticator_discards_partial_or_valid_stdout_before_decoding() {
    use std::os::unix::process::ExitStatusExt;
    for stdout in [b"{partial".to_vec(), br#"{"cancelled":null}"#.to_vec()] {
      let error = decode_auth_output(
        std::process::Output {
          status: std::process::ExitStatus::from_raw(71 << 8),
          stdout,
          stderr: Vec::new(),
        },
        true,
      )
      .unwrap_err();
      assert!(error.downcast_ref::<RenderingCrash>().is_some());
    }
  }

  #[test]
  fn successful_malformed_auth_output_is_not_a_rendering_crash() {
    use std::os::unix::process::ExitStatusExt;
    let error = decode_auth_output(
      std::process::Output {
        status: std::process::ExitStatus::from_raw(0),
        stdout: b"partial".to_vec(),
        stderr: Vec::new(),
      },
      true,
    )
    .unwrap_err();
    assert!(error.downcast_ref::<RenderingCrash>().is_none());
    assert_eq!(error.to_string(), "Failed to parse auth data");
  }

  #[tokio::test]
  async fn custom_authenticator_crash_does_not_request_recovery() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("auth-fixture");
    std::fs::write(
      &executable,
      "#!/bin/sh\nprintf 'Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.\\n' >&2\nexit 1\n",
    )
    .unwrap();
    // The launcher drops root privileges in CI; its child must be able to
    // traverse the fixture directory and execute this non-sensitive script.
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = SamlAuthLauncher::new("fixture.invalid")
      .auth_executable(executable.to_str())
      .dmabuf_disabled(true)
      .launch()
      .await
      .unwrap_err();
    assert!(error.downcast_ref::<RenderingCrash>().is_none());
    assert!(
      error.to_string().starts_with("Authenticator exited unsuccessfully:"),
      "unexpected launch error: {error:#}"
    );
  }
  #[test]
  fn service_provided_bundled_path_is_not_a_custom_authenticator() {
    let directory = tempfile::tempdir().unwrap();
    let bundled = directory.path().join("gpauth");
    let custom = directory.path().join("custom");
    std::fs::write(&bundled, "bundled").unwrap();
    std::fs::write(&custom, "custom").unwrap();
    assert!(is_bundled_authenticator(&bundled, &bundled));
    assert!(!is_bundled_authenticator(&custom, &bundled));
    let alias = directory.path().join("auth-alias");
    std::os::unix::fs::symlink(&bundled, &alias).unwrap();
    assert!(is_bundled_authenticator(&alias, &bundled));
  }
}
