use gpapi::service::request::ConnectArgs;
use openconnect::{Vpn, VpnBuilder};

use super::selection::{ScriptSource, select};

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> anyhow::Result<VpnBuilder> {
  let installed = gpservice::vpnc_script::installed_script()?;
  let legacy = args.vpnc_script();
  match select(installed.as_deref(), legacy.as_deref()) {
    ScriptSource::Installed(path) => {
      Ok(Vpn::builder(server, cookie).script_path(Some(path.to_string_lossy().into_owned())))
    }
    ScriptSource::Legacy(script) => Ok(Vpn::builder(server, cookie).script(Some(script.to_owned()))),
    ScriptSource::Default => Ok(Vpn::builder(server, cookie).script_path(None)),
  }
}
