use std::process::Command;

use super::{ClientOs, NativeHostIdentitySnapshot, fallback_hostname, primary_mac_colon, resolve_host_id};

pub(super) struct RuntimeNativeHostIdentity;

impl RuntimeNativeHostIdentity {
  pub(super) fn runtime_os() -> ClientOs {
    ClientOs::Windows
  }

  pub(super) fn collect_with_control(
    host_id_override: Option<&str>,
    control: &dyn super::CollectionControl,
  ) -> super::io::Result<NativeHostIdentitySnapshot> {
    control.check()?;
    let identity = Self::collect(host_id_override);
    control.check()?;
    Ok(identity)
  }

  fn collect(host_id_override: Option<&str>) -> NativeHostIdentitySnapshot {
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

impl RuntimeNativeHostIdentity {
  pub(super) fn versions(control: &dyn super::CollectionControl) -> super::io::Result<(String, String)> {
    control.check()?;
    Ok(Self::fallback_versions())
  }
  pub(super) fn fallback_versions() -> (String, String) {
    {
      let info = os_info::get();
      let edition = info
        .edition()
        .map(str::to_owned)
        .unwrap_or_else(|| whoami::distro().unwrap_or_else(|_| super::DEFAULT_WINDOWS_DISTRO.into()));
      let version = match info.version() {
        os_info::Version::Unknown => super::DEFAULT_WINDOWS_VERSION.into(),
        version => format!("{}.0", version),
      };
      (format!("Microsoft {} , 64-bit", edition), version)
    }
  }
}
