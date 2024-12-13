use clap::Parser;
use gpapi::{
  auth::{SamlAuthData, SamlAuthResult},
  clap::{args::Os, handle_error, Args},
  gp_params::{ClientOs, GpParams},
  utils::{env_utils, normalize_server, openssl},
  GP_USER_AGENT,
};
use gpauth::auth_window::AuthWindow;
use log::{info, LevelFilter};
use serde_json::json;
use tauri::RunEvent;
use tempfile::NamedTempFile;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");

#[derive(Parser, Clone)]
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

  #[arg(long, help = "The HiDPI mode, useful for high-resolution screens")]
  hidpi: bool,

  #[arg(long, help = "Get around the OpenSSL `unsafe legacy renegotiation` error")]
  fix_openssl: bool,

  #[arg(long, help = "Ignore TLS errors")]
  ignore_tls_errors: bool,

  #[arg(long, help = "Clean the cache of the embedded browser")]
  clean: bool,

  #[arg(long, help = "Use the default browser for authentication")]
  default_browser: bool,

  #[arg(
    long,
    help = "The browser to use for authentication, e.g., `default`, `firefox`, `chrome`, `chromium`, or the path to the browser executable"
  )]
  browser: Option<String>,
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
    env_utils::patch_gui_runtime_env(self.hidpi);

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

    let mut openssl_conf = self.prepare_env()?;

    let server = normalize_server(&self.server)?;
    let server: &'static str = Box::leak(server.into_boxed_str());
    let gp_params: &'static GpParams = Box::leak(Box::new(self.build_gp_params()));

    let auth_request = self.saml_request.clone().unwrap_or_default();
    let auth_request: &'static str = Box::leak(Box::new(auth_request));

    let auth_window = AuthWindow::new(&server, gp_params)
      .with_auth_request(&auth_request)
      .with_clean(self.clean);

    let browser = if let Some(browser) = self.browser.as_deref() {
      Some(browser)
    } else if self.default_browser {
      Some("default")
    } else {
      None
    };

    if browser.is_some() {
      let auth_result = auth_window.browser_authenticate(browser).await;
      print_auth_result(auth_result);

      return Ok(());
    }

    tauri::Builder::default()
      .setup(move |app| {
        let app_handle = app.handle().clone();

        tauri::async_runtime::spawn(async move {
          let auth_result = auth_window.webview_authenticate(&app_handle).await;
          print_auth_result(auth_result);
        });

        Ok(())
      })
      .build(tauri::generate_context!())?
      .run(move |_app_handle, event| {
        if let RunEvent::Exit = event {
          if let Some(file) = openssl_conf.take() {
            if let Err(err) = file.close() {
              info!("Error closing OpenSSL config file: {}", err);
            }
          }
        }
      });

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

fn init_logger() {
  env_logger::builder().filter_level(LevelFilter::Info).init();
}

pub async fn run() {
  let cli = Cli::parse();

  init_logger();
  info!("gpauth started: {}", VERSION);

  if let Err(err) = cli.run().await {
    handle_error(err, &cli);
    std::process::exit(1);
  }
}

fn print_auth_result(auth_result: anyhow::Result<SamlAuthData>) {
  let auth_result = match auth_result {
    Ok(auth_data) => SamlAuthResult::Success(auth_data),
    Err(err) => SamlAuthResult::Failure(format!("{}", err)),
  };

  println!("{}", json!(auth_result));
}
