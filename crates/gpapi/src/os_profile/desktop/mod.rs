use super::*;
use crate::collection::{CollectionBudget, CollectionControl};
use std::{io, time::Duration};
#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
mod unix;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
use macos::RuntimeNativeHostIdentity;
#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
use unix::RuntimeNativeHostIdentity;
#[cfg(target_os = "windows")]
use windows::RuntimeNativeHostIdentity;
#[cfg(not(any(
  target_os = "macos",
  target_os = "windows",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
)))]
compile_error!("gpapi desktop collection is unsupported on this target; disable the desktop feature");
pub fn collect_identity() -> HostIdentity {
  collect_with_host_id(None)
}

pub fn collect_identity_with_control(control: &dyn CollectionControl) -> io::Result<HostIdentity> {
  collect_with_host_id_and_control(None, control)
}

pub(super) fn collect_with_host_id(host_id: Option<&str>) -> HostIdentity {
  match collect_with_host_id_and_control(host_id, &CollectionBudget::new(Duration::from_secs(60))) {
    Ok(identity) => identity,
    Err(error) => {
      log::warn!("Host identity collection was unavailable: {error}");
      default_with_host_id(host_id)
    }
  }
}

/// Best-effort convenience callers retain the established derived identity
/// when native collection is unavailable. Controlled HIP callers get errors.
pub(super) fn default_with_host_id(host_id: Option<&str>) -> HostIdentity {
  identity_from_parts(
    fallback_hostname(),
    resolve_host_id(host_id, fallback_host_id),
    String::new(),
    primary_mac_colon().unwrap_or_default(),
  )
}

pub(super) fn collect_with_host_id_and_control(
  host_id: Option<&str>,
  control: &dyn CollectionControl,
) -> io::Result<HostIdentity> {
  control.check()?;
  let identity = RuntimeNativeHostIdentity::collect_with_control(host_id, control)?.into_host_identity();
  control.check()?;
  Ok(identity)
}

pub fn identity_from_parts(computer: String, host_id: String, serialno: String, mac_addr: String) -> HostIdentity {
  let host_id = non_empty(host_id, fallback_host_id());
  let computer = non_empty(computer, fallback_hostname());
  let serial_number = non_empty(serialno, derive_serial_number(&host_id));
  let mac_address = normalize_mac_colon(&mac_addr).unwrap_or_else(|| derive_mac_address(&host_id));

  HostIdentity::from_parts(computer, host_id, serial_number, mac_address)
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
    identity_from_parts(
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

pub fn runtime_client_os() -> ClientOs {
  RuntimeNativeHostIdentity::runtime_os()
}

/// Strict native collection, followed by deterministic profile projection.
pub fn collect_profile_with_control(
  builder: OsProfileBuilder,
  host_id_override: Option<&str>,
  control: &dyn CollectionControl,
) -> io::Result<OsProfile> {
  control.check()?;
  let identity = match &builder.host_identity {
    Some(identity) => identity.clone(),
    None => collect_with_host_id_and_control(host_id_override, control)?,
  };
  let runtime_os = runtime_client_os();
  // Preserve the old behavior: only probe the native version when reporting it.
  let (os_version, software_version) = if builder.client_os == runtime_os {
    RuntimeNativeHostIdentity::versions(control)?
  } else {
    (defaults::os_version(runtime_os), defaults::software_version(runtime_os))
  };
  control.check()?;
  Ok(builder.build(HostSnapshot {
    runtime_os,
    identity,
    os_version,
    software_version,
  }))
}

/// Existing desktop best-effort policy; strict HIP callers use the controlled entry point.
pub fn collect_profile(builder: OsProfileBuilder, host_id_override: Option<&str>) -> OsProfile {
  match collect_profile_with_control(
    builder.clone(),
    host_id_override,
    &CollectionBudget::new(Duration::from_secs(60)),
  ) {
    Ok(profile) => profile,
    Err(error) => {
      log::warn!("OS profile collection was unavailable: {error}");
      let identity = builder
        .host_identity
        .clone()
        .unwrap_or_else(|| default_with_host_id(host_id_override));
      let runtime_os = runtime_client_os();
      let (os_version, software_version) = RuntimeNativeHostIdentity::fallback_versions();
      builder.build(HostSnapshot {
        runtime_os,
        identity,
        os_version,
        software_version,
      })
    }
  }
}
pub fn default_computer_name(client_os: &ClientOs) -> String {
  projection::project(runtime_client_os(), *client_os, &collect_identity(), None)
    .computer()
    .into()
}
pub fn default_os_version(client_os: &ClientOs) -> String {
  if *client_os == runtime_client_os() {
    RuntimeNativeHostIdentity::versions(&CollectionBudget::new(Duration::from_secs(60)))
      .unwrap_or_else(|_| RuntimeNativeHostIdentity::fallback_versions())
      .0
  } else {
    defaults::os_version(*client_os)
  }
}
pub fn default_software_version(client_os: &ClientOs) -> String {
  if *client_os == runtime_client_os() {
    RuntimeNativeHostIdentity::versions(&CollectionBudget::new(Duration::from_secs(60)))
      .unwrap_or_else(|_| RuntimeNativeHostIdentity::fallback_versions())
      .1
  } else {
    defaults::software_version(*client_os)
  }
}

#[cfg(test)]
mod tests;
