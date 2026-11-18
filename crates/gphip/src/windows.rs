//! Public JSON adapter and native Windows host identity.
use super::windows_report::WindowsReport;
use gpapi::{collection::CollectionControl, process::collection::CollectorCommands};
use std::io;

pub(super) fn collect(commands: &CollectorCommands<'_>, control: &dyn CollectionControl) -> io::Result<WindowsReport> {
  let executable = std::env::current_exe()?;
  let helper = executable
    .parent()
    .ok_or_else(|| io::Error::other("Application directory is unavailable"))?
    .join("gp-posture.exe");
  let output = commands
    .run_executable(&helper, &[])?
    .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Windows posture helper is not installed"))?;
  if !output.status.success() {
    return Err(io::Error::other("Windows posture collection failed"));
  }
  let report: WindowsReport = serde_json::from_slice(&output.stdout).map_err(io::Error::other)?;
  for category in &report.unavailable {
    log::warn!("Windows HIP data unavailable: {category}");
  }
  control.check()?;
  Ok(report)
}

/// Windows supplies the loopback adapter's real ID through GetAdaptersAddresses (via netdev).
pub(super) fn loopback_interfaces() -> Vec<super::NetworkInterface> {
  netdev::get_interfaces()
    .into_iter()
    .filter(|interface| interface.is_loopback())
    .map(|interface| {
      super::NetworkInterface::new(interface.name.clone(), interface.description.unwrap_or(interface.name))
        .with_mac(Some(
          interface
            .mac_addr
            .map(|mac| super::format_native_windows_mac(mac.address()))
            .unwrap_or_default(),
        ))
        .with_ipv4(interface.ipv4.first().map(|ip| ip.addr().to_string()))
        .with_ipv6(interface.ipv6.first().map(|ip| ip.addr().to_string()))
    })
    .collect()
}

/// Primary DNS domain, or physical DNS domain if the primary query fails.
pub(super) fn domain() -> io::Result<String> {
  use windows::Win32::{
    Networking::ActiveDirectory::{
      DSROLE_PRIMARY_DOMAIN_INFO_BASIC, DsRoleFreeMemory, DsRoleGetPrimaryDomainInformation,
      DsRolePrimaryDomainInfoBasic,
    },
    System::SystemInformation::{ComputerNamePhysicalDnsDomain, GetComputerNameExW},
  };
  use windows::core::PWSTR;

  let mut buffer = std::ptr::null_mut();
  // SAFETY: a null server selects this computer; Windows allocates and owns the returned structure.
  let status = unsafe { DsRoleGetPrimaryDomainInformation(None, DsRolePrimaryDomainInfoBasic, &mut buffer) };
  if status == 0 && !buffer.is_null() {
    // SAFETY: the requested information level returns this structure. Copy the string before freeing its allocation.
    let result = unsafe { copy_domain_name((*buffer.cast::<DSROLE_PRIMARY_DOMAIN_INFO_BASIC>()).DomainNameDns) };
    unsafe { DsRoleFreeMemory(buffer.cast()) };
    return result;
  }

  let mut name = [0u16; 257];
  let mut size = name.len() as u32;
  // SAFETY: the writable UTF-16 buffer and its character count remain valid for the call.
  unsafe { GetComputerNameExW(ComputerNamePhysicalDnsDomain, Some(PWSTR(name.as_mut_ptr())), &mut size) }
    .map_err(io::Error::other)?;
  String::from_utf16(&name[..size as usize]).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

// SAFETY: a non-null name must point to a live NUL-terminated UTF-16 string.
unsafe fn copy_domain_name(name: windows::core::PWSTR) -> io::Result<String> {
  if name.is_null() {
    // A successful primary-domain query with no DNS name is an empty domain, including workgroup hosts.
    return Ok(String::new());
  }
  unsafe { name.to_string() }.map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
  use super::*;
  use windows::core::PWSTR;
  #[test]
  fn primary_domain_preserves_dns_name_and_empty_workgroup_domain() {
    let mut name: Vec<u16> = "Lab.Example.test\0".encode_utf16().collect();
    assert_eq!(
      unsafe { copy_domain_name(PWSTR(name.as_mut_ptr())) }.unwrap(),
      "Lab.Example.test"
    );
    assert_eq!(unsafe { copy_domain_name(PWSTR::null()) }.unwrap(), "");
  }
}
