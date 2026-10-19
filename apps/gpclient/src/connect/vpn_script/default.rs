use openconnect::{Vpn, VpnBuilder};

use crate::connect::ConnectArgs;

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> VpnBuilder {
  Vpn::builder(server, cookie)
    .script(args.script.clone())
    .script_tun(args.script_tun)
}
