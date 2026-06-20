use clap::Args;
use gpapi::{
  clap::args::Os,
  cookie_store,
  gp_params::CscMode,
  os_profile::{ClientOs, OsProfile},
};
use log::warn;
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct ConnectArgs {
  #[arg(help = "The portal server to connect to")]
  pub(super) server: String,

  #[arg(short, long, help = "The gateway to connect to, it will prompt if not specified")]
  pub(super) gateway: Option<String>,

  #[arg(
    long,
    conflicts_with = "gateway",
    help = "Automatically try gateways in priority order until gateway auth/config succeeds, without prompting"
  )]
  pub(super) auto_gateway: bool,

  #[arg(short, long, help = "The username to use, it will prompt if not specified")]
  pub(super) user: Option<String>,

  #[arg(long, help = "Read the password from standard input")]
  pub(super) passwd_on_stdin: bool,

  #[arg(long, help = "Read the gpauth authentication result from standard input")]
  pub(super) cookie_on_stdin: bool,

  #[arg(
    long,
    help = "Read and write the portal cookie cache, optionally specify the cache file path",
    default_missing_value = "",
    num_args=0..=1
  )]
  pub(super) cookie_cache: Option<String>,

  #[arg(long, short, help = "The VPNC script to use", required_if_eq("script_tun", "true"))]
  pub(super) script: Option<String>,

  #[arg(long, short, help = "The IFNAME for tunnel interface")]
  pub(super) interface: Option<String>,

  #[arg(long, short = 'S', help = "Pass traffic to '--script' program, not tun")]
  pub(super) script_tun: bool,

  #[arg(long, help = "Connect the server as a gateway, instead of a portal")]
  pub(super) as_gateway: bool,

  #[arg(
    long,
    help = "Use HIP (Host Integrity Protection) extension, optionally specify the HIP script path",
    default_missing_value = "",
    num_args=0..=1
  )]
  pub(super) hip: Option<String>,

  #[arg(long, help = "The user used to run the HIP script")]
  pub(super) hip_user: Option<String>,

  #[arg(
    short,
    long,
    help = "Use SSL client certificate file in pkcs#8 (.pem) or pkcs#12 (.p12, .pfx) format"
  )]
  pub(super) certificate: Option<String>,

  #[arg(short = 'k', long, help = "Use SSL private key file in pkcs#8 (.pem) format")]
  pub(super) sslkey: Option<String>,

  #[arg(short = 'p', long, help = "The key passphrase of the private key")]
  pub(super) key_password: Option<String>,

  #[arg(long, hide = true)]
  pub(super) csd_user: Option<String>,

  #[arg(long, hide = true)]
  pub(super) csd_wrapper: Option<String>,

  #[arg(long, default_value = "300", help = "Reconnection retry timeout in seconds")]
  pub(super) reconnect_timeout: u32,

  #[arg(short, long, help = "Request MTU from server (legacy servers only)")]
  pub(super) mtu: Option<u32>,

  #[arg(long, help = "Do not ask for IPv6 connectivity")]
  pub(super) disable_ipv6: bool,

  #[arg(long = "user-agent", hide = true)]
  pub(super) deprecated_user_agent: Option<String>,

  #[arg(long = "os-version", hide = true)]
  pub(super) deprecated_os_version: Option<String>,

  #[arg(long, help = "Override the client version reported to the server, e.g., '6.2.4-49'")]
  pub(super) client_version: Option<String>,

  #[arg(long, value_enum, default_value_t = Os::default())]
  pub(super) os: Os,

  #[arg(long, value_enum, default_value_t = CscMode::Auto, help = "CSC support mode: auto, yes, or no")]
  pub(super) csc: CscMode,

  #[arg(long, help = "Disable DTLS and ESP")]
  pub(super) no_dtls: bool,

  #[arg(
    long = "local-hostname",
    help = "Same as the '--local-hostname' option in the openconnect command"
  )]
  pub(super) local_hostname: Option<String>,

  #[arg(
    long = "force-dpd",
    help = "Same as the '--force-dpd' option in the openconnect command"
  )]
  pub(super) dpd_interval: Option<u32>,

  #[arg(
    long = "no-xmlpost",
    help = "Same as the '--no-xmlpost' option in the openconnect command"
  )]
  pub(super) no_xmlpost: bool,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "The HiDPI mode, useful for high-resolution screens")]
  pub(super) hidpi: bool,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "Do not reuse the remembered authentication cookie")]
  pub(super) clean: bool,

  #[cfg(feature = "webview-auth")]
  #[arg(long, help = "Deprecated: Use the `--browser` option instead")]
  pub(super) default_browser: bool,

  #[arg(
    long,
    help = "Use the specified browser to authenticate, e.g., `default`, `firefox`, `chrome`, `chromium`, `remote`.\nOr the path to the browser executable.\nUse 'remote' for headless servers.",
    default_missing_value = "default",
    num_args=0..=1
  )]
  pub(super) browser: Option<String>,
}

pub(super) fn build_os_profile(args: &ConnectArgs) -> OsProfile {
  build_os_profile_with_host_id(args, None)
}

pub(super) fn build_os_profile_with_host_id(args: &ConnectArgs, host_id: Option<&str>) -> OsProfile {
  let mut builder = OsProfile::builder(ClientOs::from(args.os));
  if let Some(client_version) = args.client_version.as_deref() {
    builder = builder.client_version(client_version.to_string());
  }
  if let Some(local_hostname) = args.local_hostname.as_deref() {
    builder = builder.computer_name_override(local_hostname.to_string());
  }
  if let Some(host_id) = host_id {
    builder = builder.host_id_override(host_id);
  }
  builder.build()
}

pub(super) fn cookie_cache_path(args: &ConnectArgs) -> Option<PathBuf> {
  args.cookie_cache.as_deref().map(|path| {
    let custom = (!path.is_empty()).then_some(path);
    cookie_store::cookie_path(custom)
  })
}

pub(super) fn warn_deprecated_connect_args(args: &ConnectArgs) {
  if args.csd_user.is_some() {
    warn!("Deprecated option --csd-user will be removed; use --hip-user instead");
  }

  if args.csd_wrapper.is_some() {
    warn!("Deprecated option --csd-wrapper will be removed; use --hip instead");
  }

  if args.deprecated_user_agent.is_some() {
    warn!("Deprecated option --user-agent is ignored; user agent is derived from --os");
  }

  if args.deprecated_os_version.is_some() {
    warn!("Deprecated option --os-version is ignored; OS version is derived from --os");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(clap::Parser)]
  struct ConnectArgsTestCli {
    #[command(flatten)]
    args: ConnectArgs,
  }

  #[test]
  fn parses_csc_mode_values() {
    use clap::ValueEnum;

    assert_eq!(CscMode::from_str("auto", false), Ok(CscMode::Auto));
    assert_eq!(CscMode::from_str("yes", false), Ok(CscMode::Yes));
    assert_eq!(CscMode::from_str("no", false), Ok(CscMode::No));
    assert!(CscMode::from_str("maybe", false).is_err());
  }

  #[test]
  fn auto_gateway_flag_parses() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from(["test", "portal.example.com", "--auto-gateway"])
      .expect("--auto-gateway alone should parse");

    assert!(cli.args.auto_gateway);
    assert!(cli.args.gateway.is_none());
  }

  #[test]
  fn auto_gateway_help_describes_auth_config_retry() {
    use clap::CommandFactory;

    let help = ConnectArgsTestCli::command().render_long_help().to_string();

    assert!(help.contains("--auto-gateway"));
    assert!(help.contains("Automatically try gateways in priority order until gateway auth/config succeeds"));
  }

  #[test]
  fn gateway_flag_alone_parses() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from(["test", "portal.example.com", "--gateway", "gw1"])
      .expect("--gateway alone should parse");

    assert!(!cli.args.auto_gateway);
    assert_eq!(cli.args.gateway.as_deref(), Some("gw1"));
  }

  #[test]
  fn os_defaults_to_runtime_os() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from(["test", "portal.example.com"]).expect("connect args should parse");

    assert_eq!(cli.args.os, Os::default());
  }

  #[test]
  fn deprecated_profile_override_flags_still_parse() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from([
      "test",
      "portal.example.com",
      "--user-agent",
      "legacy-agent",
      "--os-version",
      "legacy-os",
      "--client-version",
      "legacy-client",
    ])
    .expect("deprecated profile override flags should remain parse-compatible");

    assert_eq!(cli.args.deprecated_user_agent.as_deref(), Some("legacy-agent"));
    assert_eq!(cli.args.deprecated_os_version.as_deref(), Some("legacy-os"));
    assert_eq!(cli.args.client_version.as_deref(), Some("legacy-client"));
  }

  #[test]
  fn deprecated_profile_override_flags_do_not_affect_os_profile() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from([
      "test",
      "portal.example.com",
      "--os",
      "Linux",
      "--user-agent",
      "legacy-agent",
      "--os-version",
      "legacy-os",
      "--client-version",
      "legacy-client",
    ])
    .expect("deprecated profile override flags should remain parse-compatible");

    let profile = build_os_profile(&cli.args);

    assert_ne!(profile.user_agent(), "legacy-agent");
    assert_ne!(profile.os_version(), "legacy-os");
    assert_eq!(profile.client_version(), "legacy-client");
    assert!(profile.user_agent().contains("legacy-client"));
  }

  #[test]
  fn stdin_host_id_sets_profile_runtime_identity_seed() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from(["test", "portal.example.com", "--os", "Linux"])
      .expect("connect args should parse");

    let profile = build_os_profile_with_host_id(&cli.args, Some("auth-host-id"));

    assert_eq!(profile.host_identity().host_id(), "auth-host-id");
  }

  #[test]
  fn cookie_cache_is_disabled_by_default() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from(["test", "portal.example.com"]).expect("connect args should parse");

    assert!(cli.args.cookie_cache.is_none());
    assert!(cookie_cache_path(&cli.args).is_none());
  }

  #[test]
  fn cookie_cache_accepts_default_path_without_value() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from(["test", "portal.example.com", "--cookie-cache"])
      .expect("--cookie-cache without value should parse");

    assert_eq!(cli.args.cookie_cache.as_deref(), Some(""));
    assert!(cookie_cache_path(&cli.args).is_some());
  }

  #[test]
  fn cookie_cache_accepts_custom_path() {
    use clap::Parser;

    let cli =
      ConnectArgsTestCli::try_parse_from(["test", "portal.example.com", "--cookie-cache", "/tmp/gp-cookie.json"])
        .expect("--cookie-cache with value should parse");

    assert_eq!(
      cookie_cache_path(&cli.args).as_deref(),
      Some(std::path::Path::new("/tmp/gp-cookie.json"))
    );
  }

  #[test]
  fn deprecated_hip_alias_flags_still_parse() {
    use clap::Parser;

    let cli = ConnectArgsTestCli::try_parse_from([
      "test",
      "portal.example.com",
      "--csd-user",
      "legacy-user",
      "--csd-wrapper",
      "/tmp/legacy-hip.sh",
    ])
    .expect("deprecated HIP alias flags should remain parse-compatible");

    assert_eq!(cli.args.csd_user.as_deref(), Some("legacy-user"));
    assert_eq!(cli.args.csd_wrapper.as_deref(), Some("/tmp/legacy-hip.sh"));
  }

  #[test]
  fn auto_gateway_and_gateway_are_mutually_exclusive() {
    use clap::Parser;
    use clap::error::ErrorKind;

    let result =
      ConnectArgsTestCli::try_parse_from(["test", "portal.example.com", "--gateway", "gw1", "--auto-gateway"]);

    let err = match result {
      Ok(_) => panic!("--gateway and --auto-gateway must conflict"),
      Err(err) => err,
    };
    assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
  }
}
