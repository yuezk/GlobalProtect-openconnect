use std::{cell::RefCell, fs, sync::Arc};

use anyhow::bail;
use clap::Args;
use common::vpn_utils::find_csd_wrapper;
use gpapi::{
  auth::SamlAuthResult,
  clap::args::Os,
  credential::{Credential, PasswordCredential},
  error::PortalError,
  gateway::{gateway_login, GatewayLogin},
  gp_params::{ClientOs, GpParams},
  portal::{prelogin, retrieve_config, Prelogin},
  process::{
    auth_launcher::SamlAuthLauncher,
    users::{get_non_root_user, get_user_by_name},
  },
  utils::{request::RequestIdentityError, shutdown_signal},
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

  #[arg(short, long, help = "The gateway to connect to, it will prompt if not specified")]
  gateway: Option<String>,

  #[arg(short, long, help = "The username to use, it will prompt if not specified")]
  user: Option<String>,

  #[arg(long, help = "Read the password from standard input")]
  passwd_on_stdin: bool,

  #[arg(long, help = "Read the cookie from standard input")]
  cookie_on_stdin: bool,

  #[arg(long, short, help = "The VPNC script to use")]
  script: Option<String>,

  #[arg(long, help = "Connect the server as a gateway, instead of a portal")]
  as_gateway: bool,

  #[arg(
    long,
    help = "Use the default CSD wrapper to generate the HIP report and send it to the server"
  )]
  hip: bool,

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

  #[arg(long, help = "Same as the '--csd-user' option in the openconnect command")]
  csd_user: Option<String>,

  #[arg(long, help = "Same as the '--csd-wrapper' option in the openconnect command")]
  csd_wrapper: Option<String>,

  #[arg(long, default_value = "300", help = "Reconnection retry timeout in seconds")]
  reconnect_timeout: u32,

  #[arg(short, long, help = "Request MTU from server (legacy servers only)")]
  mtu: Option<u32>,

  #[arg(long, help = "Do not ask for IPv6 connectivity")]
  disable_ipv6: bool,

  #[arg(long, default_value = GP_USER_AGENT, help = "The user agent to use")]
  user_agent: String,

  #[arg(long, default_value = "Linux")]
  os: Os,

  #[arg(long)]
  os_version: Option<String>,

  #[arg(long, help = "Disable DTLS and ESP")]
  no_dtls: bool,

  #[arg(long, help = "The HiDPI mode, useful for high-resolution screens")]
  hidpi: bool,

  #[arg(long, help = "Do not reuse the remembered authentication cookie")]
  clean: bool,

  #[arg(long, help = "Use the default browser to authenticate")]
  default_browser: bool,

  #[arg(
    long,
    help = "Use the specified browser to authenticate, e.g., `default`, `firefox`, `chrome`, `chromium`, or the path to the browser executable"
  )]
  browser: Option<String>,
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
  latest_key_password: RefCell<Option<String>>,
}

impl<'a> ConnectHandler<'a> {
  pub(crate) fn new(args: &'a ConnectArgs, shared_args: &'a SharedArgs) -> Self {
    Self {
      args,
      shared_args,
      latest_key_password: Default::default(),
    }
  }

  fn build_gp_params(&self) -> GpParams {
    GpParams::builder()
      .user_agent(&self.args.user_agent)
      .client_os(ClientOs::from(&self.args.os))
      .os_version(self.args.os_version())
      .ignore_tls_errors(self.shared_args.ignore_tls_errors)
      .certificate(self.args.certificate.clone())
      .sslkey(self.args.sslkey.clone())
      .key_password(self.latest_key_password.borrow().clone())
      .build()
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    if self.args.default_browser && self.args.browser.is_some() {
      bail!("Cannot use `--default-browser` and `--browser` options at the same time");
    }

    self.latest_key_password.replace(self.args.key_password.clone());

    loop {
      let Err(err) = self.handle_impl().await else {
        return Ok(());
      };

      let Some(root_cause) = err.root_cause().downcast_ref::<RequestIdentityError>() else {
        return Err(err);
      };

      match root_cause {
        RequestIdentityError::NoKey => {
          eprintln!("ERROR: No private key found in the certificate file");
          eprintln!("ERROR: Please provide the private key file using the `-k` option");
          return Ok(());
        }
        RequestIdentityError::NoPassphrase(cert_type) | RequestIdentityError::DecryptError(cert_type) => {
          // Decrypt the private key error, ask for the key password
          let message = format!("Enter the {} passphrase:", cert_type);
          let password = Password::new(&message)
            .without_confirmation()
            .with_display_mode(PasswordDisplayMode::Masked)
            .prompt()?;

          self.latest_key_password.replace(Some(password));
        }
      }
    }
  }

  pub(crate) async fn handle_impl(&self) -> anyhow::Result<()> {
    let server = self.args.server.as_str();
    let as_gateway = self.args.as_gateway;

    if as_gateway {
      info!("Treating the server as a gateway");
      return self.connect_gateway_with_prelogin(server).await;
    }

    let Err(err) = self.connect_portal_with_prelogin(server).await else {
      return Ok(());
    };

    info!("Failed to connect portal with prelogin: {}", err);
    if err.root_cause().downcast_ref::<PortalError>().is_some() {
      info!("Trying the gateway authentication workflow...");
      self.connect_gateway_with_prelogin(server).await?;

      eprintln!("\nNOTE: the server may be a gateway, not a portal.");
      eprintln!("NOTE: try to use the `--as-gateway` option if you were authenticated twice.");

      Ok(())
    } else {
      Err(err)
    }
  }

  async fn connect_portal_with_prelogin(&self, portal: &str) -> anyhow::Result<()> {
    let gp_params = self.build_gp_params();

    let prelogin = prelogin(portal, &gp_params).await?;

    let cred = self.obtain_credential(&prelogin, portal).await?;
    let mut portal_config = retrieve_config(portal, &cred, &gp_params).await?;

    let selected_gateway = match &self.args.gateway {
      Some(gateway) => portal_config
        .find_gateway(gateway)
        .ok_or_else(|| anyhow::anyhow!("Cannot find gateway specified: {}", gateway))?,
      None => {
        portal_config.sort_gateways(prelogin.region());
        let gateways = portal_config.gateways();

        if gateways.len() > 1 {
          let gateway = Select::new("Which gateway do you want to connect to?", gateways)
            .with_vim_mode(true)
            .prompt()?;
          info!("Connecting to the selected gateway: {}", gateway);
          gateway
        } else {
          info!("Connecting to the only available gateway: {}", gateways[0]);
          gateways[0]
        }
      }
    };

    let gateway = selected_gateway.server();
    let cred = portal_config.auth_cookie().into();

    let cookie = match self.login_gateway(gateway, &cred, &gp_params).await {
      Ok(cookie) => cookie,
      Err(err) => {
        info!("Gateway login failed: {}", err);
        return self.connect_gateway_with_prelogin(gateway).await;
      }
    };

    self.connect_gateway(gateway, &cookie).await
  }

  async fn connect_gateway_with_prelogin(&self, gateway: &str) -> anyhow::Result<()> {
    info!("Performing the gateway authentication...");

    let mut gp_params = self.build_gp_params();
    gp_params.set_is_gateway(true);

    let prelogin = prelogin(gateway, &gp_params).await?;
    let cred = self.obtain_credential(&prelogin, gateway).await?;

    let cookie = self.login_gateway(gateway, &cred, &gp_params).await?;

    self.connect_gateway(gateway, &cookie).await
  }

  async fn login_gateway(&self, gateway: &str, cred: &Credential, gp_params: &GpParams) -> anyhow::Result<String> {
    let mut gp_params = gp_params.clone();

    loop {
      match gateway_login(gateway, cred, &gp_params).await? {
        GatewayLogin::Cookie(cookie) => return Ok(cookie),
        GatewayLogin::Mfa(message, input_str) => {
          let otp = Text::new(&message).prompt()?;
          gp_params.set_input_str(&input_str);
          gp_params.set_otp(&otp);

          info!("Retrying gateway login with MFA...");
        }
      }
    }
  }

  async fn connect_gateway(&self, gateway: &str, cookie: &str) -> anyhow::Result<()> {
    let mtu = self.args.mtu.unwrap_or(0);
    let csd_uid = get_csd_uid(&self.args.csd_user)?;
    let csd_wrapper = if self.args.csd_wrapper.is_some() {
      self.args.csd_wrapper.clone()
    } else if self.args.hip {
      find_csd_wrapper()
    } else {
      None
    };

    let os = ClientOs::from(&self.args.os).to_openconnect_os().to_string();
    let vpn = Vpn::builder(gateway, cookie)
      .script(self.args.script.clone())
      .user_agent(self.args.user_agent.clone())
      .os(Some(os))
      .certificate(self.args.certificate.clone())
      .sslkey(self.args.sslkey.clone())
      .key_password(self.latest_key_password.borrow().clone())
      .csd_uid(csd_uid)
      .csd_wrapper(csd_wrapper)
      .reconnect_timeout(self.args.reconnect_timeout)
      .mtu(mtu)
      .disable_ipv6(self.args.disable_ipv6)
      .no_dtls(self.args.no_dtls)
      .build()?;

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

  async fn obtain_credential(&self, prelogin: &Prelogin, server: &str) -> anyhow::Result<Credential> {
    if self.args.cookie_on_stdin {
      return read_cookie_from_stdin();
    }

    let is_gateway = prelogin.is_gateway();

    match prelogin {
      Prelogin::Saml(prelogin) => {
        let use_default_browser = prelogin.support_default_browser() && self.args.default_browser;
        let browser = if prelogin.support_default_browser() {
          self.args.browser.as_deref()
        } else {
          None
        };

        let cred = SamlAuthLauncher::new(&self.args.server)
          .gateway(is_gateway)
          .saml_request(prelogin.saml_request())
          .user_agent(&self.args.user_agent)
          .os(self.args.os.as_str())
          .os_version(Some(&self.args.os_version()))
          .hidpi(self.args.hidpi)
          .fix_openssl(self.shared_args.fix_openssl)
          .ignore_tls_errors(self.shared_args.ignore_tls_errors)
          .clean(self.args.clean)
          .default_browser(use_default_browser)
          .browser(browser)
          .launch()
          .await?;

        Ok(cred)
      }

      Prelogin::Standard(prelogin) => {
        let prefix = if is_gateway { "Gateway" } else { "Portal" };
        println!("{} ({}: {})", prelogin.auth_message(), prefix, server);

        let user = self.args.user.as_ref().map_or_else(
          || Text::new(&format!("{}:", prelogin.label_username())).prompt(),
          |user| Ok(user.to_owned()),
        )?;

        let password = if self.args.passwd_on_stdin {
          info!("Reading password from standard input");
          let mut input = String::new();
          std::io::stdin().read_line(&mut input)?;
          input.trim_end().to_owned()
        } else {
          Password::new(&format!("{}:", prelogin.label_password()))
            .without_confirmation()
            .with_display_mode(PasswordDisplayMode::Masked)
            .prompt()?
        };

        let password_cred = PasswordCredential::new(&user, &password);

        Ok(password_cred.into())
      }
    }
  }
}

fn read_cookie_from_stdin() -> anyhow::Result<Credential> {
  info!("Reading cookie from standard input");

  let mut cookie = String::new();
  std::io::stdin().read_line(&mut cookie)?;

  let Ok(auth_result) = serde_json::from_str::<SamlAuthResult>(cookie.trim_end()) else {
    bail!("Failed to parse auth data")
  };

  Credential::try_from(auth_result)
}

fn write_pid_file() {
  let pid = std::process::id();

  fs::write(GP_CLIENT_LOCK_FILE, pid.to_string()).unwrap();
  info!("Wrote PID {} to {}", pid, GP_CLIENT_LOCK_FILE);
}

fn get_csd_uid(csd_user: &Option<String>) -> anyhow::Result<u32> {
  if let Some(csd_user) = csd_user {
    get_user_by_name(csd_user).map(|user| user.uid())
  } else {
    get_non_root_user().map_or_else(|_| Ok(0), |user| Ok(user.uid()))
  }
}
