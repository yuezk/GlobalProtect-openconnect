use std::{fs, sync::Arc};

use clap::Args;
use gpapi::{
  clap::args::Os,
  credential::{Credential, PasswordCredential},
  gateway::gateway_login,
  gp_params::{ClientOs, GpParams},
  portal::{prelogin, retrieve_config, PortalError, Prelogin},
  process::auth_launcher::SamlAuthLauncher,
  utils::shutdown_signal,
  GP_USER_AGENT,
};
use inquire::{Password, PasswordDisplayMode, Select, Text};
use log::info;
use openconnect::Vpn;

use crate::{cli::SharedArgs, GP_CLIENT_LOCK_FILE};

#[derive(Args)]
pub(crate) struct ConnectArgs {
  #[arg(help = "The portal server to connect to")]
  server: String,
  #[arg(
    short,
    long,
    help = "The gateway to connect to, it will prompt if not specified"
  )]
  gateway: Option<String>,
  #[arg(
    short,
    long,
    help = "The username to use, it will prompt if not specified"
  )]
  user: Option<String>,
  #[arg(long, short, help = "The VPNC script to use")]
  script: Option<String>,
  #[arg(long, default_value = GP_USER_AGENT, help = "The user agent to use")]
  user_agent: String,
  #[arg(long, default_value = "Linux")]
  os: Os,
  #[arg(long)]
  os_version: Option<String>,
  #[arg(long, help = "The HiDPI mode, useful for high resolution screens")]
  hidpi: bool,
  #[arg(long, help = "Do not reuse the remembered authentication cookie")]
  clean: bool,
}

impl ConnectArgs {
  fn os_version(&self) -> String {
    if let Some(os_version) = &self.os_version {
      return os_version.to_owned();
    }

    match self.os {
      Os::Linux => format!("Linux {}", whoami::distro()),
      Os::Windows => String::from("Microsoft Windows 11 Pro , 64-bit"),
      Os::Mac => String::from("Apple Mac OS X 13.4.0"),
    }
  }
}

pub(crate) struct ConnectHandler<'a> {
  args: &'a ConnectArgs,
  shared_args: &'a SharedArgs,
}

impl<'a> ConnectHandler<'a> {
  pub(crate) fn new(args: &'a ConnectArgs, shared_args: &'a SharedArgs) -> Self {
    Self { args, shared_args }
  }

  fn build_gp_params(&self) -> GpParams {
    GpParams::builder()
      .user_agent(&self.args.user_agent)
      .client_os(ClientOs::from(&self.args.os))
      .os_version(self.args.os_version())
      .ignore_tls_errors(self.shared_args.ignore_tls_errors)
      .build()
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    let server = self.args.server.as_str();

    let Err(err) = self.connect_portal_with_prelogin(server).await else {
      return Ok(());
    };

    info!("Failed to connect portal with prelogin: {}", err);
    if err.root_cause().downcast_ref::<PortalError>().is_some() {
      info!("Trying the gateway authentication workflow...");
      return self.connect_gateway_with_prelogin(server).await;
    }

    Err(err)
  }

  async fn connect_portal_with_prelogin(&self, portal: &str) -> anyhow::Result<()> {
    let gp_params = self.build_gp_params();

    let prelogin = prelogin(portal, &gp_params).await?;

    let cred = self.obtain_credential(&prelogin, portal).await?;
    let mut portal_config = retrieve_config(portal, &cred, &gp_params).await?;

    let selected_gateway = match &self.args.gateway {
      Some(gateway) => portal_config
        .find_gateway(gateway)
        .ok_or_else(|| anyhow::anyhow!("Cannot find gateway {}", gateway))?,
      None => {
        portal_config.sort_gateways(prelogin.region());
        let gateways = portal_config.gateways();

        if gateways.len() > 1 {
          Select::new("Which gateway do you want to connect to?", gateways)
            .with_vim_mode(true)
            .prompt()?
        } else {
          gateways[0]
        }
      }
    };

    let gateway = selected_gateway.server();
    let cred = portal_config.auth_cookie().into();

    let cookie = match gateway_login(gateway, &cred, &gp_params).await {
      Ok(cookie) => cookie,
      Err(err) => {
        info!("Gateway login failed: {}", err);
        return self.connect_gateway_with_prelogin(gateway).await;
      }
    };

    self.connect_gateway(gateway, &cookie).await
  }

  async fn connect_gateway_with_prelogin(&self, gateway: &str) -> anyhow::Result<()> {
    let mut gp_params = self.build_gp_params();
    gp_params.set_is_gateway(true);

    let prelogin = prelogin(gateway, &gp_params).await?;
    let cred = self.obtain_credential(&prelogin, &gateway).await?;

    let cookie = gateway_login(gateway, &cred, &gp_params).await?;

    self.connect_gateway(gateway, &cookie).await
  }

  async fn connect_gateway(&self, gateway: &str, cookie: &str) -> anyhow::Result<()> {
    let vpn = Vpn::builder(gateway, cookie)
      .user_agent(self.args.user_agent.clone())
      .script(self.args.script.clone())
      .build();

    let vpn = Arc::new(vpn);
    let vpn_clone = vpn.clone();

    // Listen for the interrupt signal in the background
    tokio::spawn(async move {
      shutdown_signal().await;
      info!("Received the interrupt signal, disconnecting...");
      vpn_clone.disconnect();
    });

    vpn.connect(write_pid_file);

    if fs::metadata(GP_CLIENT_LOCK_FILE).is_ok() {
      info!("Removing PID file");
      fs::remove_file(GP_CLIENT_LOCK_FILE)?;
    }

    Ok(())
  }

  async fn obtain_credential(
    &self,
    prelogin: &Prelogin,
    server: &str,
  ) -> anyhow::Result<Credential> {
    let is_gateway = prelogin.is_gateway();

    match prelogin {
      Prelogin::Saml(prelogin) => {
        SamlAuthLauncher::new(&self.args.server)
          .gateway(is_gateway)
          .saml_request(prelogin.saml_request())
          .user_agent(&self.args.user_agent)
          .os(self.args.os.as_str())
          .os_version(Some(&self.args.os_version()))
          .hidpi(self.args.hidpi)
          .fix_openssl(self.shared_args.fix_openssl)
          .ignore_tls_errors(self.shared_args.ignore_tls_errors)
          .clean(self.args.clean)
          .launch()
          .await
      }
      Prelogin::Standard(prelogin) => {
        let prefix = if is_gateway { "Gateway" } else { "Portal" };
        println!("{} ({}: {})", prelogin.auth_message(), prefix, server);

        let user = self.args.user.as_ref().map_or_else(
          || Text::new(&format!("{}:", prelogin.label_username())).prompt(),
          |user| Ok(user.to_owned()),
        )?;
        let password = Password::new(&format!("{}:", prelogin.label_password()))
          .without_confirmation()
          .with_display_mode(PasswordDisplayMode::Masked)
          .prompt()?;

        let password_cred = PasswordCredential::new(&user, &password);

        Ok(password_cred.into())
      }
    }
  }
}

fn write_pid_file() {
  let pid = std::process::id();

  fs::write(GP_CLIENT_LOCK_FILE, pid.to_string()).unwrap();
  info!("Wrote PID {} to {}", pid, GP_CLIENT_LOCK_FILE);
}
