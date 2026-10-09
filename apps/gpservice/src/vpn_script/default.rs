use gpapi::service::request::ConnectArgs;
use openconnect::{Vpn, VpnBuilder};

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> anyhow::Result<VpnBuilder> {
  Ok(Vpn::builder(server, cookie).script_path(args.vpnc_script()))
}
