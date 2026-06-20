use serde::{Deserialize, Serialize};
use specta::Type;

use super::{ClientOs, serial_number};
use platform::RuntimeNativeHostIdentity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HostIdentity {
  computer: String,
  host_id: String,
  #[serde(rename = "serialno")]
  serial_number: String,
  #[serde(rename = "macAddr")]
  mac_address: String,
}

impl HostIdentity {
  pub fn collect() -> Self {
    Self::collect_with_host_id(None)
  }

  pub(crate) fn collect_with_host_id(host_id: Option<&str>) -> Self {
    RuntimeNativeHostIdentity::collect(host_id).into_host_identity()
  }

  #[cfg(test)]
  pub(crate) fn new(computer: String, host_id: String, serialno: String, mac_addr: String) -> Self {
    Self::from_parts(computer, host_id, serialno, mac_addr)
  }

  pub(crate) fn from_parts(computer: String, host_id: String, serialno: String, mac_addr: String) -> Self {
    let host_id = non_empty(host_id, fallback_host_id());
    let computer = non_empty(computer, fallback_hostname());
    let serial_number = non_empty(serialno, derive_serial_number(&host_id));
    let mac_address = normalize_mac_colon(&mac_addr).unwrap_or_else(|| derive_mac_address(&host_id));

    Self {
      computer,
      host_id,
      serial_number,
      mac_address,
    }
  }

  pub fn computer(&self) -> &str {
    &self.computer
  }

  pub fn host_id(&self) -> &str {
    &self.host_id
  }

  pub fn serialno(&self) -> &str {
    &self.serial_number
  }

  pub fn mac_addr(&self) -> &str {
    &self.mac_address
  }

  pub fn with_computer(mut self, computer: String) -> Self {
    if !computer.trim().is_empty() {
      self.computer = computer;
    }
    self
  }

  pub(crate) fn runtime_os() -> ClientOs {
    RuntimeNativeHostIdentity::runtime_os()
  }
}

pub(super) fn fallback_hostname() -> String {
  whoami::hostname().unwrap_or_else(|_| String::from("localhost"))
}

#[derive(Debug, Clone)]
struct NativeHostIdentitySnapshot {
  host_id: String,
  computer: String,
  serial_number: Option<String>,
  mac_address: Option<String>,
}

impl NativeHostIdentitySnapshot {
  fn into_host_identity(self) -> HostIdentity {
    let host_id = non_empty(self.host_id, fallback_host_id());
    HostIdentity::from_parts(
      self.computer,
      host_id.clone(),
      self.serial_number.unwrap_or_else(|| derive_serial_number(&host_id)),
      self.mac_address.unwrap_or_else(|| derive_mac_address(&host_id)),
    )
  }
}

fn non_empty(value: String, fallback: String) -> String {
  if value.trim().is_empty() { fallback } else { value }
}

fn fallback_host_id() -> String {
  derive_uuid_from_seed(&fallback_hostname(), &["runtime-host-id"])
}

fn resolve_host_id(host_id_override: Option<&str>, collect: impl FnOnce() -> String) -> String {
  host_id_override
    .filter(|value| !value.trim().is_empty())
    .map(str::to_string)
    .unwrap_or_else(collect)
}

fn derive_serial_number(host_id: &str) -> String {
  serial_number::vmware_from_uuid(host_id).unwrap_or_else(|| {
    let uuid = derive_uuid_from_seed(host_id, &["runtime-serialno"]);
    serial_number::vmware_from_uuid(&uuid).expect("derived UUID should format as VMware serial")
  })
}

fn derive_mac_address(host_id: &str) -> String {
  let uuid = derive_uuid_from_seed(host_id, &["runtime-mac"]);
  let compact = uuid.replace('-', "");
  let bytes = compact
    .as_bytes()
    .chunks(2)
    .take(6)
    .map(|chunk| std::str::from_utf8(chunk).unwrap_or("00").to_lowercase())
    .collect::<Vec<_>>();

  bytes.join(":")
}

fn normalize_mac_colon(value: &str) -> Option<String> {
  let compact = value
    .trim()
    .replace('-', ":")
    .split(':')
    .map(|part| part.to_lowercase())
    .collect::<Vec<_>>();
  let valid = compact.len() == 6
    && compact
      .iter()
      .all(|part| part.len() == 2 && part.chars().all(|ch| ch.is_ascii_hexdigit()));

  valid.then(|| compact.join(":"))
}

fn primary_mac_colon() -> Option<String> {
  netdev::get_default_interface()
    .ok()
    .and_then(|iface| iface.mac_addr.map(|mac| mac.address()))
    .and_then(|mac| normalize_mac_colon(&mac))
}

fn derive_uuid_from_seed(seed: &str, parts: &[&str]) -> String {
  use uuid::Uuid;

  let namespace = Uuid::NAMESPACE_DNS;
  let name = format!("{}-{}", seed, parts.join("-"));

  Uuid::new_v5(&namespace, name.as_bytes()).hyphenated().to_string()
}

#[cfg(target_os = "macos")]
mod platform {
  use std::process::Command;

  use super::{
    ClientOs, NativeHostIdentitySnapshot, derive_mac_address, fallback_hostname, normalize_mac_colon,
    primary_mac_colon, resolve_host_id,
  };

  pub(super) struct RuntimeNativeHostIdentity;

  impl RuntimeNativeHostIdentity {
    pub(super) fn runtime_os() -> ClientOs {
      ClientOs::Mac
    }

    pub(super) fn collect(host_id_override: Option<&str>) -> NativeHostIdentitySnapshot {
      let native_mac = macos_builtin_mac().or_else(primary_mac_colon);
      let host_id = resolve_host_id(host_id_override, || {
        native_mac
          .clone()
          .unwrap_or_else(|| derive_mac_address(&fallback_hostname()))
      });
      NativeHostIdentitySnapshot {
        host_id,
        computer: macos_computer_name().unwrap_or_else(fallback_hostname),
        serial_number: collect_macos_serial_number(),
        mac_address: native_mac,
      }
    }
  }

  fn macos_computer_name() -> Option<String> {
    ["ComputerName", "LocalHostName"].into_iter().find_map(|name| {
      let output = Command::new("scutil").args(["--get", name]).output().ok()?;
      if !output.status.success() {
        return None;
      }

      String::from_utf8(output.stdout)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    })
  }

  fn macos_builtin_mac() -> Option<String> {
    macos_networksetup_mac("Wi-Fi")
      .or_else(|| macos_networksetup_mac("AirPort"))
      .or_else(macos_system_profiler_wifi_mac)
  }

  fn macos_networksetup_mac(port_name: &str) -> Option<String> {
    let output = Command::new("networksetup")
      .arg("-listallhardwareports")
      .output()
      .ok()?;
    if !output.status.success() {
      return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let mut in_port = false;
    for line in stdout.lines() {
      if let Some(port) = line.strip_prefix("Hardware Port: ") {
        in_port = port == port_name;
        continue;
      }

      if in_port && let Some(mac) = line.strip_prefix("Ethernet Address: ") {
        return normalize_mac_colon(mac);
      }
    }

    None
  }

  fn macos_system_profiler_wifi_mac() -> Option<String> {
    let output = Command::new("system_profiler").arg("SPNetworkDataType").output().ok()?;
    if !output.status.success() {
      return None;
    }

    let stdout = String::from_utf8(output.stdout).ok()?;
    let mut in_wifi = false;
    for line in stdout.lines() {
      let trimmed = line.trim();
      if trimmed.ends_with(':') && !trimmed.starts_with("MAC Address:") {
        in_wifi = trimmed == "Wi-Fi:" || trimmed == "AirPort:";
        continue;
      }

      if in_wifi && let Some(mac) = trimmed.strip_prefix("MAC Address: ") {
        return normalize_mac_colon(mac);
      }
    }

    None
  }

  fn collect_macos_serial_number() -> Option<String> {
    let output = Command::new("ioreg")
      .args(["-rd1", "-c", "IOPlatformExpertDevice"])
      .output()
      .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;

    stdout.lines().find_map(|line| {
      line
        .split_once("IOPlatformSerialNumber")
        .and_then(|(_, value)| value.split_once('='))
        .map(|(_, value)| value.trim().trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
    })
  }
}

#[cfg(target_os = "windows")]
mod platform {
  use std::process::Command;

  use super::{ClientOs, NativeHostIdentitySnapshot, fallback_hostname, primary_mac_colon, resolve_host_id};

  pub(super) struct RuntimeNativeHostIdentity;

  impl RuntimeNativeHostIdentity {
    pub(super) fn runtime_os() -> ClientOs {
      ClientOs::Windows
    }

    pub(super) fn collect(host_id_override: Option<&str>) -> NativeHostIdentitySnapshot {
      let host_id = resolve_host_id(host_id_override, Self::host_id);
      NativeHostIdentitySnapshot {
        host_id,
        computer: fallback_hostname(),
        serial_number: collect_windows_serial_number(),
        mac_address: primary_mac_colon(),
      }
    }

    fn host_id() -> String {
      windows_machine_seed().unwrap_or_else(fallback_hostname)
    }
  }

  fn collect_windows_serial_number() -> Option<String> {
    let output = Command::new("wmic")
      .args(["bios", "get", "serialnumber"])
      .output()
      .ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;

    stdout
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty() && *line != "SerialNumber")
      .map(str::to_string)
      .next()
  }

  fn windows_machine_seed() -> Option<String> {
    collect_windows_csproduct_uuid().or_else(collect_windows_machine_guid)
  }

  fn collect_windows_csproduct_uuid() -> Option<String> {
    let output = Command::new("wmic").args(["csproduct", "get", "uuid"]).output().ok()?;
    let stdout = String::from_utf8(output.stdout).ok()?;

    stdout
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty() && *line != "UUID")
      .map(str::to_string)
      .next()
  }

  fn collect_windows_machine_guid() -> Option<String> {
    let output = Command::new("reg")
      .args(["query", r"HKLM\SOFTWARE\Microsoft\Cryptography", "/v", "MachineGuid"])
      .output()
      .ok()?;
    if !output.status.success() {
      return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;

    stdout.lines().find_map(|line| {
      let mut parts = line.split_whitespace();
      if parts.next()? != "MachineGuid" {
        return None;
      }

      parts.last().map(str::to_string)
    })
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod platform {
  use super::{
    ClientOs, NativeHostIdentitySnapshot, derive_uuid_from_seed, fallback_hostname, primary_mac_colon, resolve_host_id,
  };
  use log::debug;

  pub(super) struct RuntimeNativeHostIdentity;

  impl RuntimeNativeHostIdentity {
    pub(super) fn runtime_os() -> ClientOs {
      ClientOs::Linux
    }

    pub(super) fn collect(host_id_override: Option<&str>) -> NativeHostIdentitySnapshot {
      if let Some(host_id) = host_id_override.filter(|value| !value.trim().is_empty()) {
        debug!("Runtime host-id source: explicit override ({})", host_id);
      }
      let host_id = resolve_host_id(host_id_override, Self::host_id);
      NativeHostIdentitySnapshot {
        host_id,
        computer: fallback_hostname(),
        serial_number: collect_linux_serial_number(),
        mac_address: primary_mac_colon(),
      }
    }

    fn host_id() -> String {
      if let Some(host_id) = collect_linux_product_uuid() {
        debug!("Runtime host-id source: /sys/class/dmi/id/product_uuid ({})", host_id);
        return host_id;
      }

      let (source_name, source) = collect_linux_fallback_seed().unwrap_or_else(|| {
        let hostname = fallback_hostname();
        ("hostname", hostname)
      });
      let host_id = derive_uuid_from_seed(&source, &[]);
      debug!("Runtime host-id source: derived from {} ({})", source_name, host_id);
      host_id
    }
  }

  fn collect_linux_product_uuid() -> Option<String> {
    read_linux_identity_file("/sys/class/dmi/id/product_uuid").and_then(|value| normalize_uuid(&value))
  }

  fn normalize_uuid(value: &str) -> Option<String> {
    uuid::Uuid::parse_str(value.trim())
      .ok()
      .map(|uuid| uuid.hyphenated().to_string())
  }

  fn collect_linux_fallback_seed() -> Option<(&'static str, String)> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
      .iter()
      .find_map(|path| read_linux_identity_file(path).map(|value| (*path, value)))
  }

  fn read_linux_identity_file(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
      .ok()
      .map(|value| value.trim().to_string())
      .filter(|value| is_usable_linux_identity_value(value))
  }

  fn is_usable_linux_identity_value(value: &str) -> bool {
    !value.is_empty() && value != "None" && value != "To Be Filled By O.E.M."
  }

  fn collect_linux_serial_number() -> Option<String> {
    ["/sys/class/dmi/id/product_serial", "/sys/class/dmi/id/product_uuid"]
      .iter()
      .find_map(|path| read_linux_identity_file(path))
  }

  #[cfg(test)]
  mod tests {
    use super::*;

    #[test]
    fn normalize_uuid_preserves_real_uuid_identity() {
      assert_eq!(
        normalize_uuid("5A784D56-6461-19AC-9EA9-D36A3B9C6CEF"),
        Some("5a784d56-6461-19ac-9ea9-d36a3b9c6cef".to_string())
      );
      assert_eq!(normalize_uuid("not-a-uuid"), None);
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn collect_produces_non_empty_identity() {
    let identity = HostIdentity::collect();

    assert!(!identity.computer().is_empty());
    assert!(!identity.host_id().is_empty());
    assert!(!identity.serialno().is_empty());
    assert!(!identity.mac_addr().is_empty());
    assert_mac(identity.mac_addr(), ':');
  }

  #[test]
  fn collect_with_host_id_uses_provided_host_id() {
    let identity = HostIdentity::collect_with_host_id(Some("provided-host-id"));

    assert_eq!(identity.host_id(), "provided-host-id");
    assert!(!identity.computer().is_empty());
    assert!(!identity.serialno().is_empty());
    assert!(!identity.mac_addr().is_empty());
  }

  #[test]
  fn collect_with_blank_host_id_falls_back_to_runtime_host_id() {
    let identity = HostIdentity::collect_with_host_id(Some(" "));

    assert!(!identity.host_id().is_empty());
    assert_ne!(identity.host_id(), " ");
  }

  #[test]
  fn resolve_host_id_does_not_collect_when_override_is_provided() {
    let host_id = resolve_host_id(Some("provided-host-id"), || {
      panic!("runtime host id should not be collected")
    });

    assert_eq!(host_id, "provided-host-id");
  }

  #[test]
  fn host_identity_normalizes_mac_to_lowercase_colon_format() {
    let identity = HostIdentity::new(
      "computer".to_string(),
      "host-id".to_string(),
      "serial-number".to_string(),
      "AA-BB-CC-DD-EE-FF".to_string(),
    );

    assert_eq!(identity.mac_addr(), "aa:bb:cc:dd:ee:ff");
  }

  #[test]
  fn native_snapshot_uses_host_id_as_derivation_seed() {
    let native = RuntimeNativeHostIdentity::collect(None);

    assert!(!native.host_id.is_empty());
  }

  #[test]
  fn serialization_keeps_globalprotect_field_names() {
    let identity = HostIdentity::new(
      "computer".to_string(),
      "host-id".to_string(),
      "serial-number".to_string(),
      "aa:bb:cc:dd:ee:ff".to_string(),
    );
    let value = serde_json::to_value(identity).unwrap();

    assert_eq!(value["hostId"], "host-id");
    assert_eq!(value["computer"], "computer");
    assert!(value.get("osVersion").is_none());
    assert!(value.get("softwareVersion").is_none());
    assert_eq!(value["serialno"], "serial-number");
    assert_eq!(value["macAddr"], "aa:bb:cc:dd:ee:ff");
  }

  #[test]
  fn native_identity_falls_back_to_derived_values_when_missing() {
    let native = NativeHostIdentitySnapshot {
      host_id: "native-host-id".to_string(),
      computer: "native-computer".to_string(),
      serial_number: None,
      mac_address: None,
    };

    let identity = native.into_host_identity();

    assert_eq!(identity.computer(), "native-computer");
    assert_eq!(identity.host_id(), "native-host-id");
    assert!(identity.serialno().starts_with("VMware-"));
    assert_mac(identity.mac_addr(), ':');
  }

  #[test]
  fn derived_serial_uses_vmware_uuid_byte_order_when_host_id_is_uuid() {
    let identity = HostIdentity::new(
      "computer".to_string(),
      "5a784d56-6461-19ac-9ea9-d36a3b9c6cef".to_string(),
      String::new(),
      "aa:bb:cc:dd:ee:ff".to_string(),
    );

    assert_eq!(
      identity.serialno(),
      "VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef"
    );
  }

  #[test]
  fn empty_host_id_falls_back_to_non_empty_value() {
    let identity = HostIdentity::new(String::new(), String::new(), String::new(), String::new());

    assert!(!identity.host_id().is_empty());
    assert!(!identity.computer().is_empty());
    assert!(!identity.serialno().is_empty());
    assert!(!identity.mac_addr().is_empty());
  }

  fn assert_mac(value: &str, separator: char) {
    let parts: Vec<&str> = value.split(separator).collect();
    assert_eq!(parts.len(), 6, "MAC should have 6 octets");
    for part in &parts {
      assert_eq!(part.len(), 2);
      assert!(part.chars().all(|c| c.is_ascii_hexdigit()));
    }
  }
}
