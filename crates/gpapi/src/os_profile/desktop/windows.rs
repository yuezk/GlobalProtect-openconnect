use std::{io, os::windows::process::CommandExt, process::Command};
use windows_sys::Win32::System::Registry::{HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW};
use windows_sys::Win32::System::SystemInformation::{ComputerNamePhysicalNetBIOS, GetComputerNameExW};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

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
    .creation_flags(CREATE_NO_WINDOW)
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
  collect_windows_csproduct_uuid().or_else(|| collect_windows_machine_guid().ok())
}

fn collect_windows_csproduct_uuid() -> Option<String> {
  let output = Command::new("wmic")
    .creation_flags(CREATE_NO_WINDOW)
    .args(["csproduct", "get", "uuid"])
    .output()
    .ok()?;
  let stdout = String::from_utf8(output.stdout).ok()?;

  stdout
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && *line != "UUID")
    .map(str::to_string)
    .next()
}

// Native Windows uses the OS installation ID and physical NetBIOS name.
// Keep the hardware identity used to seed other OS profiles unchanged.
pub(super) fn native_profile_identity(
  identity: super::HostIdentity,
  host_id_override: Option<&str>,
) -> io::Result<super::HostIdentity> {
  let host_id = match host_id_override.filter(|id| !id.trim().is_empty()) {
    Some(id) => id.to_owned(),
    None => collect_windows_machine_guid()?,
  };
  Ok(super::HostIdentity::from_parts(
    collect_windows_computer_name()?,
    host_id,
    identity.serialno().into(),
    identity.mac_addr().into(),
  ))
}

fn collect_windows_computer_name() -> io::Result<String> {
  let mut name = [0u16; 256];
  let mut size = name.len() as u32;
  // SAFETY: the buffer and character count describe writable UTF-16 storage.
  if unsafe { GetComputerNameExW(ComputerNamePhysicalNetBIOS, name.as_mut_ptr(), &mut size) } == 0 {
    return Err(io::Error::last_os_error());
  }
  String::from_utf16(&name[..size as usize]).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn collect_windows_machine_guid() -> io::Result<String> {
  let key: Vec<u16> = "SOFTWARE\\Microsoft\\Cryptography\0".encode_utf16().collect();
  let value: Vec<u16> = "MachineGuid\0".encode_utf16().collect();
  let mut buffer = [0u16; 128];
  let mut size = std::mem::size_of_val(&buffer) as u32;
  // SAFETY: both names are NUL-terminated; buffer and byte length describe writable storage.
  let status = unsafe {
    RegGetValueW(
      HKEY_LOCAL_MACHINE,
      key.as_ptr(),
      value.as_ptr(),
      RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY,
      std::ptr::null_mut(),
      buffer.as_mut_ptr().cast(),
      &mut size,
    )
  };
  if status != 0 {
    return Err(io::Error::from_raw_os_error(status as i32));
  }
  let end = buffer.iter().position(|value| *value == 0).unwrap_or(buffer.len());
  let guid = String::from_utf16(&buffer[..end]).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
  if guid.trim().is_empty() {
    return Err(io::Error::new(
      io::ErrorKind::InvalidData,
      "Windows MachineGuid is empty",
    ));
  }
  Ok(guid)
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
