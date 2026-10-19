use std::ffi::OsStr;

use openconnect::{Vpn, VpnBuilder};

use crate::connect::ConnectArgs;

pub(crate) fn builder(server: &str, cookie: &str, args: &ConnectArgs) -> VpnBuilder {
  let configured_script = std::env::var_os("GP_VPNC_SCRIPT");
  configure_builder(server, cookie, args, configured_script.as_deref())
}

fn configure_builder(server: &str, cookie: &str, args: &ConnectArgs, configured_script: Option<&OsStr>) -> VpnBuilder {
  let builder = Vpn::builder(server, cookie)
    .script(args.script.clone())
    .script_tun(args.script_tun);
  let has_environment_script = configured_script.is_some_and(|value| !value.is_empty());
  if args.script.is_none() && !args.script_tun && !has_environment_script {
    builder.network_configuration_handler(|configuration| {
      gpvpnc::macos::configure_network(&configuration.reason, &configuration.environment)
    })
  } else {
    builder
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::Parser;

  #[derive(Parser)]
  struct TestCli {
    #[command(flatten)]
    args: ConnectArgs,
  }

  fn args(options: &[&str]) -> ConnectArgs {
    TestCli::try_parse_from(
      ["connect", "vpn.example.com"]
        .into_iter()
        .chain(options.iter().copied()),
    )
    .unwrap()
    .args
  }

  #[test]
  fn default_does_not_require_an_installed_script() {
    for configured in [None, Some(OsStr::new(""))] {
      assert!(
        configure_builder("server", "cookie", &args(&[]), configured)
          .build()
          .is_ok()
      );
      assert!(
        configure_builder("server", "cookie", &args(&[]), configured)
          .script("/bin/sh".to_string())
          .script_tun(true)
          .build()
          .is_err()
      );
    }
  }

  #[test]
  fn explicit_commands_and_script_tunnels_remain_supported() {
    for options in [
      vec!["--script", "exec /bin/sh"],
      vec!["--script", "/bin/sh", "--script-tun"],
    ] {
      assert!(
        configure_builder("server", "cookie", &args(&options), None)
          .build()
          .is_ok()
      );
    }
  }

  #[test]
  fn environment_override_does_not_install_default_callback() {
    let builder = configure_builder("server", "cookie", &args(&[]), Some(OsStr::new("/bin/sh")));
    // Converting the selected builder to an explicit packet script would fail
    // if the embedded configuration callback had replaced the environment override.
    assert!(builder.script("/bin/sh".to_string()).script_tun(true).build().is_ok());
  }
}
