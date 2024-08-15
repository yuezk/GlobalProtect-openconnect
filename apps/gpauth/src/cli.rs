use std::{env::temp_dir, fs, os::unix::fs::PermissionsExt};

use clap::Parser;
use gpapi::{
  auth::{SamlAuthData, SamlAuthResult},
  clap::args::Os,
  gp_params::{ClientOs, GpParams},
  process::browser_authenticator::BrowserAuthenticator,
  utils::{normalize_server, openssl},
  GP_USER_AGENT,
};
use log::{info, LevelFilter};
use serde_json::json;
use tauri::{App, AppHandle, RunEvent};
use tempfile::NamedTempFile;
use tokio::{io::AsyncReadExt, net::TcpListener};

use crate::auth_window::{portal_prelogin, AuthWindow};

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

impl Cli {
  async fn run(&mut self) -> anyhow::Result<()> {
    if self.ignore_tls_errors {
      info!("TLS errors will be ignored");
    }

    let mut openssl_conf = self.prepare_env()?;

    self.server = normalize_server(&self.server)?;
    let gp_params = self.build_gp_params();

    // Get the initial SAML request
    let saml_request = match self.saml_request {
      Some(ref saml_request) => saml_request.clone(),
      None => portal_prelogin(&self.server, &gp_params).await?,
    };

    let browser_auth = if let Some(browser) = &self.browser {
      Some(BrowserAuthenticator::new_with_browser(&saml_request, browser))
    } else if self.default_browser {
      Some(BrowserAuthenticator::new(&saml_request))
    } else {
      None
    };

    if let Some(browser_auth) = browser_auth {
      browser_auth.authenticate()?;

      info!("Please continue the authentication process in the default browser");

      let auth_result = match wait_auth_data().await {
        Ok(auth_data) => SamlAuthResult::Success(auth_data),
        Err(err) => SamlAuthResult::Failure(format!("{}", err)),
      };

      info!("Authentication completed");

      println!("{}", json!(auth_result));

      return Ok(());
    }

    self.saml_request.replace(saml_request);

    let app = create_app(self.clone())?;

    app.run(move |_app_handle, event| {
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

  fn prepare_env(&self) -> anyhow::Result<Option<NamedTempFile>> {
    std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");

    if self.hidpi {
      info!("Setting GDK_SCALE=2 and GDK_DPI_SCALE=0.5");

      std::env::set_var("GDK_SCALE", "2");
      std::env::set_var("GDK_DPI_SCALE", "0.5");
    }

    if self.fix_openssl {
      info!("Fixing OpenSSL environment");
      let file = openssl::fix_openssl_env()?;

      return Ok(Some(file));
    }

    Ok(None)
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

  async fn saml_auth(&self, app_handle: AppHandle) -> anyhow::Result<SamlAuthData> {
    let auth_window = AuthWindow::new(app_handle)
      .server(&self.server)
      .user_agent(&self.user_agent)
      .gp_params(self.build_gp_params())
      .saml_request(self.saml_request.as_ref().unwrap())
      .clean(self.clean);

    auth_window.open().await
  }
}

fn create_app(cli: Cli) -> anyhow::Result<App> {
  let app = tauri::Builder::default()
    .setup(|app| {
      let app_handle = app.handle();

      tauri::async_runtime::spawn(async move {
        let auth_result = match cli.saml_auth(app_handle.clone()).await {
          Ok(auth_data) => SamlAuthResult::Success(auth_data),
          Err(err) => SamlAuthResult::Failure(format!("{}", err)),
        };

        println!("{}", json!(auth_result));
      });
      Ok(())
    })
    .build(tauri::generate_context!())?;

  Ok(app)
}

fn init_logger() {
  env_logger::builder().filter_level(LevelFilter::Info).init();
}

pub async fn run() {
  let mut cli = Cli::parse();

  init_logger();
  info!("gpauth started: {}", VERSION);

  if let Err(err) = cli.run().await {
    eprintln!("\nError: {}", err);

    if err.to_string().contains("unsafe legacy renegotiation") && !cli.fix_openssl {
      eprintln!("\nRe-run it with the `--fix-openssl` option to work around this issue, e.g.:\n");
      // Print the command
      let args = std::env::args().collect::<Vec<_>>();
      eprintln!("{} --fix-openssl {}\n", args[0], args[1..].join(" "));
    }

    std::process::exit(1);
  }
}

async fn wait_auth_data() -> anyhow::Result<SamlAuthData> {
  // Start a local server to receive the browser authentication data
  let listener = TcpListener::bind("127.0.0.1:0").await?;
  let port = listener.local_addr()?.port();
  let port_file = temp_dir().join("gpcallback.port");

  // Write the port to a file
  fs::write(&port_file, port.to_string())?;
  fs::set_permissions(&port_file, fs::Permissions::from_mode(0o600))?;

  // Remove the previous log file
  let callback_log = temp_dir().join("gpcallback.log");
  let _ = fs::remove_file(&callback_log);

  info!("Listening authentication data on port {}", port);
  info!(
    "If it hangs, please check the logs at `{}` for more information",
    callback_log.display()
  );
  let (mut socket, _) = listener.accept().await?;

  info!("Received the browser authentication data from the socket");
  let mut data = String::new();
  socket.read_to_string(&mut data).await?;

  // Remove the port file
  fs::remove_file(&port_file)?;

  let auth_data = SamlAuthData::from_gpcallback(&data)?;
  Ok(auth_data)
}
