use super::{
  ClientOs, NativeHostIdentitySnapshot, derive_mac_address, fallback_hostname, normalize_mac_colon, primary_mac_colon,
  resolve_host_id,
};
use crate::collection::CollectionControl;
use crate::process::collection::CollectorCommands;
use std::io;

pub(super) struct RuntimeNativeHostIdentity;

impl RuntimeNativeHostIdentity {
  pub(super) fn runtime_os() -> ClientOs {
    ClientOs::Mac
  }

  pub(super) fn collect_with_control(
    host_id_override: Option<&str>,
    control: &dyn CollectionControl,
  ) -> io::Result<NativeHostIdentitySnapshot> {
    let commands = CollectorCommands::new(control);
    let native_mac = macos_builtin_mac(&commands)?.or_else(primary_mac_colon);
    control.check()?;
    let host_id = resolve_host_id(host_id_override, || {
      native_mac
        .clone()
        .unwrap_or_else(|| derive_mac_address(&fallback_hostname()))
    });
    let computer = macos_computer_name(&commands)?.unwrap_or_else(fallback_hostname);
    let serial_number = collect_macos_serial_number(&commands)?;
    control.check()?;
    Ok(NativeHostIdentitySnapshot {
      host_id,
      computer,
      serial_number,
      mac_address: native_mac,
    })
  }
}

fn output(commands: &CollectorCommands<'_>, program: &str, args: &[&str]) -> io::Result<Option<String>> {
  Ok(
    commands
      .run(program, args)?
      .filter(|output| output.status.success())
      .and_then(|output| String::from_utf8(output.stdout).ok()),
  )
}

fn macos_computer_name(commands: &CollectorCommands<'_>) -> io::Result<Option<String>> {
  for name in ["ComputerName", "LocalHostName"] {
    if let Some(value) = output(commands, "scutil", &["--get", name])?
      .map(|value| value.trim().to_string())
      .filter(|value| !value.is_empty())
    {
      return Ok(Some(value));
    }
  }
  Ok(None)
}

fn macos_builtin_mac(commands: &CollectorCommands<'_>) -> io::Result<Option<String>> {
  if let Some(stdout) = output(commands, "networksetup", &["-listallhardwareports"])? {
    for name in ["Wi-Fi", "AirPort"] {
      if let Some(mac) = parse_hardware_port_mac(&stdout, name) {
        return Ok(Some(mac));
      }
    }
  }
  macos_system_profiler_wifi_mac(commands)
}

fn parse_hardware_port_mac(stdout: &str, name: &str) -> Option<String> {
  let mut in_port = false;
  for line in stdout.lines() {
    if let Some(port) = line.strip_prefix("Hardware Port: ") {
      in_port = port == name;
      continue;
    }
    if in_port && let Some(mac) = line.strip_prefix("Ethernet Address: ") {
      return normalize_mac_colon(mac);
    }
  }
  None
}

fn macos_system_profiler_wifi_mac(commands: &CollectorCommands<'_>) -> io::Result<Option<String>> {
  let Some(stdout) = output(commands, "system_profiler", &["SPNetworkDataType"])? else {
    return Ok(None);
  };
  let mut in_wifi = false;
  for line in stdout.lines() {
    let trimmed = line.trim();
    if trimmed.ends_with(':') && !trimmed.starts_with("MAC Address:") {
      in_wifi = trimmed == "Wi-Fi:" || trimmed == "AirPort:";
      continue;
    }
    if in_wifi && let Some(mac) = trimmed.strip_prefix("MAC Address: ") {
      return Ok(normalize_mac_colon(mac));
    }
  }
  Ok(None)
}

fn collect_macos_serial_number(commands: &CollectorCommands<'_>) -> io::Result<Option<String>> {
  let Some(stdout) = output(commands, "ioreg", &["-rd1", "-c", "IOPlatformExpertDevice"])? else {
    return Ok(None);
  };
  Ok(stdout.lines().find_map(|line| {
    line
      .split_once("IOPlatformSerialNumber")
      .and_then(|(_, value)| value.split_once('='))
      .map(|(_, value)| value.trim().trim_matches('"').to_string())
      .filter(|value| !value.is_empty())
  }))
}

impl RuntimeNativeHostIdentity {
  pub(super) fn versions(control: &dyn super::CollectionControl) -> super::io::Result<(String, String)> {
    let commands = crate::process::collection::CollectorCommands::new(control);
    let version = commands
      .run("sw_vers", &["-productVersion"])?
      .filter(|output| output.status.success())
      .and_then(|output| String::from_utf8(output.stdout).ok())
      .map(|version| version.trim().to_string())
      .filter(|version| !version.is_empty())
      .unwrap_or_else(|| super::DEFAULT_MACOS_VERSION.into());
    control.check()?;
    Ok((format!("Apple Mac OS X {version}"), version))
  }
  pub(super) fn fallback_versions() -> (String, String) {
    (
      super::defaults::os_version(ClientOs::Mac),
      super::defaults::software_version(ClientOs::Mac),
    )
  }
}
