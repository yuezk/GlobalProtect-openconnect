use askama::Template;
use clap::Args;
use gpapi::{clap::args::Os, utils::host_utils};
use log::debug;
use std::{collections::HashMap, env};
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
  host_info: HostInfo<'a>,
  md5: &'a str,
}

struct HostInfo<'a> {
  os_vendor: &'a str,
  os_version: &'a str,
  host_id: String,
  host_name: String,
  software_version: &'a str,
  domain: String,
  network_interfaces: Vec<NetworkInterface>,
}

impl<'a> HostInfo<'a> {
  fn default_ipv4(&self) -> &str {
    self
      .network_interfaces
      .first()
      .and_then(|iface| iface.ipv4.as_deref())
      .unwrap_or_default()
  }

  fn default_ipv6(&self) -> &str {
    self
      .network_interfaces
      .first()
      .and_then(|iface| iface.ipv6.as_deref())
      .unwrap_or_default()
  }
}

#[derive(Clone)]
struct NetworkInterface {
  name: String,
  description: String,
  mac_address: Option<String>,
  ipv4: Option<String>,
  ipv6: Option<String>,
}

impl NetworkInterface {
  fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
    Self {
      name: name.into(),
      description: description.into(),
      mac_address: None,
      ipv4: None,
      ipv6: None,
    }
  }

  fn with_ipv4(mut self, ipv4: Option<String>) -> Self {
    self.ipv4 = ipv4;
    self
  }

  fn with_ipv6(mut self, ipv6: Option<String>) -> Self {
    self.ipv6 = ipv6;
    self
  }
}

#[derive(Args)]
pub(crate) struct HipArgs {
  #[arg(
    long,
    help = "The GP client version, defaults to APP_VERSION or the packaged version"
  )]
  client_version: Option<String>,

  #[arg(long, value_enum, default_value_t = HipArgs::default_os(), help = "The client OS")]
  client_os: Os,

  #[arg(long, help = "The OS version string, defaults to a computed value for the target OS")]
  os_version: Option<String>,

  #[arg(long, help = "The authentication cookie")]
  cookie: String,

  #[arg(long, help = "The client IPv4 address")]
  client_ip: Option<String>,

  #[arg(long, help = "The client IPv6 address")]
  client_ipv6: Option<String>,

  #[arg(long, help = "The MD5 digest to encode into the HIP report")]
  md5: String,
}

impl HipArgs {
  fn default_os() -> Os {
    if cfg!(target_os = "macos") {
      Os::Mac
    } else if cfg!(target_os = "windows") {
      Os::Windows
    } else {
      Os::Linux
    }
  }

  fn client_version(&self) -> String {
    self
      .client_version
      .clone()
      .or_else(|| env::var("APP_VERSION").ok())
      .unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
  }

  fn os_version(&self) -> String {
    self.os_version.clone().unwrap_or_else(|| match self.client_os {
      Os::Linux => host_utils::get_linux_os_string().to_string(),
      Os::Windows => host_utils::get_windows_os_string().to_string(),
      Os::Mac => host_utils::get_macos_os_string().to_string(),
    })
  }
}

pub(crate) struct HipHandler<'a> {
  args: &'a HipArgs,
}

impl<'a> HipHandler<'a> {
  pub(crate) fn new(args: &'a HipArgs) -> Self {
    Self { args }
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    let cookie_params = self.parse_cookie();
    let report = self.generate_hip_report(&cookie_params)?;

    debug!("Generated HIP report:\n{}", report);
    println!("{}", report);

    Ok(())
  }

  fn parse_cookie(&self) -> HashMap<String, String> {
    serde_urlencoded::from_str(&self.args.cookie).unwrap_or_default()
  }

  fn generate_hip_report(&self, cookie_params: &HashMap<String, String>) -> anyhow::Result<String> {
    let (generate_time, day, month, year) = get_current_time_components();
    let client_version = self.args.client_version();
    let os_version = self.args.os_version();
    let user_name = cookie_params.get("user").map(|value| value.as_str()).unwrap_or("");
    let host_info = self.collect_host_info(cookie_params, &os_version);

    let template = HipReportTemplate {
      client_version: &client_version,
      generate_time,
      day,
      month,
      year,
      user_name,
      host_info,
      md5: &self.args.md5,
    };

    format_xml(&template.render()?)
  }

  fn collect_host_info<'b>(&'b self, cookie_params: &'b HashMap<String, String>, os_version: &'b str) -> HostInfo<'b> {
    let domain = cookie_params.get("domain").cloned().unwrap_or_default();
    let ipv4 = self
      .args
      .client_ip
      .clone()
      .or_else(|| cookie_params.get("preferred-ip").cloned());
    let ipv6 = self
      .args
      .client_ipv6
      .clone()
      .or_else(|| cookie_params.get("preferred-ipv6").cloned());

    match self.args.client_os {
      Os::Linux => HostInfo {
        os_vendor: "Linux",
        os_version,
        host_id: host_utils::derive_uuid(&["linux"]),
        host_name: whoami::devicename(),
        software_version: "",
        domain,
        network_interfaces: vec![NetworkInterface::new("enp1s0f0", "enp1s0f0")
          .with_ipv4(ipv4)
          .with_ipv6(ipv6)],
      },
      Os::Mac => HostInfo {
        os_vendor: "Apple",
        os_version,
        host_id: host_utils::get_machine_id().to_string(),
        host_name: whoami::devicename(),
        software_version: host_utils::get_macos_version(),
        domain,
        network_interfaces: vec![NetworkInterface::new("en0", "en0").with_ipv4(ipv4).with_ipv6(ipv6)],
      },
      Os::Windows => {
        let iface = NetworkInterface::new(
          format!("{{{}}}", host_utils::derive_uuid(&["pangp"]).to_uppercase()),
          "PANGP Virtual Ethernet Adapter Secure",
        )
        .with_ipv4(ipv4)
        .with_ipv6(ipv6);

        let loopback = NetworkInterface {
          name: iface.name.clone(),
          description: "Software Loopback Interface 1".to_string(),
          mac_address: Some(String::new()),
          ipv4: Some("127.0.0.1".to_string()),
          ipv6: Some("::1".to_string()),
        };

        HostInfo {
          os_vendor: "Microsoft",
          os_version,
          host_id: host_utils::get_machine_id().to_string(),
          host_name: whoami::devicename(),
          software_version: host_utils::get_windows_version(),
          domain,
          network_interfaces: vec![iface, loopback],
        }
      }
    }
  }
}

fn get_current_time_components() -> (String, String, String, String) {
  let now = chrono::Local::now();
  (
    now.format("%m/%d/%Y %H:%M:%S").to_string(),
    now.format("%d").to_string(),
    now.format("%m").to_string(),
    now.format("%Y").to_string(),
  )
}

fn format_xml(xml_str: &str) -> anyhow::Result<String> {
  let xml = Element::parse(xml_str.as_bytes())?;
  let mut xml_buf = Vec::new();
  xml.write_with_config(&mut xml_buf, xmltree::EmitterConfig::new().perform_indent(true))?;
  Ok(String::from_utf8(xml_buf)?)
}
