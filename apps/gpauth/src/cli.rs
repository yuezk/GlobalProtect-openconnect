use clap::Parser;
use gpapi::{
  auth::{SamlAuthData, SamlAuthResult},
  clap::args::Os,
  gp_params::{ClientOs, GpParams},
  utils::{normalize_server, openssl},
  GP_USER_AGENT,
};
use log::{info, LevelFilter};
use serde_json::json;
use tauri::{App, AppHandle, RunEvent};
use tempfile::NamedTempFile;

use crate::auth_window::{portal_prelogin, AuthWindow};

const VERSION: &str = concat!(
  env!("CARGO_PKG_VERSION"),
  " (",
  compile_time::date_str!(),
  ")"
);

#[derive(Parser, Clone)]
#[command(version = VERSION)]
struct Cli {
  server: String,
  #[arg(long)]
  saml_request: Option<String>,
  #[arg(long, default_value = GP_USER_AGENT)]
  user_agent: String,
  #[arg(long, default_value = "Linux")]
  os: Os,
  #[arg(long)]
  os_version: Option<String>,
  #[arg(long)]
  hidpi: bool,
  #[arg(long)]
  fix_openssl: bool,
  #[arg(long)]
  ignore_tls_errors: bool,
  #[arg(long)]
  clean: bool,
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
