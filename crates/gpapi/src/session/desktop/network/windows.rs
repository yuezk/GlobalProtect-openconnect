//! Windows network identity from IP Helper and adapter DNS configuration.
use super::*;
use std::os::windows::io::AsRawSocket;
use windows_sys::Win32::{
  NetworkManagement::IpHelper::{
    DNS_INTERFACE_SETTINGS, DNS_INTERFACE_SETTINGS_VERSION1, FreeInterfaceDnsSettings, FreeMibTable,
    GetInterfaceDnsSettings, GetIpForwardTable2,
  },
  Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, IP_UNICAST_IF, IPPROTO_IP, IPPROTO_IPV6, IPV6_UNICAST_IF, SOCKADDR_INET,
    SOCKET_ERROR, WSAGetLastError, setsockopt,
  },
};

pub(super) fn scope_resolver_socket(
  socket: &tokio::net::UdpSocket,
  interface: &Interface,
  endpoint: SocketAddr,
) -> anyhow::Result<()> {
  ensure!(interface.index != 0, "Resolver interface index is missing");
  // IP_UNICAST_IF uses network byte order; IPV6_UNICAST_IF uses host byte order.
  let (level, option, index) = if endpoint.is_ipv4() {
    (IPPROTO_IP, IP_UNICAST_IF, interface.index.to_be())
  } else {
    (IPPROTO_IPV6, IPV6_UNICAST_IF, interface.index)
  };
  let result = unsafe {
    setsockopt(
      socket.as_raw_socket() as usize,
      level,
      option,
      (&index as *const u32).cast(),
      std::mem::size_of::<u32>() as i32,
    )
  };
  if result == SOCKET_ERROR {
    return Err(io::Error::from_raw_os_error(unsafe { WSAGetLastError() }).into());
  }
  Ok(())
}

pub(super) fn routes(control: &dyn CollectionControl) -> anyhow::Result<Vec<RouteContext>> {
  control.check()?;
  let mut table = std::ptr::null_mut();
  let status = unsafe { GetIpForwardTable2(AF_UNSPEC, &mut table) };
  if status != 0 {
    return Err(io::Error::from_raw_os_error(status as i32).into());
  }
  let interfaces = netdev::get_interfaces();
  let result = unsafe {
    let rows = std::slice::from_raw_parts((*table).Table.as_ptr(), (*table).NumEntries as usize);
    rows
      .iter()
      .filter_map(|row| {
        let destination = address(&row.DestinationPrefix.Prefix)?;
        let gateway = address(&row.NextHop)?;
        let interface = interfaces
          .iter()
          .find(|interface| interface.index == row.InterfaceIndex)?;
        Some(RouteContext {
          destination: format!("{destination}/{}", row.DestinationPrefix.PrefixLength),
          gateway: gateway.to_string(),
          interface: interface.name.clone(),
          table: "main".into(),
          metric: row.Metric.to_string(),
        })
      })
      .collect()
  };
  unsafe {
    FreeMibTable(table.cast());
  }
  control.check()?;
  Ok(result)
}

unsafe fn address(address: &SOCKADDR_INET) -> Option<IpAddr> {
  unsafe {
    match address.si_family {
      AF_INET => Some(std::net::Ipv4Addr::from(address.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes()).into()),
      AF_INET6 => Some(std::net::Ipv6Addr::from(address.Ipv6.sin6_addr.u.Byte).into()),
      _ => None,
    }
  }
}

pub(super) fn resolvers(control: &dyn CollectionControl) -> anyhow::Result<Vec<ResolverContext>> {
  control.check()?;
  let mut resolvers = netdev::get_interfaces()
    .into_iter()
    .filter(|interface| !interface.dns_servers.is_empty())
    .map(|interface| {
      Ok(ResolverContext {
        interface: Some(interface.name.clone()),
        servers: interface.dns_servers.iter().map(ToString::to_string).collect(),
        domains: resolver_domains(&interface.name)?,
      })
    })
    .collect::<anyhow::Result<Vec<_>>>()?;
  resolvers.sort();
  control.check()?;
  Ok(resolvers)
}

fn resolver_domains(interface: &str) -> anyhow::Result<Vec<String>> {
  let id = uuid::Uuid::parse_str(interface.trim_matches(['{', '}']))?;
  let (data1, data2, data3, data4) = id.as_fields();
  let guid = windows_sys::core::GUID {
    data1,
    data2,
    data3,
    data4: *data4,
  };
  let mut settings = DNS_INTERFACE_SETTINGS {
    Version: DNS_INTERFACE_SETTINGS_VERSION1,
    ..Default::default()
  };
  let status = unsafe { GetInterfaceDnsSettings(guid, &mut settings) };
  if status != 0 {
    return Err(io::Error::from_raw_os_error(status as i32).into());
  }
  let mut domains = Vec::new();
  for pointer in [settings.Domain, settings.SearchList] {
    if pointer.is_null() {
      continue;
    }
    let text = unsafe {
      let mut length = 0;
      while *pointer.add(length) != 0 {
        length += 1;
      }
      String::from_utf16_lossy(std::slice::from_raw_parts(pointer, length))
    };
    for domain in text
      .split(|ch: char| ch == ',' || ch.is_whitespace())
      .filter(|domain| !domain.is_empty())
    {
      let domain = domain.to_ascii_lowercase();
      if !domains.contains(&domain) {
        domains.push(domain);
      }
    }
  }
  unsafe {
    FreeInterfaceDnsSettings(&mut settings);
  }
  Ok(domains)
}
