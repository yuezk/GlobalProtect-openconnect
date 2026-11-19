use gpapi::service::request::ConnectArgs;
use gpvpnc::windows::SystemTunnel;
use openconnect::{Vpn, VpnBuilder};
use std::sync::Mutex;
pub(crate) fn recover_network() -> std::io::Result<()> {
  let _tunnel = SystemTunnel::try_acquire()?;
  Ok(())
}

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> anyhow::Result<VpnBuilder> {
  anyhow::ensure!(
    args.vpnc_script().is_none(),
    "Windows does not support custom VPN scripts"
  );
  let network = Mutex::new(SystemTunnel::acquire()?.configurator()?);
  Ok(
    Vpn::builder(server, cookie).network_configuration_handler(move |configuration| {
      network
        .lock()
        .map_err(|_| std::io::Error::other("Windows network state is unavailable"))?
        .configure(&configuration.reason, &configuration.environment)
    }),
  )
}
