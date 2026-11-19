//! Adapter DNS settings and Windows name-resolution policy scoped to this connection.
use super::{SystemTunnel, configuration::Configuration};
use base64::Engine;
use serde_json::json;
use std::{
  io::{self, Write},
  os::windows::process::CommandExt,
  process::{Command, Stdio},
};
use windows_sys::{
  Win32::{
    NetworkManagement::{IpHelper::*, Ndis::NET_LUID_LH},
    System::Threading::CREATE_NO_WINDOW,
  },
  core::GUID,
};

struct Snapshot {
  domain: String,
  search: String,
}
pub(super) struct Installed {
  guid: GUID,
  snapshot: Option<Snapshot>,
  tag: Option<String>,
  tunnel: Option<SystemTunnel>,
}
fn check(status: u32) -> io::Result<()> {
  if status == 0 {
    Ok(())
  } else {
    Err(io::Error::from_raw_os_error(status as i32))
  }
}
fn wide(text: &str) -> Vec<u16> {
  text.encode_utf16().chain(Some(0)).collect()
}
unsafe fn string(pointer: *const u16) -> String {
  if pointer.is_null() {
    return String::new();
  }
  unsafe {
    let mut length = 0;
    while *pointer.add(length) != 0 {
      length += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
  }
}
impl Snapshot {
  fn capture(guid: GUID) -> io::Result<Self> {
    let mut settings = DNS_INTERFACE_SETTINGS {
      Version: DNS_INTERFACE_SETTINGS_VERSION1,
      ..Default::default()
    };
    check(unsafe { GetInterfaceDnsSettings(guid, &mut settings) })?;
    let snapshot = unsafe {
      Self {
        domain: string(settings.Domain),
        search: string(settings.SearchList),
      }
    };
    unsafe {
      FreeInterfaceDnsSettings(&mut settings);
    }
    Ok(snapshot)
  }
  fn write(&self, guid: GUID) -> io::Result<()> {
    let (mut domain, mut search) = (wide(&self.domain), wide(&self.search));
    let settings = DNS_INTERFACE_SETTINGS {
      Version: DNS_INTERFACE_SETTINGS_VERSION1,
      Flags: (DNS_SETTING_DOMAIN | DNS_SETTING_SEARCHLIST) as u64,
      Domain: domain.as_mut_ptr(),
      SearchList: search.as_mut_ptr(),
      ..Default::default()
    };
    check(unsafe { SetInterfaceDnsSettings(guid, &settings) })
  }
}
impl Installed {
  pub fn apply(configuration: &Configuration, recovery_key: &str, tunnel: Option<&SystemTunnel>) -> io::Result<Self> {
    let mut luid = NET_LUID_LH::default();
    let mut guid = GUID::default();
    check(unsafe { ConvertInterfaceIndexToLuid(configuration.interface, &mut luid) })?;
    check(unsafe { ConvertInterfaceLuidToGuid(&luid, &mut guid) })?;
    let mut installed = Self {
      guid,
      snapshot: Some(Snapshot::capture(guid)?),
      tag: None,
      tunnel: tunnel.cloned(),
    };
    Snapshot {
      domain: configuration.domains.first().cloned().unwrap_or_default(),
      search: configuration.domains.join(","),
    }
    .write(guid)?;
    // NRPT selects both IPv4 and IPv6 VPN resolvers. Split namespaces leave
    // unrelated DNS on the physical network; "." covers full-tunnel DNS.
    // Existing static/DHCP adapter nameserver configuration stays untouched.
    if !configuration.servers.is_empty() {
      let namespaces: Vec<String> = if configuration.split_dns.is_empty() {
        vec![".".into()]
      } else {
        configuration
          .split_dns
          .iter()
          .map(|domain| format!(".{}", domain.trim_matches('.')))
          .collect()
      };
      let tag = match tunnel {
        Some(_) => format!("GPConnect:{}", uuid::Uuid::new_v4()),
        None => journal_tag(recovery_key),
      };
      installed.tag = Some(tag.clone());
      policy(
        tunnel,
        json!({"operation":"add", "tag":tag, "namespaces":namespaces, "servers":configuration.servers.iter().map(ToString::to_string).collect::<Vec<_>>()}),
      )?;
    }
    Ok(installed)
  }
  pub fn restore(&mut self) -> io::Result<()> {
    let mut failure = None;
    if let Some(tag) = self.tag.take() {
      if let Err(error) = policy(self.tunnel.as_ref(), json!({"operation":"remove", "tag":tag})) {
        log::warn!("Could not remove VPN DNS policy: {error}");
        failure = Some(error);
      }
    }
    if let Some(snapshot) = self.snapshot.take() {
      if let Err(error) = snapshot.write(self.guid) {
        log::warn!("Could not restore VPN adapter DNS: {error}");
        if failure.is_none() {
          failure = Some(error);
        }
      }
    }
    failure.map_or(Ok(()), Err)
  }
}
impl Drop for Installed {
  fn drop(&mut self) {
    let _ = self.restore();
  }
}

/// A crashed service cannot run Drop. Remove only GP Connect's tagged policies
/// before accepting a new connection; adapter rows disappear with Wintun itself.
pub(super) fn clear_stale_policies(tunnel: &SystemTunnel) -> io::Result<()> {
  policy(Some(tunnel), json!({"operation":"clear-stale"}))
}
// Keep isolated journals outside the GPConnect:* namespace used by system recovery.
fn journal_tag(recovery_key: &str) -> String {
  format!("GPConnectJournal:{recovery_key}")
}

pub(super) fn remove_journal_policy(recovery_key: &str) -> io::Result<()> {
  policy(None, json!({"operation":"remove", "tag":journal_tag(recovery_key)}))
}

fn policy(tunnel: Option<&SystemTunnel>, request: serde_json::Value) -> io::Result<()> {
  const SCRIPT: &str = include_str!("dns-policy.ps1");
  let encoded = base64::engine::general_purpose::STANDARD
    .encode(SCRIPT.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>());
  let root =
    std::env::var_os("SystemRoot").ok_or_else(|| io::Error::other("Windows system directory is unavailable"))?;
  let executable = std::path::PathBuf::from(root).join("System32/WindowsPowerShell/v1.0/powershell.exe");
  // The script produces no standard output. Use that handle to give the worker
  // a copy of the lease so it keeps ownership if the tunnel process dies.
  let stdout = match tunnel {
    Some(tunnel) => Stdio::from(tunnel.duplicate_lock()?),
    None => Stdio::null(),
  };
  let mut child = Command::new(executable)
    .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded])
    .creation_flags(CREATE_NO_WINDOW)
    .stdin(Stdio::piped())
    .stdout(stdout)
    .stderr(Stdio::piped())
    .spawn()?;
  let input = child
    .stdin
    .take()
    .expect("stdin is piped")
    .write_all(request.to_string().as_bytes());
  if let Err(error) = input {
    let _ = child.kill();
    let _ = child.wait();
    return Err(error);
  }
  let output = child.wait_with_output()?;
  if !output.status.success() {
    return Err(io::Error::other(format!(
      "Windows DNS policy failed: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    )));
  }
  Ok(())
}
