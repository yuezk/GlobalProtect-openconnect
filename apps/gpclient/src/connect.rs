use std::{fs, sync::Arc};

use clap::Args;
use gpapi::{
  clap::args::Os,
  credential::{Credential, PasswordCredential},
  gateway::gateway_login,
  gp_params::{ClientOs, GpParams},
  portal::{prelogin, retrieve_config, Prelogin},
  process::auth_launcher::SamlAuthLauncher,
  utils::{self, shutdown_signal},
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

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    let portal = utils::normalize_server(self.args.server.as_str())?;

    let gp_params = GpParams::builder()
      .user_agent(&self.args.user_agent)
      .client_os(ClientOs::from(&self.args.os))
      .os_version(self.args.os_version())
      .ignore_tls_errors(self.shared_args.ignore_tls_errors)
      .build();

    let prelogin = prelogin(&portal, &gp_params).await?;
    let portal_credential = self.obtain_portal_credential(&prelogin).await?;
    let mut portal_config = retrieve_config(&portal, &portal_credential, &gp_params).await?;

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
    let token = gateway_login(gateway, &cred, &gp_params).await?;

    let vpn = Vpn::builder(gateway, &token)
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

  async fn obtain_portal_credential(&self, prelogin: &Prelogin) -> anyhow::Result<Credential> {
    match prelogin {
      Prelogin::Saml(prelogin) => {
        SamlAuthLauncher::new(&self.args.server)
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
        println!("{}", prelogin.auth_message());

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
