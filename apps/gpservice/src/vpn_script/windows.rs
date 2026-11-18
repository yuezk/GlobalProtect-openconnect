use gpapi::service::request::ConnectArgs;
use openconnect::{Vpn, VpnBuilder};
use std::sync::Mutex;
const RECOVERY_KEY: &str = r"SOFTWARE\GP Connect\NetworkRecovery";

pub(crate) fn recover_network() -> std::io::Result<()> {
  gpvpnc::windows::recover(RECOVERY_KEY)
}

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> anyhow::Result<VpnBuilder> {
  anyhow::ensure!(
    args.vpnc_script().is_none(),
    "Windows does not support custom VPN scripts"
  );
  let network = Mutex::new(gpvpnc::windows::NetworkConfigurator::new(RECOVERY_KEY)?);
  Ok(
    Vpn::builder(server, cookie).network_configuration_handler(move |configuration| {
      network
        .lock()
        .map_err(|_| std::io::Error::other("Windows network state is unavailable"))?
        .configure(&configuration.reason, &configuration.environment)
    }),
  )
}
