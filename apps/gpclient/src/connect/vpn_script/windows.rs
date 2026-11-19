use std::sync::Mutex;

use openconnect::{Vpn, VpnBuilder};

use crate::connect::ConnectHandler;

impl ConnectHandler<'_> {
  pub(in crate::connect) fn vpn_builder(&self, server: &str, cookie: &str) -> anyhow::Result<VpnBuilder> {
    let owner = self.shared_args.system_tunnel.borrow();
    let tunnel = owner
      .as_ref()
      .ok_or_else(|| anyhow::anyhow!("System tunnel ownership is unavailable"))?;
    let network = Mutex::new(tunnel.configurator()?);
    Ok(
      Vpn::builder(server, cookie).network_configuration_handler(move |configuration| {
        network
          .lock()
          .map_err(|_| std::io::Error::other("Windows network state is unavailable"))?
          .configure(&configuration.reason, &configuration.environment)
      }),
    )
  }
}
