#![cfg(windows)]
//! Run as administrator with GP_TEST_WINTUN_DLL pointing to the signed Wintun DLL.
use std::{ffi::c_void, io, net::IpAddr, os::windows::ffi::OsStrExt, path::PathBuf, ptr};
use windows_sys::{
  Win32::{
    Foundation::{FreeLibrary, HMODULE},
    NetworkManagement::Ndis::NET_LUID_LH,
    System::LibraryLoader::*,
  },
  core::GUID,
};
type CloseAdapter = unsafe extern "system" fn(*mut c_void);
type EndSession = unsafe extern "system" fn(*mut c_void);
struct Library(HMODULE);
impl Drop for Library {
  fn drop(&mut self) {
    unsafe {
      FreeLibrary(self.0);
    }
  }
}
struct Adapter {
  handle: *mut c_void,
  session: *mut c_void,
  close: CloseAdapter,
  end: EndSession,
  index: u32,
  _library: Library,
}
impl Adapter {
  fn create() -> Result<Self, Box<dyn std::error::Error>> {
    let path =
      PathBuf::from(std::env::var_os("GP_TEST_WINTUN_DLL").ok_or("GP_TEST_WINTUN_DLL is required")?).canonicalize()?;
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let module = unsafe {
      LoadLibraryExW(
        path.as_ptr(),
        ptr::null_mut(),
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
      )
    };
    if module.is_null() {
      return Err(io::Error::last_os_error().into());
    }
    let library = Library(module);
    // Wintun's public C ABI uses WINAPI and pointer-sized opaque handles.
    let create: unsafe extern "system" fn(*const u16, *const u16, *const GUID) -> *mut c_void = unsafe {
      std::mem::transmute(
        GetProcAddress(module, c"WintunCreateAdapter".as_ptr().cast()).ok_or("Missing WintunCreateAdapter")?,
      )
    };
    let close: CloseAdapter = unsafe {
      std::mem::transmute(
        GetProcAddress(module, c"WintunCloseAdapter".as_ptr().cast()).ok_or("Missing WintunCloseAdapter")?,
      )
    };
    let luid: unsafe extern "system" fn(*mut c_void, *mut NET_LUID_LH) = unsafe {
      std::mem::transmute(
        GetProcAddress(module, c"WintunGetAdapterLUID".as_ptr().cast()).ok_or("Missing WintunGetAdapterLUID")?,
      )
    };
    let start: unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void = unsafe {
      std::mem::transmute(
        GetProcAddress(module, c"WintunStartSession".as_ptr().cast()).ok_or("Missing WintunStartSession")?,
      )
    };
    let end: EndSession = unsafe {
      std::mem::transmute(
        GetProcAddress(module, c"WintunEndSession".as_ptr().cast()).ok_or("Missing WintunEndSession")?,
      )
    };
    let name: Vec<u16> = format!("GP Connect Test {}", uuid::Uuid::new_v4().simple())
      .encode_utf16()
      .chain(Some(0))
      .collect();
    let kind: Vec<u16> = "GP Connect".encode_utf16().chain(Some(0)).collect();
    let handle = unsafe { create(name.as_ptr(), kind.as_ptr(), ptr::null()) };
    if handle.is_null() {
      return Err(io::Error::last_os_error().into());
    }
    let mut adapter = Self {
      handle,
      session: ptr::null_mut(),
      close,
      end,
      index: 0,
      _library: library,
    };
    let mut id = NET_LUID_LH::default();
    unsafe {
      luid(handle, &mut id);
    }
    check(unsafe { ConvertInterfaceLuidToIndex(&id, &mut adapter.index) });
    adapter.session = unsafe { start(handle, 0x400000) };
    if adapter.session.is_null() {
      return Err(io::Error::last_os_error().into());
    }
    std::thread::sleep(std::time::Duration::from_secs(1));
    Ok(adapter)
  }
}
impl Drop for Adapter {
  fn drop(&mut self) {
    unsafe {
      if !self.session.is_null() {
        (self.end)(self.session);
      }
      (self.close)(self.handle);
    }
  }
}

use gpvpnc::windows::NetworkConfigurator;
use windows_sys::Win32::{NetworkManagement::IpHelper::*, Networking::WinSock::*};
fn check(status: u32) {
  assert_eq!(
    status,
    0,
    "Native IP Helper failed: {}",
    io::Error::from_raw_os_error(status as i32)
  );
}
fn address(value: &SOCKADDR_INET) -> IpAddr {
  unsafe {
    match value.si_family {
      AF_INET => std::net::Ipv4Addr::from(value.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes()).into(),
      AF_INET6 => std::net::Ipv6Addr::from(value.Ipv6.sin6_addr.u.Byte).into(),
      _ => panic!("unexpected family"),
    }
  }
}
fn routes(index: Option<u32>) -> Vec<String> {
  let mut result = Vec::new();
  unsafe {
    let mut table = std::ptr::null_mut();
    check(GetIpForwardTable2(AF_UNSPEC, &mut table));
    for row in std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize) {
      if Some(row.InterfaceIndex) != index
        && !["198.51.100.1", "203.0.113.128"].contains(&address(&row.DestinationPrefix.Prefix).to_string().as_str())
      {
        continue;
      }
      result.push(format!(
        "route {} {}/{} {} {}",
        row.InterfaceIndex,
        address(&row.DestinationPrefix.Prefix),
        row.DestinationPrefix.PrefixLength,
        address(&row.NextHop),
        row.Metric
      ));
    }
    FreeMibTable(table.cast());
  }
  result.sort();
  result
}
fn state(index: u32) -> Vec<String> {
  let mut result = routes(Some(index));
  unsafe {
    let mut table = std::ptr::null_mut();
    check(GetUnicastIpAddressTable(AF_UNSPEC, &mut table));
    for row in std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize) {
      if row.InterfaceIndex == index {
        result.push(format!("address {}/{}", address(&row.Address), row.OnLinkPrefixLength));
      }
    }
    FreeMibTable(table.cast());
    for family in [AF_INET, AF_INET6] {
      let mut row = MIB_IPINTERFACE_ROW {
        Family: family,
        InterfaceIndex: index,
        ..Default::default()
      };
      check(GetIpInterfaceEntry(&mut row));
      result.push(format!(
        "interface {} {} {} {} {}",
        family, row.Metric, row.UseAutomaticMetric, row.NlMtu, row.DadTransmits
      ));
    }
  }
  unsafe {
    let mut luid = NET_LUID_LH::default();
    let mut guid = GUID::default();
    check(ConvertInterfaceIndexToLuid(index, &mut luid));
    check(ConvertInterfaceLuidToGuid(&luid, &mut guid));
    let mut dns = DNS_INTERFACE_SETTINGS {
      Version: DNS_INTERFACE_SETTINGS_VERSION1,
      ..Default::default()
    };
    check(GetInterfaceDnsSettings(guid, &mut dns));
    for (name, pointer) in [
      ("dns-domain", dns.Domain),
      ("dns-search", dns.SearchList),
      ("dns-servers", dns.NameServer),
    ] {
      let mut length = 0;
      if !pointer.is_null() {
        while *pointer.add(length) != 0 {
          length += 1;
        }
      }
      let text = if pointer.is_null() {
        String::new()
      } else {
        String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
      };
      result.push(format!("{name} {text}"));
    }
    FreeInterfaceDnsSettings(&mut dns);
  }
  result.sort();
  result
}
fn best_route(destination: &str) -> MIB_IPFORWARD_ROW2 {
  let ip: IpAddr = destination.parse().unwrap();
  let mut destination = SOCKADDR_INET::default();
  match ip {
    IpAddr::V4(ip) => {
      destination.Ipv4.sin_family = AF_INET;
      destination.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
    }
    IpAddr::V6(ip) => {
      destination.Ipv6.sin6_family = AF_INET6;
      destination.Ipv6.sin6_addr.u.Byte = ip.octets();
    }
  }
  let mut route = MIB_IPFORWARD_ROW2::default();
  let mut source = SOCKADDR_INET::default();
  check(unsafe { GetBestRoute2(ptr::null(), 0, ptr::null(), &destination, 0, &mut route, &mut source) });
  route
}

fn assert_dns_policy(active: bool) {
  const CHECK: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$expected = [int]$env:GP_TEST_EXPECTED_DNS_POLICIES
$configured = @(Get-DnsClientNrptRule | Where-Object { $_.Namespace -contains '.gp-connect-test.invalid' })
$effective = @(Get-DnsClientNrptPolicy | Where-Object { $_.Namespace -contains '.gp-connect-test.invalid' })
if ($configured.Count -ne $expected -or $effective.Count -ne $expected) { throw 'Unexpected configured or effective VPN DNS policy' }
if ($expected) {
  $servers = @($configured[0].NameServers)
  if ($servers -notcontains '198.18.0.53' -or $servers -notcontains 'fd42:1235::53') { throw 'VPN DNS server families are missing' }
}
"#;
  let powershell =
    PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32/WindowsPowerShell/v1.0/powershell.exe");
  let output = std::process::Command::new(powershell)
    .args(["-NoProfile", "-NonInteractive", "-Command", CHECK])
    .env("GP_TEST_EXPECTED_DNS_POLICIES", if active { "1" } else { "0" })
    .output()
    .unwrap();
  assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
#[ignore = "requires administrator privileges and GP_TEST_WINTUN_DLL"]
fn installs_and_restores_native_network_configuration() -> Result<(), Box<dyn std::error::Error>> {
  let adapter = Adapter::create()?;
  let index = adapter.index;
  if let Some(recovery_key) = std::env::var_os("GP_TEST_CRASH_KEY") {
    let recovery_key = recovery_key.to_str().ok_or("Invalid registry key")?;
    let mut network = NetworkConfigurator::new(recovery_key)?;
    network.configure("connect", &environment(index))?;
    std::fs::write(
      std::env::var_os("GP_TEST_CRASH_READY").ok_or("Missing readiness path")?,
      index.to_string(),
    )?;
    loop {
      std::thread::park();
    }
  }
  let environment = environment(index);
  let recovery_key = format!(r"SOFTWARE\GP Connect\Tests\Network{}", uuid::Uuid::new_v4().simple());
  assert_dns_policy(false);
  let before = state(index);
  {
    let mut network = NetworkConfigurator::new(&recovery_key)?;
    network.configure("connect", &environment)?;
    let during = state(index);
    assert!(during.iter().any(|row| row == "address 192.0.2.2/24"));
    assert!(during.iter().any(|row| row == "address fd42:1234::2/64"));
    assert!(
      during
        .iter()
        .any(|row| row.starts_with(&format!("route {index} 203.0.113.0/24 ")))
    );
    assert!(
      during
        .iter()
        .any(|row| row.starts_with(&format!("route {index} 2001:db8:1234::/64 ")))
    );
    assert_eq!(best_route("203.0.113.1").InterfaceIndex, index);
    assert_eq!(best_route("2001:db8:1234::1").InterfaceIndex, index);
    assert_eq!(best_route("198.18.0.53").InterfaceIndex, index);
    assert_eq!(best_route("fd42:1235::53").InterfaceIndex, index);
    assert_ne!(best_route("203.0.113.200").InterfaceIndex, index);
    assert_ne!(best_route("198.51.100.1").InterfaceIndex, index);
    println!("PASS: IPv4/IPv6 addresses, split routes, resolver routes and MTU installed");
    assert_dns_policy(true);
    network.configure("disconnect", &[])?;
  }
  assert_eq!(before, state(index), "Network state was not restored");
  assert_dns_policy(false);
  println!("PASS: disconnect restored addresses, routes and interface settings");
  {
    let mut network = NetworkConfigurator::new(&recovery_key)?;
    network.configure("connect", &environment)?;
  }
  assert_eq!(
    before,
    state(index),
    "Dropping the connection did not restore network state"
  );
  assert_dns_policy(false);
  let mut invalid = environment.clone();
  invalid[0].1 = Some(u32::MAX.to_string());
  assert!(
    NetworkConfigurator::new(&recovery_key)?
      .configure("connect", &invalid)
      .is_err()
  );
  assert_eq!(before, state(index), "Failed connection left a physical gateway route");
  println!("PASS: partial setup failure rolled back its physical gateway route");
  drop(adapter);
  assert_crash_recovery(&recovery_key)?;
  winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE).delete_subkey(&recovery_key)?;
  Ok(())
}

fn environment(index: u32) -> Vec<(String, Option<String>)> {
  let pairs = [
    ("TUNIDX", index.to_string()),
    ("VPNGATEWAY", "198.51.100.1".into()),
    ("INTERNAL_IP4_ADDRESS", "192.0.2.2".into()),
    ("INTERNAL_IP4_NETMASKLEN", "24".into()),
    ("INTERNAL_IP6_ADDRESS", "fd42:1234::2".into()),
    ("INTERNAL_IP6_NETMASK", "fd42:1234::2/64".into()),
    ("INTERNAL_IP4_MTU", "1406".into()),
    ("CISCO_SPLIT_INC", "1".into()),
    ("CISCO_SPLIT_INC_0_ADDR", "203.0.113.0".into()),
    ("CISCO_SPLIT_INC_0_MASKLEN", "24".into()),
    ("CISCO_SPLIT_EXC", "1".into()),
    ("CISCO_SPLIT_EXC_0_ADDR", "203.0.113.128".into()),
    ("CISCO_SPLIT_EXC_0_MASKLEN", "25".into()),
    ("CISCO_IPV6_SPLIT_INC", "1".into()),
    ("CISCO_IPV6_SPLIT_INC_0_ADDR", "2001:db8:1234::".into()),
    ("CISCO_IPV6_SPLIT_INC_0_MASKLEN", "64".into()),
    ("INTERNAL_IP4_DNS", "198.18.0.53 fd42:1235::53".into()),
    ("CISCO_DEF_DOMAIN", "gp-connect-test.invalid".into()),
    ("CISCO_SPLIT_DNS", "gp-connect-test.invalid".into()),
  ];
  pairs
    .into_iter()
    .map(|(key, value)| (key.to_string(), Some(value)))
    .collect()
}

struct CrashChild {
  child: std::process::Child,
  key: String,
  modified_route: Option<MIB_IPFORWARD_ROW2>,
}
impl Drop for CrashChild {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    if let Some(route) = self.modified_route.take() {
      unsafe {
        DeleteIpForwardEntry2(&route);
      }
    }
    let _ = gpvpnc::windows::recover(&self.key);
  }
}
fn assert_crash_recovery(key: &str) -> Result<(), Box<dyn std::error::Error>> {
  let before = routes(None);
  let directory = tempfile::tempdir()?;
  let ready = directory.path().join("ready");
  let child = std::process::Command::new(std::env::current_exe()?)
    .args([
      "--ignored",
      "--exact",
      "installs_and_restores_native_network_configuration",
    ])
    .env("GP_TEST_CRASH_KEY", key)
    .env("GP_TEST_CRASH_READY", &ready)
    .stdout(std::process::Stdio::null())
    .spawn()?;
  let mut child = CrashChild {
    child,
    key: key.into(),
    modified_route: None,
  };
  let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
  while !ready.exists() {
    if child.child.try_wait()?.is_some() || std::time::Instant::now() > deadline {
      return Err("Crash child did not install its network state".into());
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
  }
  let child_index: u32 = std::fs::read_to_string(&ready)?.parse()?;
  assert_dns_policy(true);
  let dirty = routes(None);
  assert_ne!(before, dirty, "Child did not create physical routes");
  child.child.kill()?;
  child.child.wait()?;
  assert_dns_policy(true);
  let mut addresses = ptr::null_mut();
  check(unsafe { GetUnicastIpAddressTable(AF_UNSPEC, &mut addresses) });
  let child_addresses = unsafe {
    std::slice::from_raw_parts((*addresses).Table.as_ptr(), (*addresses).NumEntries as usize)
      .iter()
      .any(|row| row.InterfaceIndex == child_index)
  };
  unsafe {
    FreeMibTable(addresses.cast());
  }
  assert!(!child_addresses, "Terminated process left its Wintun addresses");
  assert_ne!(
    before,
    routes(None),
    "Physical routes unexpectedly disappeared with the process"
  );
  // Model an administrator taking ownership of an exclusion after the crash.
  let mut modified = best_route("203.0.113.200");
  assert_eq!(address(&modified.DestinationPrefix.Prefix).to_string(), "203.0.113.128");
  modified.Metric += 1;
  check(unsafe { SetIpForwardEntry2(&modified) });
  child.modified_route = Some(modified);
  let mut expected = before.clone();
  expected.push(format!(
    "route {} {}/{} {} {}",
    modified.InterfaceIndex,
    address(&modified.DestinationPrefix.Prefix),
    modified.DestinationPrefix.PrefixLength,
    address(&modified.NextHop),
    modified.Metric
  ));
  expected.sort();
  gpvpnc::windows::recover(key)?;
  assert_eq!(
    expected,
    routes(None),
    "Recovery must remove owned routes and preserve the changed route"
  );
  assert_dns_policy(false);
  drop(NetworkConfigurator::new(key)?);
  check(unsafe { DeleteIpForwardEntry2(&modified) });
  child.modified_route = None;
  assert_eq!(before, routes(None), "Crash recovery did not restore the baseline");
  println!("PASS: crash recovery removed owned physical routes and DNS policy and preserved an administrator's change");
  Ok(())
}
