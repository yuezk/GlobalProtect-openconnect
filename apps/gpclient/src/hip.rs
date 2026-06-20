use askama::Template;
use clap::Args;
use gpapi::{
  clap::args::Os,
  os_profile::{ClientOs, OsProfile, OsProfileBuilder},
};
use log::debug;
use serde::Deserialize;
use std::collections::HashMap;
use std::process::Command;
use xmltree::Element;

#[derive(Template)]
#[template(path = "hip_report.xml")]
struct HipReportTemplate<'a> {
  client_version: &'a str,
  generate_time: String,
  day: String,
  month: String,
  year: String,
  user_name: &'a str,
  host_info: HostInfo,
  md5: &'a str,
}

#[derive(Debug, Clone, Deserialize)]
struct DefenderInfo {
  #[serde(rename = "appVersion")]
  app_version: String,
  #[serde(rename = "engineVersion")]
  engine_version: String,
  #[serde(rename = "definitionsVersion")]
  definitions_version: String,
  #[serde(rename = "realTimeProtectionEnabled")]
  real_time_protection_enabled: RealTimeProtection,
}

#[derive(Debug, Clone, Deserialize)]
struct RealTimeProtection {
  value: bool,
}

/// Host information for HIP reporting
struct HostInfo {
  /// Common for all OSes, e.g., "Apple", "Microsoft", "Linux"
  os_vendor: String,
  /// Common for all OSes, e.g., "Linux Ubuntu 20.04", "Apple Mac OS X 10.15.7", etc.
  os_version: String,
  /// Per-OS machine identifier (UUID for Linux/Windows, MAC for macOS)
  host_id: String,
  /// Common for all OSes
  host_name: String,
  /// Only for macOS and Windows, e.g., "10.15.7", "10.0.19044.2130"
  software_version: String,
  domain: String,
  network_interfaces: Vec<NetworkInterface>,
  defender: Option<DefenderInfo>,
}

impl HostInfo {
  /// Get the first available IPv4 address from network interfaces
  pub fn default_ipv4(&self) -> &str {
    self
      .network_interfaces
      .first()
      .and_then(|iface| iface.ipv4.as_deref())
      .unwrap_or_default()
  }

  /// Get the first available IPv6 address from network interfaces
  pub fn default_ipv6(&self) -> &str {
    self
      .network_interfaces
      .first()
      .and_then(|iface| iface.ipv6.as_deref())
      .unwrap_or_default()
  }
}

/// Network interface information
#[derive(Clone)]
struct NetworkInterface {
  name: String,
  description: String,
  mac_address: Option<String>,
  ipv4: Option<String>,
  ipv6: Option<String>,
}

impl NetworkInterface {
  /// Create a new network interface with basic information
  fn new(name: String, description: String) -> Self {
    Self {
      name,
      description,
      mac_address: None,
      ipv4: None,
      ipv6: None,
    }
  }

  /// Set MAC address
  fn with_mac(mut self, mac: Option<String>) -> Self {
    self.mac_address = mac;
    self
  }

  /// Set IPv4 address
  fn with_ipv4(mut self, ipv4: Option<String>) -> Self {
    self.ipv4 = ipv4;
    self
  }

  /// Set IPv6 address
  fn with_ipv6(mut self, ipv6: Option<String>) -> Self {
    self.ipv6 = ipv6;
    self
  }
}

#[derive(Args)]
pub(crate) struct HipArgs {
  #[arg(long, help = "The GP client version, e.g., 6.2.4-49")]
  client_version: String,

  #[arg(long, value_enum, help = "The client OS")]
  client_os: Os,

  #[arg(long, hide = true)]
  os_version: Option<String>,

  #[arg(long, help = "Use this runtime host ID when building the OS profile")]
  host_id: Option<String>,

  #[arg(long, help = "The authentication cookie")]
  cookie: String,

  #[arg(long, help = "The client IPv4 address")]
  client_ip: Option<String>,

  #[arg(long, help = "The client IPv6 address")]
  client_ipv6: Option<String>,

  #[arg(long, help = "The MD5 digest to encode into the HIP report")]
  md5: String,
}

pub(crate) struct HipHandler<'a> {
  args: &'a HipArgs,
  profile: OsProfile,
}

impl<'a> HipHandler<'a> {
  pub(crate) fn new(args: &'a HipArgs) -> Self {
    let profile = build_os_profile(args);
    Self { args, profile }
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    let cookie_params = self.parse_cookie();
    let report = self.generate_hip_report(&cookie_params)?;

    debug!("Generated HIP report:\n{}", report);
    println!("{}", report);

    Ok(())
  }

  fn parse_cookie(&self) -> HashMap<String, String> {
    // Parse URL-encoded cookie string using serde_urlencoded
    serde_urlencoded::from_str(&self.args.cookie).unwrap_or_default()
  }

  /// Generate the complete HIP report XML
  fn generate_hip_report(&self, cookie_params: &HashMap<String, String>) -> anyhow::Result<String> {
    let (generate_time, day, month, year) = get_current_time_components();
    let user_name = cookie_params.get("user").map(|s| s.as_str()).unwrap_or("");
    let host_info = self.collect_host_info(cookie_params);

    let template = HipReportTemplate {
      client_version: &self.args.client_version,
      generate_time,
      day,
      month,
      year,
      user_name,
      host_info,
      md5: &self.args.md5,
    };

    let report = template.render()?;
    format_xml(&report)
  }

  /// Collect host information using the configured OsProfile.
  fn collect_host_info(&self, cookie_params: &'a HashMap<String, String>) -> HostInfo {
    HostInfoCollector::new(&self.profile, self.args, cookie_params).collect()
  }
}

/// Construct an `OsProfile` from CLI args, falling back to per-OS defaults
/// for any value not supplied on the command line.
fn build_os_profile(args: &HipArgs) -> OsProfile {
  let client_os = ClientOs::from(args.client_os);
  let mut builder = OsProfileBuilder::new(client_os).client_version(args.client_version.clone());
  if let Some(host_id) = args.host_id.as_deref() {
    debug!("HIP host-id override supplied: {}", host_id);
    builder = builder.host_id_override(host_id);
  }
  let profile = builder.build();
  debug!(
    "HIP OS profile host-id: runtime={}, projected={}, client_os={}",
    profile.host_identity().host_id(),
    profile.host_id(),
    client_os.as_str()
  );
  profile
}

// ============================================================================
// Host Information Collector
// ============================================================================

/// Helper struct for collecting host information.
///
/// Identity values (vendor, host_id, computer name, software version) come
/// from the configured `OsProfile`, while runtime state (network interfaces,
/// IP addresses) is enumerated here from the underlying machine.
struct HostInfoCollector<'p, 'a> {
  profile: &'p OsProfile,
  args: &'a HipArgs,
  cookie_params: &'a HashMap<String, String>,
}

impl<'p, 'a> HostInfoCollector<'p, 'a> {
  fn new(profile: &'p OsProfile, args: &'a HipArgs, cookie_params: &'a HashMap<String, String>) -> Self {
    Self {
      profile,
      args,
      cookie_params,
    }
  }

  /// Domain belongs to the runtime user/session, not the simulated OS, so
  /// it is sourced from the auth cookie when available.
  fn get_domain(&self) -> String {
    self
      .cookie_params
      .get("domain")
      .map(|s| s.to_string())
      .unwrap_or_default()
  }

  /// Collect network interface information with fallback
  fn collect_network_interface(&self) -> NetworkInterface {
    match netdev::get_default_interface() {
      Ok(iface) => NetworkInterface::new(
        iface.name.clone(),
        iface.description.unwrap_or_else(|| iface.name.clone()),
      )
      .with_mac(iface.mac_addr.map(|mac| mac.address()))
      .with_ipv4(iface.ipv4.first().map(|ip| ip.addr().to_string()))
      .with_ipv6(iface.ipv6.first().map(|ip| ip.addr().to_string())),

      Err(_) => NetworkInterface::new("unknown".to_string(), "unknown".to_string())
        .with_ipv4(self.args.client_ip.clone())
        .with_ipv6(self.args.client_ipv6.clone()),
    }
  }

  /// Single entry point for collecting host info — dispatches per-OS
  /// behavior via `OsProfile` methods rather than `#[cfg(target_os)]`.
  fn collect(&self) -> HostInfo {
    let runtime_iface = self.collect_network_interface();
    let primary = self.adapt_primary_interface(&runtime_iface);

    let mut interfaces = vec![primary.clone()];
    interfaces.extend(self.extra_interfaces(&primary));

    HostInfo {
      os_vendor: self.profile.os_vendor().to_string(),
      os_version: self.profile.os_version().to_string(),
      host_id: self.profile.host_id().to_string(),
      host_name: self.profile.computer().to_string(),
      software_version: self.profile.software_version().to_string(),
      domain: self.domain_for_profile(),
      network_interfaces: interfaces,
      defender: self.defender_for_profile(),
    }
  }

  /// Build the primary network interface with OS-appropriate naming and
  /// MAC formatting. When emulating a different OS, the runtime interface
  /// name/description are replaced with the canonical placeholder for the
  /// target OS.
  fn adapt_primary_interface(&self, runtime: &NetworkInterface) -> NetworkInterface {
    let (name, description) = if self.profile.is_native() {
      (runtime.name.clone(), runtime.description.clone())
    } else {
      placeholder_interface_for(self.profile, runtime)
    };

    NetworkInterface {
      name,
      description,
      mac_address: format_mac_for(self.profile, runtime.mac_address.clone()),
      ipv4: runtime.ipv4.clone(),
      ipv6: runtime.ipv6.clone(),
    }
  }

  /// Additional interfaces appended after the primary. Only Windows
  /// emits the software loopback interface in HIP reports.
  fn extra_interfaces(&self, primary: &NetworkInterface) -> Vec<NetworkInterface> {
    match self.profile.client_os() {
      ClientOs::Windows => vec![NetworkInterface {
        name: derive_windows_network_name(self.profile.host_id(), primary),
        description: "Software Loopback Interface 1".to_string(),
        mac_address: Some(String::new()),
        ipv4: Some("127.0.0.1".to_string()),
        ipv6: Some("::1".to_string()),
      }],
      ClientOs::Linux | ClientOs::Mac => vec![],
    }
  }

  /// Linux HIP reports omit the domain field; macOS/Windows include it
  /// from the auth cookie.
  fn domain_for_profile(&self) -> String {
    match self.profile.client_os() {
      ClientOs::Linux => String::new(),
      ClientOs::Mac | ClientOs::Windows => self.get_domain(),
    }
  }

  fn defender_for_profile(&self) -> Option<DefenderInfo> {
    match self.profile.client_os() {
      ClientOs::Linux => detect_microsoft_defender_blocking(),
      ClientOs::Mac | ClientOs::Windows => None,
    }
  }
}

fn detect_microsoft_defender_blocking() -> Option<DefenderInfo> {
  let output = Command::new("mdatp")
    .arg("health")
    .arg("--output")
    .arg("json")
    .output()
    .ok()?;

  if !output.status.success() {
    debug!("mdatp health command failed");
    return None;
  }

  let json = String::from_utf8(output.stdout).ok()?;
  let defender = parse_defender_info(&json)?;

  debug!("Detected Microsoft Defender: {:?}", defender);
  Some(defender)
}

fn parse_defender_info(json: &str) -> Option<DefenderInfo> {
  serde_json::from_str(json).ok()
}

// ============================================================================
// Per-OS interface helpers (selected via OsProfile, not cfg)
// ============================================================================

/// Canonical interface name/description for the target OS when the runtime
/// is a different OS (i.e. emulation).
fn placeholder_interface_for(profile: &OsProfile, runtime: &NetworkInterface) -> (String, String) {
  match profile.client_os() {
    ClientOs::Linux => ("enp1s0f0".to_string(), "enp1s0f0".to_string()),
    ClientOs::Mac => ("en0".to_string(), "en0".to_string()),
    ClientOs::Windows => (
      derive_windows_network_name(profile.host_id(), runtime),
      "PANGP Virtual Ethernet Adapter Secure".to_string(),
    ),
  }
}

/// MAC address formatting per target OS. Windows uses hyphen separators;
/// Linux and macOS preserve the colon-separated form returned by the
/// platform.
fn format_mac_for(profile: &OsProfile, mac: Option<String>) -> Option<String> {
  match profile.client_os() {
    ClientOs::Windows => mac.map(|m| m.replace(':', "-")),
    ClientOs::Linux | ClientOs::Mac => mac,
  }
}

// ============================================================================
// Utility Functions
// ============================================================================

/// Get current time components for HIP report
fn get_current_time_components() -> (String, String, String, String) {
  let now = chrono::Local::now();
  (
    now.format("%m/%d/%Y %H:%M:%S").to_string(),
    now.format("%d").to_string(),
    now.format("%m").to_string(),
    now.format("%Y").to_string(),
  )
}

/// Format XML string with proper indentation
fn format_xml(xml_str: &str) -> anyhow::Result<String> {
  let xml = Element::parse(xml_str.as_bytes())?;

  let config = xmltree::EmitterConfig::new().perform_indent(true);
  let mut xml_buf = Vec::new();
  xml.write_with_config(&mut xml_buf, config)?;

  Ok(String::from_utf8(xml_buf)?)
}

fn derive_windows_network_name(host_id: &str, iface: &NetworkInterface) -> String {
  let seed = format!(
    "{}-{}-{}",
    host_id,
    iface.mac_address.as_deref().unwrap_or("00:00:00:00:00:00"),
    iface.name
  );

  format!(
    "{{{}}}",
    uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, seed.as_bytes())
      .to_string()
      .to_uppercase()
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use gpapi::os_profile::runtime_client_os;

  fn make_hip_args(client_os: Os) -> HipArgs {
    HipArgs {
      client_version: "6.2.4-49".to_string(),
      client_os,
      os_version: Some("test-os-version".to_string()),
      host_id: None,
      cookie: String::new(),
      client_ip: None,
      client_ipv6: None,
      md5: "deadbeef".to_string(),
    }
  }

  fn make_profile(client_os: ClientOs) -> OsProfile {
    OsProfileBuilder::new(client_os).build()
  }

  #[test]
  fn host_info_os_vendor_is_linux_for_linux_profile() {
    let args = make_hip_args(Os::Linux);
    let profile = make_profile(ClientOs::Linux);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

    assert_eq!(info.os_vendor, "Linux");
  }

  #[test]
  fn host_info_os_vendor_is_apple_for_mac_profile() {
    let args = make_hip_args(Os::Mac);
    let profile = make_profile(ClientOs::Mac);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

    assert_eq!(info.os_vendor, "Apple");
  }

  #[test]
  fn host_info_os_vendor_is_microsoft_for_windows_profile() {
    let args = make_hip_args(Os::Windows);
    let profile = make_profile(ClientOs::Windows);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

    assert_eq!(info.os_vendor, "Microsoft");
  }

  #[test]
  fn host_info_host_id_comes_from_os_profile() {
    let args = make_hip_args(Os::Linux);
    let profile = make_profile(ClientOs::Linux);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

    assert_eq!(info.host_id, profile.host_id());
  }

  #[test]
  fn host_info_host_id_independent_of_runtime_for_each_os() {
    // host_id should reflect the OsProfile's machine identity for every
    // simulated OS, regardless of the runtime platform.
    for (client_os, os_arg) in [
      (ClientOs::Linux, Os::Linux),
      (ClientOs::Mac, Os::Mac),
      (ClientOs::Windows, Os::Windows),
    ] {
      let args = make_hip_args(os_arg);
      let profile = make_profile(client_os);
      let cookie_params: HashMap<String, String> = HashMap::new();

      let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

      assert_eq!(info.host_id, profile.host_id());
    }
  }

  #[test]
  fn hip_os_version_argument_is_ignored() {
    let args = make_hip_args(Os::Linux);
    let profile = build_os_profile(&args);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

    assert_eq!(info.os_version, profile.os_version());
    assert_ne!(info.os_version, "test-os-version");
  }

  #[test]
  fn host_id_argument_sets_profile_runtime_identity() {
    let mut args = make_hip_args(Os::default());
    args.host_id = Some("auth-host-id".to_string());

    let profile = build_os_profile(&args);

    assert_eq!(profile.client_os(), runtime_client_os());
    assert_eq!(profile.host_identity().host_id(), "auth-host-id");
  }

  #[test]
  fn host_info_uses_host_id_argument_for_native_profile() {
    let mut args = make_hip_args(Os::default());
    args.host_id = Some("auth-host-id".to_string());
    let profile = build_os_profile(&args);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &args, &cookie_params).collect();

    assert_eq!(profile.host_identity().host_id(), "auth-host-id");
    assert_eq!(info.host_id, profile.host_id());
  }

  #[test]
  fn parses_microsoft_defender_health_json() {
    let defender = parse_defender_info(
      r#"{
        "appVersion": "101.25042.0000",
        "engineVersion": "1.1.25040.2",
        "definitionsVersion": "1.429.201.0",
        "realTimeProtectionEnabled": { "value": true }
      }"#,
    )
    .expect("defender health json should parse");

    assert_eq!(defender.app_version, "101.25042.0000");
    assert_eq!(defender.engine_version, "1.1.25040.2");
    assert_eq!(defender.definitions_version, "1.429.201.0");
    assert!(defender.real_time_protection_enabled.value);
  }

  #[test]
  fn rejects_invalid_microsoft_defender_health_json() {
    assert!(parse_defender_info("{}").is_none());
  }
}
