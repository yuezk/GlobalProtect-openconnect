//! Native IP Helper changes with ownership of only the rows this connection creates.
use super::{
  configuration::Configuration,
  recovery::{Journal, Route},
};
use ipnet::IpNet;
use std::{
  io,
  net::{IpAddr, Ipv4Addr, Ipv6Addr},
  ptr,
};
use windows_sys::Win32::{
  Foundation::{ERROR_NOT_FOUND, ERROR_OBJECT_ALREADY_EXISTS},
  NetworkManagement::IpHelper::*,
  Networking::WinSock::*,
};

struct InstalledRoute {
  row: MIB_IPFORWARD_ROW2,
  record: Option<String>,
}
pub(super) struct Installed {
  routes: Vec<InstalledRoute>,
  journal: Journal,
  addresses: Vec<MIB_UNICASTIPADDRESS_ROW>,
  interfaces: Vec<MIB_IPINTERFACE_ROW>,
}
fn check(status: u32) -> io::Result<()> {
  if status == 0 {
    Ok(())
  } else {
    Err(io::Error::from_raw_os_error(status as i32))
  }
}
fn socket_address(address: IpAddr) -> SOCKADDR_INET {
  let mut result = SOCKADDR_INET::default();
  match address {
    IpAddr::V4(address) => {
      result.Ipv4.sin_family = AF_INET;
      result.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(address.octets());
    }
    IpAddr::V6(address) => {
      result.Ipv6.sin6_family = AF_INET6;
      result.Ipv6.sin6_addr.u.Byte = address.octets();
    }
  }
  result
}
fn empty_address(ipv4: bool) -> SOCKADDR_INET {
  socket_address(if ipv4 {
    Ipv4Addr::UNSPECIFIED.into()
  } else {
    Ipv6Addr::UNSPECIFIED.into()
  })
}
fn route(prefix: IpNet, interface: u32) -> MIB_IPFORWARD_ROW2 {
  let mut storage = std::mem::MaybeUninit::<MIB_IPFORWARD_ROW2>::zeroed();
  // IP Helper initializes BOOLEAN fields to 0xff. They must contain canonical
  // Rust bool values before this storage can become an initialized Rust row.
  let mut row = unsafe {
    let pointer = storage.as_mut_ptr();
    InitializeIpForwardEntry(pointer);
    std::ptr::addr_of_mut!((*pointer).Loopback).write(false);
    std::ptr::addr_of_mut!((*pointer).AutoconfigureAddress).write(false);
    std::ptr::addr_of_mut!((*pointer).Publish).write(false);
    std::ptr::addr_of_mut!((*pointer).Immortal).write(true);
    storage.assume_init()
  };
  row.InterfaceIndex = interface;
  row.DestinationPrefix.Prefix = socket_address(prefix.network());
  row.DestinationPrefix.PrefixLength = prefix.prefix_len();
  row.NextHop = empty_address(prefix.addr().is_ipv4());
  row.SitePrefixLength = 0;
  row.Metric = 0;
  row.Protocol = MIB_IPPROTO_NETMGMT;
  row
}
fn physical_route(prefix: IpNet, tunnel: u32) -> io::Result<MIB_IPFORWARD_ROW2> {
  let destination = socket_address(prefix.addr());
  let mut existing = MIB_IPFORWARD_ROW2::default();
  let mut source = SOCKADDR_INET::default();
  check(unsafe { GetBestRoute2(ptr::null(), 0, ptr::null(), &destination, 0, &mut existing, &mut source) })?;
  if existing.InterfaceIndex == tunnel {
    return Err(io::Error::other(
      "The VPN gateway or exclusion resolves through the VPN adapter",
    ));
  }
  let mut row = route(prefix, existing.InterfaceIndex);
  row.InterfaceLuid = existing.InterfaceLuid;
  row.NextHop = existing.NextHop;
  row.Metric = existing.Metric;
  Ok(row)
}
impl Installed {
  pub fn apply(configuration: &Configuration, recovery_key: &str) -> io::Result<Self> {
    // Resolve all physical paths before introducing any tunnel routes.
    let gateway = IpNet::new(
      configuration.gateway,
      if configuration.gateway.is_ipv4() { 32 } else { 128 },
    )
    .map_err(io::Error::other)?;
    let physical = std::iter::once(gateway)
      .chain(configuration.excludes.iter().copied())
      .map(|prefix| physical_route(prefix, configuration.interface))
      .collect::<io::Result<Vec<_>>>()?;
    let mut installed = Self {
      routes: Vec::new(),
      addresses: Vec::new(),
      interfaces: Vec::new(),
      journal: Journal::begin(recovery_key)?,
    };
    for row in physical {
      installed.add_route(row, true)?;
    }
    for family in [AF_INET, AF_INET6] {
      if !configuration
        .addresses
        .iter()
        .any(|address| address.addr().is_ipv4() == (family == AF_INET))
      {
        continue;
      }
      let mut row = MIB_IPINTERFACE_ROW {
        Family: family,
        InterfaceIndex: configuration.interface,
        ..Default::default()
      };
      check(unsafe { GetIpInterfaceEntry(&mut row) })?;
      installed.interfaces.push(row);
      row.UseAutomaticMetric = false;
      row.Metric = 1;
      row.DadTransmits = 0;
      row.RouterDiscoveryBehavior = RouterDiscoveryDisabled;
      if let Some(mtu) = configuration.mtu {
        row.NlMtu = mtu;
      }
      if family == AF_INET {
        row.SitePrefixLength = 0;
      }
      check(unsafe { SetIpInterfaceEntry(&mut row) })?;
    }
    for address in &configuration.addresses {
      let mut row = MIB_UNICASTIPADDRESS_ROW::default();
      unsafe {
        InitializeUnicastIpAddressEntry(&mut row);
      }
      row.InterfaceIndex = configuration.interface;
      row.Address = socket_address(address.addr());
      row.OnLinkPrefixLength = address.prefix_len();
      row.PrefixOrigin = IpPrefixOriginManual;
      row.SuffixOrigin = IpSuffixOriginManual;
      row.DadState = IpDadStatePreferred;
      let status = unsafe { CreateUnicastIpAddressEntry(&row) };
      if status != ERROR_OBJECT_ALREADY_EXISTS {
        check(status)?;
        installed.addresses.push(row);
      }
    }
    for prefix in &configuration.includes {
      installed.add_route(route(*prefix, configuration.interface), false)?;
    }
    for server in &configuration.servers {
      if *server == configuration.gateway {
        continue;
      }
      let prefix = IpNet::new(*server, if server.is_ipv4() { 32 } else { 128 }).map_err(io::Error::other)?;
      installed.add_route(route(prefix, configuration.interface), false)?;
    }
    Ok(installed)
  }
  fn add_route(&mut self, row: MIB_IPFORWARD_ROW2, physical: bool) -> io::Result<()> {
    let status = unsafe { CreateIpForwardEntry2(&row) };
    if status != ERROR_OBJECT_ALREADY_EXISTS {
      check(status)?;
      // Retain rollback ownership even if writing the registry record fails.
      self.routes.push(InstalledRoute { row, record: None });
      if physical {
        let name = self.journal.record(&Route::from_row(&row)?)?;
        self.routes.last_mut().expect("route was just installed").record = Some(name);
      }
    }
    Ok(())
  }
  pub fn restore(&mut self) -> io::Result<()> {
    let mut failure = None;
    for route in self.routes.drain(..).rev() {
      match remove_owned_route(&route.row) {
        Ok(()) => {
          if let Some(name) = route.record {
            if let Err(error) = self.journal.forget(&name) {
              log::warn!("Could not clear Windows route ownership: {error}");
              if failure.is_none() {
                failure = Some(error);
              }
            }
          }
        }
        Err(error) => {
          log::warn!("Could not remove owned Windows route: {error}");
          if failure.is_none() {
            failure = Some(error);
          }
        }
      }
    }
    let mut record = |status| {
      if status != ERROR_NOT_FOUND && status != 0 {
        let error = io::Error::from_raw_os_error(status as i32);
        log::warn!("Could not restore VPN network configuration: {error}");
        if failure.is_none() {
          failure = Some(error);
        }
      }
    };
    for row in self.addresses.drain(..).rev() {
      record(unsafe { DeleteUnicastIpAddressEntry(&row) });
    }
    for original in self.interfaces.drain(..).rev() {
      let mut current = MIB_IPINTERFACE_ROW {
        Family: original.Family,
        InterfaceIndex: original.InterfaceIndex,
        ..Default::default()
      };
      let status = unsafe { GetIpInterfaceEntry(&mut current) };
      if status != 0 {
        record(status);
        continue;
      }
      current.UseAutomaticMetric = original.UseAutomaticMetric;
      current.Metric = original.Metric;
      current.NlMtu = original.NlMtu;
      current.DadTransmits = original.DadTransmits;
      current.RouterDiscoveryBehavior = original.RouterDiscoveryBehavior;
      if current.Family == AF_INET {
        current.SitePrefixLength = 0;
      }
      record(unsafe { SetIpInterfaceEntry(&mut current) });
    }
    failure.map_or(Ok(()), Err)
  }
}
impl Drop for Installed {
  fn drop(&mut self) {
    let _ = self.restore();
  }
}

fn address(value: &SOCKADDR_INET) -> io::Result<IpAddr> {
  unsafe {
    match value.si_family {
      AF_INET => Ok(Ipv4Addr::from(value.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes()).into()),
      AF_INET6 => Ok(Ipv6Addr::from(value.Ipv6.sin6_addr.u.Byte).into()),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid route address family",
      )),
    }
  }
}
fn scope(value: &SOCKADDR_INET) -> u32 {
  unsafe {
    if value.si_family == AF_INET6 {
      value.Ipv6.Anonymous.sin6_scope_id
    } else {
      0
    }
  }
}
impl Route {
  fn from_row(row: &MIB_IPFORWARD_ROW2) -> io::Result<Self> {
    Ok(Self {
      prefix: IpNet::new(
        address(&row.DestinationPrefix.Prefix)?,
        row.DestinationPrefix.PrefixLength,
      )
      .map_err(io::Error::other)?,
      next_hop: address(&row.NextHop)?,
      interface_luid: unsafe { row.InterfaceLuid.Value },
      metric: row.Metric,
      prefix_scope: scope(&row.DestinationPrefix.Prefix),
      next_hop_scope: scope(&row.NextHop),
    })
  }
  fn row(&self) -> io::Result<MIB_IPFORWARD_ROW2> {
    if self.interface_luid == 0 || self.prefix.addr().is_ipv4() != self.next_hop.is_ipv4() {
      return Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Invalid saved Windows route",
      ));
    }
    let mut row = route(self.prefix, 0);
    row.InterfaceLuid.Value = self.interface_luid;
    row.Metric = self.metric;
    row.NextHop = socket_address(self.next_hop);
    if self.prefix.addr().is_ipv6() {
      row.DestinationPrefix.Prefix.Ipv6.Anonymous.sin6_scope_id = self.prefix_scope;
      row.NextHop.Ipv6.Anonymous.sin6_scope_id = self.next_hop_scope;
    }
    Ok(row)
  }
}
fn remove_owned_route(row: &MIB_IPFORWARD_ROW2) -> io::Result<()> {
  let mut current = *row;
  let status = unsafe { GetIpForwardEntry2(&mut current) };
  if status == ERROR_NOT_FOUND {
    return Ok(());
  }
  check(status)?;
  if current.Protocol != row.Protocol || current.Metric != row.Metric {
    log::warn!("Preserving a Windows route changed after VPN installation");
    return Ok(());
  }
  check(unsafe { DeleteIpForwardEntry2(&current) })
}
pub(super) fn recover_route(saved: &Route) -> io::Result<()> {
  remove_owned_route(&saved.row()?)
}
