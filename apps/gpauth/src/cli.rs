use auth::{BrowserAuthenticator, auth_prelogin};
use clap::Parser;
use gpapi::{
  auth::{AuthenticationCancelled, SamlAuthData, SamlAuthResult},
  clap::{Args, InfoLevelVerbosity, args::Os, handle_error},
  gp_params::GpParams,
  os_profile::{ClientOs, OsProfile},
  utils::{normalize_server, openssl},
};
use log::info;
use serde_json::json;
use tempfile::NamedTempFile;

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  env!("GPAUTH_GIT_COMMIT"),
  " ",
  compile_time::date_str!(),
  ")"
);

#[derive(Parser)]
#[command(
  version = VERSION,
  author,
  about = "The authentication component for the GlobalProtect VPN client, supports the SSO authentication method.",
  help_template = "\
{before-help}{name} {version}
{author}

{about}

{usage-heading} {usage}

{all-args}{after-help}

See 'gpauth -h' for more information.
"
)]
struct Cli {
  #[arg(help = "The portal server to authenticate")]
  server: String,

  #[arg(long, help = "Treating the server as a gateway")]
  gateway: bool,

  #[arg(long, help = "The SAML authentication request")]
  saml_request: Option<String>,

  #[arg(long, value_enum, default_value_t = Os::default())]
  os: Os,

  #[arg(long, help = "Use this runtime host ID when building the OS profile")]
  host_id: Option<String>,

  #[arg(long, help = "Override the GlobalProtect client version reported to the server")]
  client_version: Option<String>,

  #[arg(
    short,
    long,
    help = "Use SSL client certificate file in pkcs#8 (.pem) or pkcs#12 (.p12, .pfx) format"
  )]
  certificate: Option<String>,

  #[arg(short = 'k', long, help = "Use SSL private key file in pkcs#8 (.pem) format")]
  sslkey: Option<String>,

  #[arg(short = 'p', long, help = "The key passphrase of the private key")]
  key_password: Option<String>,

  #[arg(long, help = "Get around the OpenSSL `unsafe legacy renegotiation` error")]
  fix_openssl: bool,

  #[arg(long, help = "Ignore TLS errors")]
  ignore_tls_errors: bool,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "Use the default browser for authentication")]
  default_browser: bool,

  #[arg(
    long,
    help = "Use external browser authentication. With no value, auto-select Chrome, Firefox, then system default. Use `default` for the system default browser.",
    default_missing_value = "auto",
    num_args=0..=1
  )]
  browser: Option<String>,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "The HiDPI mode, useful for high-resolution screens")]
  hidpi: bool,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "Clean the cache of the embedded browser")]
  pub clean: bool,

  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Args for Cli {
  fn fix_openssl(&self) -> bool {
    self.fix_openssl
  }

  fn ignore_tls_errors(&self) -> bool {
    self.ignore_tls_errors
  }
}

impl Cli {
  fn prepare_env(&self) -> anyhow::Result<Option<NamedTempFile>> {
    #[cfg(feature = "webview-auth")]
    gpapi::utils::env_utils::patch_gui_runtime_env(self.hidpi);

    if self.fix_openssl {
      info!("Fixing OpenSSL environment");
      let file = openssl::fix_openssl_env()?;

      return Ok(Some(file));
    }

    Ok(None)
  }

  async fn run(&self) -> anyhow::Result<()> {
    if self.ignore_tls_errors {
      info!("TLS errors will be ignored");
    }

    let openssl_conf = self.prepare_env()?;

    let server = normalize_server(&self.server)?;
    let gp_params = self.build_gp_params();
    info!(
      "gpauth auth host-id: {}",
      gp_params.os_profile().host_identity().host_id()
    );

    let auth_request = match self.saml_request.as_deref() {
      Some(auth_request) => auth_request.to_string(),
      None => auth_prelogin(&server, &gp_params, self.external_browser_requested(), self.gateway).await?,
    };

    #[cfg(feature = "webview-auth")]
    let browser = self
      .browser
      .as_deref()
      .or_else(|| self.default_browser.then(|| "default"));

    #[cfg(not(feature = "webview-auth"))]
    let browser = self.browser.as_deref().or(Some("default"));

    if let Some(browser) = browser {
      let auth_host_id = gp_params.os_profile().host_identity().host_id().to_string();
      let authenticator = BrowserAuthenticator::new(&auth_request, browser);
      let auth_result = authenticator.authenticate().await;

      print_auth_result(auth_result, Some(&auth_host_id));

      // explicitly drop openssl_conf to avoid the unused variable warning
      drop(openssl_conf);
      return Ok(());
    }

    #[cfg(feature = "webview-auth")]
    crate::webview_auth::authenticate(server, gp_params, auth_request, self.clean, openssl_conf).await?;

    Ok(())
  }

  fn build_gp_params(&self) -> GpParams {
    GpParams::builder(self.build_os_profile())
      .ignore_tls_errors(self.ignore_tls_errors)
      .certificate(self.certificate.clone())
      .sslkey(self.sslkey.clone())
      .key_password(self.key_password.clone())
      .is_gateway(self.gateway)
      .build()
  }

  fn build_os_profile(&self) -> OsProfile {
    let mut builder = OsProfile::builder(ClientOs::from(self.os));
    if let Some(host_id) = self.host_id.as_deref() {
      builder = builder.host_id_override(host_id);
    }
    if let Some(client_version) = self.client_version.as_deref() {
      builder = builder.client_version(client_version);
    }
    builder.build()
  }

  fn external_browser_requested(&self) -> bool {
    #[cfg(feature = "webview-auth")]
    {
      self.default_browser || self.browser.is_some()
    }

    #[cfg(not(feature = "webview-auth"))]
    {
      true
    }
  }
}

fn init_logger(cli: &Cli) {
  env_logger::builder()
    .filter_level(cli.verbose.log_level_filter())
    .init();
}

pub async fn run() {
  let cli = Cli::parse();

  init_logger(&cli);
  info!("gpauth started: {}", VERSION);

  if let Err(err) = cli.run().await {
    handle_error(err, &cli);
    std::process::exit(1);
  }
}

pub fn print_auth_result(auth_result: anyhow::Result<SamlAuthData>, host_id: Option<&str>) {
  let auth_result = match auth_result {
    Ok(auth_data) => {
      let auth_data = match host_id {
        Some(host_id) => auth_data.with_host_id(host_id),
        None => auth_data,
      };
      SamlAuthResult::Success(auth_data)
    }
    Err(err) if err.downcast_ref::<AuthenticationCancelled>().is_some() => SamlAuthResult::Cancelled,
    Err(err) => SamlAuthResult::Failure(format!("{}", err)),
  };

  println!("{}", json!(auth_result));
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn os_defaults_to_runtime_os() {
    let cli = Cli::try_parse_from(["gpauth", "portal.example.com"]).expect("gpauth args should parse");

    assert_eq!(cli.os, Os::default());
  }

  #[test]
  fn host_id_arg_sets_profile_host_id_seed() {
    let cli = Cli::try_parse_from(["gpauth", "portal.example.com", "--host-id", "host-seed"])
      .expect("gpauth args should parse");
    let profile = cli.build_os_profile();

    assert_eq!(profile.host_identity().host_id(), "host-seed");
  }

  #[test]
  fn browser_arg_requests_external_browser() {
    let cli =
      Cli::try_parse_from(["gpauth", "portal.example.com", "--browser", "firefox"]).expect("gpauth args should parse");

    assert!(cli.external_browser_requested());
  }

  #[test]
  fn browser_without_value_uses_auto_mode() {
    let cli = Cli::try_parse_from(["gpauth", "portal.example.com", "--browser"]).expect("gpauth args should parse");

    assert_eq!(cli.browser.as_deref(), Some("auto"));
    assert!(cli.external_browser_requested());
  }

  #[test]
  fn browser_default_keeps_system_default_mode() {
    let cli =
      Cli::try_parse_from(["gpauth", "portal.example.com", "--browser", "default"]).expect("gpauth args should parse");

    assert_eq!(cli.browser.as_deref(), Some("default"));
    assert!(cli.external_browser_requested());
  }

  #[test]
  fn client_version_arg_sets_profile_client_version() {
    let cli = Cli::try_parse_from(["gpauth", "portal.example.com", "--client-version", "legacy-client"])
      .expect("gpauth args should parse");
    let profile = cli.build_os_profile();

    assert_eq!(profile.client_version(), "legacy-client");
    assert!(profile.user_agent().contains("legacy-client"));
  }

  #[test]
  fn client_certificate_args_parse() {
    let cli = Cli::try_parse_from([
      "gpauth",
      "portal.example.com",
      "--certificate",
      "/tmp/client.pem",
      "--sslkey",
      "/tmp/client.key",
      "--key-password",
      "secret",
    ])
    .expect("gpauth args should parse");

    assert_eq!(cli.certificate.as_deref(), Some("/tmp/client.pem"));
    assert_eq!(cli.sslkey.as_deref(), Some("/tmp/client.key"));
    assert_eq!(cli.key_password.as_deref(), Some("secret"));
  }
}
