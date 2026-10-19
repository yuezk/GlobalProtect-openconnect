use gpapi::service::request::ConnectArgs;
use openconnect::{Vpn, VpnBuilder};

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> anyhow::Result<VpnBuilder> {
  anyhow::ensure!(
    args.vpnc_script().is_none(),
    "macOS does not support custom VPN scripts"
  );
  Ok(
    Vpn::builder(server, cookie).network_configuration_handler(|configuration| {
      gpvpnc::macos::configure_network(&configuration.reason, &configuration.environment)
    }),
  )
}
