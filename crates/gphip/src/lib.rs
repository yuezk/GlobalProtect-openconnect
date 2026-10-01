use anyhow::{bail, ensure};
use askama::Template;
use gpapi::os_profile::{ClientOs, OsProfile};
use serde::Deserialize;
use std::collections::HashMap;
use std::{io, time::Duration};

use gpapi::process::collection::{CollectionBudget, CollectionControl, CollectorCommands};
use xmltree::Element;

#[cfg(target_os = "macos")]
mod sentinel_macos;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
mod sentinel_unix;

pub const MAX_EDITED_REPORT_BYTES: usize = 48 * 1024;

/// Validate an edited report before it is stored or used as a submission body.
/// The parser receives UTF-8 text and never resolves external resources.
pub fn validate_edited_report(xml: &str) -> anyhow::Result<Element> {
  ensure!(xml.len() <= MAX_EDITED_REPORT_BYTES, "HIP report exceeds 48 KiB");
  let lower = xml.to_ascii_lowercase();
  ensure!(
    !lower.contains("<!doctype") && !lower.contains("<!entity"),
    "HIP report must not contain a DTD or entity declaration"
  );
  if let Some(declaration) = lower.strip_prefix("<?xml").and_then(|rest| rest.split_once("?>")) {
    let declaration = declaration.0;
    if declaration.contains("encoding") {
      ensure!(
        declaration.contains("utf-8") || declaration.contains("utf8"),
        "HIP report must use UTF-8 encoding"
      );
    }
  }

  let root = Element::parse(xml.as_bytes())?;
  ensure!(root.name == "hip-report", "HIP report root must be <hip-report>");
  for name in [
    "md5-sum",
    "user-name",
    "domain",
    "host-name",
    "host-id",
    "ip-address",
    "ipv6-address",
    "generate-time",
    "hip-report-version",
  ] {
    unique_child(&root, name)?;
  }
  let categories = unique_child(&root, "categories")?;
  let mut host_info_entries = categories
    .children
    .iter()
    .filter_map(xmltree::XMLNode::as_element)
    .filter(|entry| entry.name == "entry" && entry.attributes.get("name").is_some_and(|name| name == "host-info"));
  let host_info = host_info_entries
    .next()
    .ok_or_else(|| anyhow::anyhow!("HIP report is missing host-info"))?;
  ensure!(
    host_info_entries.next().is_none(),
    "HIP report has duplicate host-info entries"
  );
  for name in ["client-version", "os", "os-vendor", "domain", "host-name", "host-id"] {
    unique_child(host_info, name)?;
  }
  Ok(root)
}

fn unique_child<'a>(parent: &'a Element, name: &str) -> anyhow::Result<&'a Element> {
  let mut matches = parent
    .children
    .iter()
    .filter_map(xmltree::XMLNode::as_element)
    .filter(|child| child.name == name);
  let child = matches
    .next()
    .ok_or_else(|| anyhow::anyhow!("HIP report is missing <{name}>"))?;
  if matches.next().is_some() {
    bail!("HIP report has duplicate <{name}> elements");
  }
  Ok(child)
}

/// Refresh current identity/session values without running product collectors.
pub fn refresh_edited_report(edited_xml: &str, input: &ReportInput) -> anyhow::Result<String> {
  let budget = CollectionBudget::new(Duration::from_secs(60));
  refresh_edited_report_with_control(edited_xml, input, &budget)
}

pub fn refresh_edited_report_with_control(
  edited_xml: &str,
  input: &ReportInput,
  control: &dyn CollectionControl,
) -> anyhow::Result<String> {
  control.check()?;
  let mut edited = validate_edited_report(edited_xml)?;
  let cookie_params = cookie_params(input);
  let identity = HostInfoCollector::new(&input.profile, input, &cookie_params).collect_identity();
  control.check()?;
  let domain = format!("{}.internal", identity.domain);
  let generate_time = get_current_time_components().0;
  for (name, value) in [
    ("md5-sum", input.context.md5()),
    ("user-name", report_user_name(input, &cookie_params)),
    ("domain", domain.as_str()),
    ("host-name", identity.host_name.as_str()),
    ("host-id", identity.host_id.as_str()),
    ("ip-address", identity.default_ipv4()),
    ("ipv6-address", identity.default_ipv6()),
    ("generate-time", generate_time.as_str()),
  ] {
    set_field(&mut edited, name, value)?;
  }
  let host_info = host_info_mut(&mut edited)?;
  for (name, value) in [
    ("client-version", input.profile.client_version()),
    ("os", identity.os_version.as_str()),
    ("os-vendor", identity.os_vendor.as_str()),
    ("domain", domain.as_str()),
    ("host-name", identity.host_name.as_str()),
    ("host-id", identity.host_id.as_str()),
  ] {
    set_field(host_info, name, value)?;
  }
  let refreshed = write_xml(&edited)?;
  validate_edited_report(&refreshed)?;
  control.check()?;
  Ok(refreshed)
}

fn set_field(target: &mut Element, name: &str, value: &str) -> anyhow::Result<()> {
  let field = target
    .get_mut_child(name)
    .ok_or_else(|| anyhow::anyhow!("HIP report is missing <{name}>"))?;
  field.children = vec![xmltree::XMLNode::Text(value.to_owned())];
  Ok(())
}

#[cfg(test)]
fn host_info(root: &Element) -> anyhow::Result<&Element> {
  unique_child(root, "categories")?
    .children
    .iter()
    .filter_map(xmltree::XMLNode::as_element)
    .find(|entry| entry.name == "entry" && entry.attributes.get("name").is_some_and(|name| name == "host-info"))
    .ok_or_else(|| anyhow::anyhow!("HIP report is missing host-info"))
}

fn host_info_mut(root: &mut Element) -> anyhow::Result<&mut Element> {
  root
    .get_mut_child("categories")
    .ok_or_else(|| anyhow::anyhow!("HIP report is missing <categories>"))?
    .children
    .iter_mut()
    .filter_map(xmltree::XMLNode::as_mut_element)
    .find(|entry| entry.name == "entry" && entry.attributes.get("name").is_some_and(|name| name == "host-info"))
    .ok_or_else(|| anyhow::anyhow!("HIP report is missing host-info"))
}

/// Session values for an actual submission, or an explicit pre-connection preview.
pub enum ReportContext {
  Connected {
    cookie: String,
    client_ip: Option<String>,
    client_ipv6: Option<String>,
    md5: String,
  },
  Preview,
}

impl ReportContext {
  fn cookie(&self) -> &str {
    match self {
      Self::Connected { cookie, .. } => cookie,
      Self::Preview => "",
    }
  }

  fn md5(&self) -> &str {
    match self {
      Self::Connected { md5, .. } => md5,
      Self::Preview => "[available after connection]",
    }
  }

  fn client_ip(&self) -> Option<&str> {
    match self {
      Self::Connected { client_ip, .. } => client_ip.as_deref(),
      Self::Preview => None,
    }
  }

  fn client_ipv6(&self) -> Option<&str> {
    match self {
      Self::Connected { client_ipv6, .. } => client_ipv6.as_deref(),
      Self::Preview => None,
    }
  }
}

/// Inputs collected by the caller for one HIP report.
pub struct ReportInput {
  pub profile: OsProfile,
  pub context: ReportContext,
}

/// Generate the same XML report used by `gpclient hip`.
pub fn generate_report(input: &ReportInput) -> anyhow::Result<String> {
  let budget = CollectionBudget::new(Duration::from_secs(60));
  generate_report_with_control(input, &budget)
}

pub fn generate_report_with_control(input: &ReportInput, control: &dyn CollectionControl) -> anyhow::Result<String> {
  control.check()?;
  let cookie_params = cookie_params(input);
  let (generate_time, day, month, year) = get_current_time_components();
  let host_info = HostInfoCollector::new(&input.profile, input, &cookie_params).collect(control)?;
  let template = HipReportTemplate {
    client_version: input.profile.client_version(),
    generate_time,
    day,
    month,
    year,
    user_name: report_user_name(input, &cookie_params),
    host_info,
    md5: input.context.md5(),
  };
  let report = format_xml(&template.render()?)?;
  control.check()?;
  Ok(report)
}

fn cookie_params(input: &ReportInput) -> HashMap<String, String> {
  serde_urlencoded::from_str(input.context.cookie()).unwrap_or_default()
}

fn report_user_name<'a>(input: &ReportInput, cookie_params: &'a HashMap<String, String>) -> &'a str {
  match input.context {
    ReportContext::Preview => "[available after connection]",
    ReportContext::Connected { .. } => cookie_params.get("user").map(String::as_str).unwrap_or(""),
  }
}

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

#[derive(Debug, Clone)]
struct UfwInfo {
  version: String,
  is_enabled: bool,
}

#[derive(Debug, Clone)]
struct ClamAvInfo {
  version: String,
  definitions_version: Option<String>,
  real_time_protection: bool,
}

struct SentinelInfo {
  version: String,
  real_time_protection: bool,
  firewall_enabled: Option<bool>,
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
  clamav: Option<ClamAvInfo>,
  sentinel: Option<SentinelInfo>,
  ufw: Option<UfwInfo>,
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
  input: &'a ReportInput,
  cookie_params: &'a HashMap<String, String>,
}

impl<'p, 'a> HostInfoCollector<'p, 'a> {
  fn new(profile: &'p OsProfile, input: &'a ReportInput, cookie_params: &'a HashMap<String, String>) -> Self {
    Self {
      profile,
      input,
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
        .with_ipv4(self.input.context.client_ip().map(str::to_string))
        .with_ipv6(self.input.context.client_ipv6().map(str::to_string)),
    }
  }

  /// Single entry point for collecting host info — dispatches per-OS
  /// behavior via `OsProfile` methods rather than `#[cfg(target_os)]`.
  fn collect_identity(&self) -> HostInfo {
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
      defender: None,
      clamav: None,
      sentinel: None,
      ufw: None,
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

  fn collect(&self, control: &dyn CollectionControl) -> io::Result<HostInfo> {
    control.check()?;
    let mut info = self.collect_identity();
    control.check()?;
    let commands = CollectorCommands::new(control);
    if self.profile.client_os() == ClientOs::Linux && self.profile.is_native() {
      info.defender = detect_microsoft_defender(&commands)?;
      info.clamav = detect_clamav(&commands)?;
      info.ufw = detect_ufw(&commands)?;
    }
    #[cfg(target_os = "macos")]
    if self.profile.client_os() == ClientOs::Mac && self.profile.is_native() {
      info.sentinel = sentinel_macos::detect(&commands)?;
    }
    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
    if self.profile.client_os() == ClientOs::Linux && self.profile.is_native() {
      info.sentinel = sentinel_unix::detect(&commands)?;
    }
    control.check()?;
    Ok(info)
  }
}

fn detect_clamav(commands: &CollectorCommands<'_>) -> io::Result<Option<ClamAvInfo>> {
  let Some(version_str) = command_text(commands, "clamscan", &["--version"])? else {
    return Ok(None);
  };
  let Some((version, definitions_version)) = parse_clamav_version(&version_str) else {
    return Ok(None);
  };
  let real_time_protection = commands
    .run("systemctl", &["is-active", "clamav-onaccess.service"])?
    .is_some_and(|output| output.status.success());
  Ok(Some(ClamAvInfo {
    version,
    definitions_version,
    real_time_protection,
  }))
}

fn detect_ufw(commands: &CollectorCommands<'_>) -> io::Result<Option<UfwInfo>> {
  let Some(version_str) = command_text(commands, "ufw", &["version"])? else {
    return Ok(None);
  };
  let Some(version) = parse_ufw_version(&version_str) else {
    return Ok(None);
  };
  let Some(ufw) = commands.resolve("ufw") else {
    return Ok(None);
  };
  let output = if uzers::get_effective_uid() == 0 {
    commands.run_path(&ufw, &["status"])?
  } else {
    commands.run("sudo", &["-n", &ufw.to_string_lossy(), "status"])?
  };
  let Some(output) = output.filter(|output| output.status.success()) else {
    return Ok(None);
  };
  let Some(is_enabled) = String::from_utf8(output.stdout)
    .ok()
    .as_deref()
    .and_then(parse_ufw_status)
  else {
    return Ok(None);
  };
  Ok(Some(UfwInfo { version, is_enabled }))
}

fn command_text(commands: &CollectorCommands<'_>, name: &str, args: &[&str]) -> io::Result<Option<String>> {
  Ok(
    commands
      .run(name, args)?
      .filter(|output| output.status.success())
      .and_then(|output| String::from_utf8(output.stdout).ok()),
  )
}

fn parse_clamav_version(output: &str) -> Option<(String, Option<String>)> {
  let mut output_parts = output.split_whitespace();
  if output_parts.next()? != "ClamAV" {
    return None;
  }

  let mut version_parts = output_parts.next()?.split('/');
  let version = version_parts.next()?;
  if version.is_empty() {
    return None;
  }

  let definitions_version = version_parts
    .next()
    .filter(|definitions_version| !definitions_version.is_empty())
    .map(str::to_string);

  Some((version.to_string(), definitions_version))
}

fn parse_ufw_version(output: &str) -> Option<String> {
  let mut output_parts = output.split_whitespace();
  if output_parts.next()? != "ufw" {
    return None;
  }

  output_parts.next().map(str::to_string)
}

fn parse_ufw_status(output: &str) -> Option<bool> {
  let status = output
    .lines()
    .find_map(|line| line.trim().strip_prefix("Status:"))?
    .trim();

  match status {
    "active" => Some(true),
    "inactive" => Some(false),
    _ => None,
  }
}

fn detect_microsoft_defender(commands: &CollectorCommands<'_>) -> io::Result<Option<DefenderInfo>> {
  let Some(json) = command_text(commands, "mdatp", &["health", "--output", "json"])? else {
    return Ok(None);
  };
  Ok(parse_defender_info(&json))
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

  write_xml(&xml)
}

fn write_xml(xml: &Element) -> anyhow::Result<String> {
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
  const CURRENT_TOP_LEVEL_FIELDS: &[&str] = &[
    "md5-sum",
    "user-name",
    "domain",
    "host-name",
    "host-id",
    "ip-address",
    "ipv6-address",
    "generate-time",
  ];
  const CURRENT_HOST_INFO_FIELDS: &[&str] = &["client-version", "os", "os-vendor", "domain", "host-name", "host-id"];
  use super::*;
  use gpapi::os_profile::OsProfileBuilder;

  fn make_input(profile: OsProfile) -> ReportInput {
    ReportInput {
      profile,
      context: ReportContext::Connected {
        cookie: String::new(),
        client_ip: None,
        client_ipv6: None,
        md5: "deadbeef".to_string(),
      },
    }
  }

  fn make_profile(client_os: ClientOs) -> OsProfile {
    OsProfileBuilder::new(client_os).build()
  }

  #[test]
  fn host_info_os_vendor_is_linux_for_linux_profile() {
    let input = make_input(make_profile(ClientOs::Linux));
    let profile = make_profile(ClientOs::Linux);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &input, &cookie_params).collect_identity();

    assert_eq!(info.os_vendor, "Linux");
  }

  #[test]
  fn host_info_os_vendor_is_apple_for_mac_profile() {
    let input = make_input(make_profile(ClientOs::Mac));
    let profile = make_profile(ClientOs::Mac);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &input, &cookie_params).collect_identity();

    assert_eq!(info.os_vendor, "Apple");
  }

  #[test]
  fn host_info_os_vendor_is_microsoft_for_windows_profile() {
    let input = make_input(make_profile(ClientOs::Windows));
    let profile = make_profile(ClientOs::Windows);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &input, &cookie_params).collect_identity();

    assert_eq!(info.os_vendor, "Microsoft");
  }

  #[test]
  fn host_info_host_id_comes_from_os_profile() {
    let input = make_input(make_profile(ClientOs::Linux));
    let profile = make_profile(ClientOs::Linux);
    let cookie_params: HashMap<String, String> = HashMap::new();

    let info = HostInfoCollector::new(&profile, &input, &cookie_params).collect_identity();

    assert_eq!(info.host_id, profile.host_id());
  }

  #[test]
  fn host_info_host_id_independent_of_runtime_for_each_os() {
    // host_id should reflect the OsProfile's machine identity for every
    // simulated OS, regardless of the runtime platform.
    for client_os in [ClientOs::Linux, ClientOs::Mac, ClientOs::Windows] {
      let input = make_input(make_profile(client_os));
      let profile = make_profile(client_os);
      let cookie_params: HashMap<String, String> = HashMap::new();

      let info = HostInfoCollector::new(&profile, &input, &cookie_params).collect_identity();

      assert_eq!(info.host_id, profile.host_id());
    }
  }

  #[test]
  fn generated_report_contains_supplied_session_fields() {
    let mut input = make_input(make_profile(ClientOs::Linux));
    input.context = ReportContext::Connected {
      cookie: "user=test-user&domain=test-domain".to_string(),
      client_ip: None,
      client_ipv6: None,
      md5: "test-digest".to_string(),
    };
    let report = generate_report(&input).unwrap();
    assert!(report.contains("test-user"));
    assert!(report.contains("test-digest"));
    validate_edited_report(&report).unwrap();
  }

  #[test]
  fn mac_report_renders_detected_sentinel_products() {
    let input = make_input(make_profile(ClientOs::Mac));
    let cookie_params = HashMap::new();
    let mut host_info = HostInfoCollector::new(&input.profile, &input, &cookie_params).collect_identity();
    host_info.sentinel = Some(SentinelInfo {
      version: "25.3.4.8365".to_string(),
      real_time_protection: true,
      firewall_enabled: Some(false),
    });
    let (generate_time, day, month, year) = get_current_time_components();
    let report = HipReportTemplate {
      client_version: input.profile.client_version(),
      generate_time,
      day,
      month,
      year,
      user_name: "test-user",
      host_info,
      md5: "test-digest",
    }
    .render()
    .unwrap();
    let report = format_xml(&report).unwrap();
    validate_edited_report(&report).unwrap();
    assert!(report.contains("vendor=\"SentinelOne\" name=\"Sentinel Agent\" version=\"25.3.4.8365\""));
    assert!(report.contains("<real-time-protection>yes</real-time-protection>"));
    assert!(report.contains("<is-enabled>no</is-enabled>"));
    assert_eq!(report.matches("name=\"Sentinel Agent\"").count(), 2);
  }

  #[test]
  fn linux_report_renders_detected_sentinel_antimalware() {
    let input = make_input(make_profile(ClientOs::Linux));
    let cookie_params = HashMap::new();
    let mut host_info = HostInfoCollector::new(&input.profile, &input, &cookie_params).collect_identity();
    host_info.sentinel = Some(SentinelInfo {
      version: "25.2.2.14".to_string(),
      real_time_protection: true,
      firewall_enabled: None,
    });
    let (generate_time, day, month, year) = get_current_time_components();
    let report = HipReportTemplate {
      client_version: input.profile.client_version(),
      generate_time,
      day,
      month,
      year,
      user_name: "test-user",
      host_info,
      md5: "test-digest",
    }
    .render()
    .unwrap();
    let report = format_xml(&report).unwrap();
    validate_edited_report(&report).unwrap();
    assert!(report.contains("vendor=\"SentinelOne\" name=\"Sentinel Agent\" version=\"25.2.2.14\""));
    assert!(report.contains("osType=\"1\""));
    assert!(report.contains("<real-time-protection>yes</real-time-protection>"));
    assert_eq!(report.matches("name=\"Sentinel Agent\"").count(), 1);
  }

  #[test]
  fn preview_uses_explicit_pending_digest() {
    let input = ReportInput {
      profile: make_profile(ClientOs::Mac),
      context: ReportContext::Preview,
    };
    let report = generate_report(&input).unwrap();
    assert!(report.contains("[available after connection]"));
    validate_edited_report(&report).unwrap();
  }

  #[test]
  fn rejects_malformed_and_unsafe_edited_reports() {
    assert!(validate_edited_report("<other/>").is_err());
    assert!(validate_edited_report("<!DOCTYPE hip-report [<!ENTITY x 'value'>]><hip-report/>").is_err());
    assert!(validate_edited_report(&"x".repeat(MAX_EDITED_REPORT_BYTES + 1)).is_err());
    let valid = generate_report(&make_input(make_profile(ClientOs::Mac))).unwrap();
    assert!(validate_edited_report(&valid.replace("<md5-sum>", "<wrong-field>")).is_err());
  }

  #[test]
  fn refreshes_exact_current_fields_and_preserves_user_categories() {
    let mut input = make_input(make_profile(ClientOs::Mac));
    input.context = ReportContext::Connected {
      cookie: "user=current-user&domain=current-domain".to_string(),
      client_ip: Some("192.0.2.4".to_string()),
      client_ipv6: None,
      md5: "current-digest".to_string(),
    };
    let current = validate_edited_report(&generate_report(&input).unwrap()).unwrap();
    let mut saved = current.clone();
    for field in CURRENT_TOP_LEVEL_FIELDS {
      saved.get_mut_child(*field).unwrap().children = vec![xmltree::XMLNode::Text("stale".into())];
    }
    let saved_host = host_info_mut(&mut saved).unwrap();
    for field in CURRENT_HOST_INFO_FIELDS {
      saved_host.get_mut_child(*field).unwrap().children = vec![xmltree::XMLNode::Text("stale".into())];
    }
    let nested = Element::parse(
      r#"<network-interface><entry name="manual"><ip-address><entry name="203.0.113.17"/></ip-address></entry></network-interface>"#.as_bytes(),
    )
    .unwrap();
    saved_host.get_mut_child("network-interface").unwrap().children = nested.children;
    let custom_category = Element::parse(
      r#"<entry name="custom-category"><list><entry><ProductInfo><Prod name="Sentinel"/></ProductInfo></entry></list></entry>"#.as_bytes(),
    )
    .unwrap();
    saved
      .get_mut_child("categories")
      .unwrap()
      .children
      .push(xmltree::XMLNode::Element(custom_category));

    let earliest = chrono::Local::now().naive_local().and_utc().timestamp();
    let refreshed_xml = refresh_edited_report(&write_xml(&saved).unwrap(), &input).unwrap();
    let latest = chrono::Local::now().naive_local().and_utc().timestamp();
    let refreshed = validate_edited_report(&refreshed_xml).unwrap();
    let timestamp = chrono::NaiveDateTime::parse_from_str(
      &unique_child(&refreshed, "generate-time").unwrap().get_text().unwrap(),
      "%m/%d/%Y %H:%M:%S",
    )
    .unwrap()
    .and_utc()
    .timestamp();
    assert!((earliest..=latest).contains(&timestamp));
    for field in CURRENT_TOP_LEVEL_FIELDS
      .iter()
      .filter(|field| **field != "generate-time")
    {
      assert_eq!(
        unique_child(&refreshed, field).unwrap().get_text(),
        unique_child(&current, field).unwrap().get_text()
      );
    }
    let refreshed_host = host_info(&refreshed).unwrap();
    let current_host = host_info(&current).unwrap();
    for field in CURRENT_HOST_INFO_FIELDS {
      assert_eq!(
        unique_child(refreshed_host, field).unwrap().get_text(),
        unique_child(current_host, field).unwrap().get_text()
      );
    }
    assert!(refreshed_xml.contains("203.0.113.17"));
    assert!(refreshed_xml.contains("name=\"Sentinel\""));
    assert!(refreshed_xml.contains("name=\"custom-category\""));
    assert!(!refreshed_xml.contains(">stale<"));
  }

  #[test]
  fn refresh_rejects_missing_required_host_field() {
    let input = make_input(make_profile(ClientOs::Mac));
    let mut saved = validate_edited_report(&generate_report(&input).unwrap()).unwrap();
    let host = host_info_mut(&mut saved).unwrap();
    host
      .children
      .retain(|node| !matches!(node, xmltree::XMLNode::Element(element) if element.name == "os"));
    assert!(refresh_edited_report(&write_xml(&saved).unwrap(), &input).is_err());
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

  #[test]
  fn parses_clamav_version() {
    let version =
      parse_clamav_version("ClamAV 1.4.3/27562/Sun Aug 31 10:23:42 2026").expect("ClamAV version should parse");

    assert_eq!(version, ("1.4.3".to_string(), Some("27562".to_string())));
  }

  #[test]
  fn parses_clamav_version_without_definitions_version() {
    assert_eq!(parse_clamav_version("ClamAV 1.4.3"), Some(("1.4.3".to_string(), None)));
  }

  #[test]
  fn parses_ufw_version() {
    assert_eq!(parse_ufw_version("ufw 0.36.2\n"), Some("0.36.2".to_string()));
    assert_eq!(parse_ufw_version("unexpected output\n"), None);
  }

  #[test]
  fn parses_ufw_status() {
    assert_eq!(parse_ufw_status("Status: active\n"), Some(true));
    assert_eq!(parse_ufw_status("Status: inactive\n"), Some(false));
    assert_eq!(parse_ufw_status("Status: unknown\n"), None);
  }
}
