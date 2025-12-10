use askama::Template;
use clap::Args;
use gpapi::utils::host_utils;
use log::debug;
use std::collections::HashMap;
use xmltree::Element;

#[derive(Template)]
#[template(path = "hip_report.xml")]
struct HipReportTemplate<'a> {
  client_version: &'a str,
  generate_time: String,
  day: String,
  month: String,
  year: String,
  md5: &'a str,
  user_name: &'a str,
  host_info: HostInfo,
}

/// Host information for HIP reporting
struct HostInfo {
  /// Only for macOS and Windows
  host_id: String,
  /// Common for all OSes
  host_name: String,
  os: &'static str,
  /// Only for macOS and Windows
  os_version: &'static str,
  os_vendor: &'static str,
  domain: String,
  network_interfaces: Vec<NetworkInterface>,
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
  #[arg(long, help = "The authentication cookie")]
  cookie: String,

  #[arg(long, help = "The client IPv4 address")]
  client_ip: Option<String>,

  #[arg(long, help = "The client IPv6 address")]
  client_ipv6: Option<String>,

  #[arg(long, help = "The MD5 digest to encode into the HIP report")]
  md5: String,

  #[arg(long, default_value = "Windows", help = "The client OS (Linux, Mac, or Windows)")]
  client_os: String,

  #[arg(long, default_value = "", help = "The client software version")]
  client_version: String,
}

pub(crate) struct HipHandler<'a> {
  args: &'a HipArgs,
}

impl<'a> HipHandler<'a> {
  pub(crate) fn new(args: &'a HipArgs) -> Self {
    Self { args }
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    self.validate_args()?;
    let cookie_params = self.parse_cookie();
    let report = self.generate_hip_report(&cookie_params)?;

    debug!("Generated HIP report:\n{}", report);
    println!("{}", report);

    Ok(())
  }

  /// Validate that at least one IP address is provided
  fn validate_args(&self) -> anyhow::Result<()> {
    if self.args.client_ip.is_none() && self.args.client_ipv6.is_none() {
      anyhow::bail!("At least one of --client-ip or --client-ipv6 must be provided");
    }
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
    let client_version = std::env::var("APP_VERSION")
      .map(|v| v.leak() as &'a str)
      .unwrap_or(&self.args.client_version);

    let template = HipReportTemplate {
      client_version,
      generate_time,
      day,
      month,
      year,
      md5: &self.args.md5,
      user_name,
      host_info,
    };

    let report = template.render()?;
    format_xml(&report)
  }

  /// Collect host information based on the client OS
  fn collect_host_info(&self, cookie_params: &HashMap<String, String>) -> HostInfo {
    let collector = HostInfoCollector::new(self.args, cookie_params);

    match self.args.client_os.as_str() {
      "Linux" => collector.collect_linux(),
      "Mac" => collector.collect_macos(),
      _ => collector.collect_windows(),
    }
  }
}

// ============================================================================
// Host Information Collector
// ============================================================================

/// Helper struct for collecting host information
struct HostInfoCollector<'a> {
  args: &'a HipArgs,
  cookie_params: &'a HashMap<String, String>,
}

impl<'a> HostInfoCollector<'a> {
  fn new(args: &'a HipArgs, cookie_params: &'a HashMap<String, String>) -> Self {
    Self { args, cookie_params }
  }

  /// Get domain from cookie parameters
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

  /// Collect Linux-specific host information
  fn collect_linux(&self) -> HostInfo {
    let iface = self.collect_network_interface();

    // Use the fixed interface name enp1s0f0 for Linux if it is not on Linux
    let iface = NetworkInterface {
      #[cfg(target_os = "linux")]
      name: iface.name,
      #[cfg(target_os = "linux")]
      description: iface.description,

      #[cfg(not(target_os = "linux"))]
      name: "enp1s0f0".to_string(),
      #[cfg(not(target_os = "linux"))]
      description: "enp1s0f0".to_string(),

      mac_address: iface.mac_address,
      ipv4: iface.ipv4,
      ipv6: iface.ipv6,
    };

    HostInfo {
      os_vendor: "Linux",
      host_id: host_utils::derive_uuid(&[]),
      os: host_utils::get_linux_os_string(),
      host_name: whoami::devicename(),
      os_version: "",
      domain: String::new(),
      network_interfaces: vec![iface],
    }
  }

  /// Collect macOS-specific host information
  fn collect_macos(&self) -> HostInfo {
    let iface = self.collect_network_interface();
    // macOS use the mac address as host ID
    let host_id = iface
      .mac_address
      .as_ref()
      .map(|s| s.to_string())
      .unwrap_or_else(|| "00:00:00:00:00:00".to_string());

    // Use the fixed interface name en0 for macOS if it is not on macOS
    let iface = NetworkInterface {
      #[cfg(target_os = "macos")]
      name: iface.name,
      #[cfg(target_os = "macos")]
      description: iface.description,

      #[cfg(not(target_os = "macos"))]
      name: "en0".to_string(),
      #[cfg(not(target_os = "macos"))]
      description: "en0".to_string(),

      mac_address: iface.mac_address,
      ipv4: iface.ipv4,
      ipv6: iface.ipv6,
    };

    HostInfo {
      os_vendor: "Apple",
      host_id,
      os: host_utils::get_macos_os_string(),
      host_name: whoami::devicename(),
      os_version: host_utils::get_macos_version(),
      domain: self.get_domain(),
      network_interfaces: vec![iface],
    }
  }

  /// Collect Windows-specific host information
  fn collect_windows(&self) -> HostInfo {
    // Use the actual machine ID on Windows
    #[cfg(target_os = "windows")]
    let host_id = host_utils::get_machine_id().to_string();
    #[cfg(not(target_os = "windows"))]
    let host_id = host_utils::derive_uuid(&[]);

    let iface = self.collect_network_interface();

    // Use the fixed interface name if it is not on Windows
    let iface = NetworkInterface {
      #[cfg(target_os = "windows")]
      name: iface.name,
      #[cfg(target_os = "windows")]
      description: iface.description,

      #[cfg(not(target_os = "windows"))]
      name: derive_windows_network_name(&iface),
      #[cfg(not(target_os = "windows"))]
      description: "PANGP Virtual Ethernet Adapter Secure".to_string(),

      // mac address on windows should replace ':' with '-'
      mac_address: iface.mac_address.map(|mac| mac.replace(':', "-")),
      ipv4: iface.ipv4,
      ipv6: iface.ipv6,
    };

    // Add the local loopback interface
    let loopback_iface = NetworkInterface {
      name: derive_windows_network_name(&iface),
      description: "Software Loopback Interface 1".to_string(),
      mac_address: Some(String::new()),
      ipv4: Some("127.0.0.1".to_string()),
      ipv6: Some("::1".to_string()),
    };

    let ifaces = vec![iface, loopback_iface];

    HostInfo {
      os_vendor: "Microsoft",
      host_id,
      os: host_utils::get_windows_os_string(),
      host_name: whoami::devicename(),
      os_version: host_utils::get_windows_version(),
      domain: self.get_domain(),
      network_interfaces: ifaces,
    }
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

fn derive_windows_network_name(iface: &NetworkInterface) -> String {
  let seeds = [
    iface.mac_address.as_deref().unwrap_or("00:00:00:00:00:00"),
    iface.name.as_str(),
  ];

  format!("{{{}}}", host_utils::derive_uuid(&seeds).to_uppercase())
}
