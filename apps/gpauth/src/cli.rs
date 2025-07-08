use auth::{auth_prelogin, BrowserAuthenticator};
use clap::Parser;
use gpapi::{
  auth::{SamlAuthData, SamlAuthResult},
  clap::{args::Os, handle_error, Args, InfoLevelVerbosity},
  gp_params::{ClientOs, GpParams},
  utils::{normalize_server, openssl},
  GP_USER_AGENT,
};
use log::info;
use serde_json::json;
use tempfile::NamedTempFile;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");

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

  #[arg(long, default_value = GP_USER_AGENT, help = "The user agent to use")]
  user_agent: String,

  #[arg(long, default_value = "Linux")]
  os: Os,

  #[arg(long)]
  os_version: Option<String>,

  #[arg(long, help = "Get around the OpenSSL `unsafe legacy renegotiation` error")]
  fix_openssl: bool,

  #[arg(long, help = "Ignore TLS errors")]
  ignore_tls_errors: bool,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "Use the default browser for authentication")]
  default_browser: bool,

  #[arg(
    long,
    help = "The browser to use for authentication, e.g., `default`, `firefox`, `chrome`, `chromium`, or the path to the browser executable"
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

    let auth_request = match self.saml_request.as_deref() {
      Some(auth_request) => auth_request.to_string(),
      None => auth_prelogin(&server, &gp_params).await?,
    };

    #[cfg(feature = "webview-auth")]
    let browser = self
      .browser
      .as_deref()
      .or_else(|| self.default_browser.then_some("default"));

    #[cfg(not(feature = "webview-auth"))]
    let browser = self.browser.as_deref().or(Some("default"));

    if let Some(browser) = browser {
      let authenticator = BrowserAuthenticator::new(&auth_request, browser);
      let auth_result = authenticator.authenticate().await;

      print_auth_result(auth_result);

      // explicitly drop openssl_conf to avoid the unused variable warning
      drop(openssl_conf);
      return Ok(());
    }

    #[cfg(feature = "webview-auth")]
    crate::webview_auth::authenticate(server, gp_params, auth_request, self.clean, openssl_conf).await?;

    Ok(())
  }

  fn build_gp_params(&self) -> GpParams {
    let gp_params = GpParams::builder()
      .user_agent(&self.user_agent)
      .client_os(ClientOs::from(&self.os))
      .os_version(self.os_version.clone())
      .ignore_tls_errors(self.ignore_tls_errors)
      .is_gateway(self.gateway)
      .build();

    gp_params
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

pub fn print_auth_result(auth_result: anyhow::Result<SamlAuthData>) {
  let auth_result = match auth_result {
    Ok(auth_data) => SamlAuthResult::Success(auth_data),
    Err(err) => SamlAuthResult::Failure(format!("{}", err)),
  };

  println!("{}", json!(auth_result));
}
