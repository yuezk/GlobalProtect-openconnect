use openconnect::{Vpn, VpnBuilder};

use crate::connect::ConnectHandler;

impl ConnectHandler<'_> {
  pub(in crate::connect) fn vpn_builder(&self, server: &str, cookie: &str) -> anyhow::Result<VpnBuilder> {
    Ok(
      Vpn::builder(server, cookie)
        .script(self.args.script.clone())
        .script_tun(self.args.script_tun),
    )
  }
}
