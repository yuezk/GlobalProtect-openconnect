//! Convert OpenConnect's normal configuration callback into a Windows network plan.
use ipnet::IpNet;
use std::{collections::HashMap, io, net::IpAddr};

#[derive(Debug)]
pub(super) struct Configuration {
  pub interface: u32,
  pub gateway: IpAddr,
  pub addresses: Vec<IpNet>,
  pub mtu: Option<u32>,
  pub includes: Vec<IpNet>,
  pub excludes: Vec<IpNet>,
  pub servers: Vec<IpAddr>,
  pub domains: Vec<String>,
  pub split_dns: Vec<String>,
}

fn invalid(message: impl Into<String>) -> io::Error {
  io::Error::new(io::ErrorKind::InvalidData, message.into())
}
impl Configuration {
  pub fn parse(environment: &[(String, Option<String>)]) -> io::Result<Self> {
    let values: HashMap<_, _> = environment
      .iter()
      .filter_map(|(key, value)| value.as_deref().map(|value| (key.as_str(), value)))
      .collect();
    let required = |key: &str| {
      values
        .get(key)
        .copied()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid(format!("Missing VPN configuration: {key}")))
    };
    let interface: u32 = required("TUNIDX")?
      .parse()
      .map_err(|_| invalid("Invalid VPN interface index"))?;
    if interface == 0 {
      return Err(invalid("VPN interface index is zero"));
    }
    let gateway = required("VPNGATEWAY")?
      .parse()
      .map_err(|_| invalid("Invalid VPN gateway address"))?;
    let mut addresses = Vec::new();
    if let Some(address) = values.get("INTERNAL_IP4_ADDRESS") {
      let prefix = values.get("INTERNAL_IP4_NETMASKLEN").copied().unwrap_or("32");
      let network: IpNet = format!("{address}/{prefix}")
        .parse()
        .map_err(|_| invalid("Invalid VPN IPv4 address or prefix"))?;
      if !matches!(network, IpNet::V4(_)) {
        return Err(invalid("VPN IPv4 address has the wrong family"));
      }
      addresses.push(network);
    }
    if let Some(address) = values.get("INTERNAL_IP6_ADDRESS") {
      let prefix = values
        .get("INTERNAL_IP6_NETMASK")
        .and_then(|value| value.rsplit_once('/'))
        .map(|(_, prefix)| prefix)
        .unwrap_or("128");
      let network: IpNet = format!("{address}/{prefix}")
        .parse()
        .map_err(|_| invalid("Invalid VPN IPv6 address or prefix"))?;
      if !matches!(network, IpNet::V6(_)) {
        return Err(invalid("VPN IPv6 address has the wrong family"));
      }
      addresses.push(network);
    }
    if addresses.is_empty() {
      return Err(invalid("The VPN did not assign an IP address"));
    }
    let mtu = values
      .get("INTERNAL_IP4_MTU")
      .filter(|value| !value.is_empty())
      .map(|value| value.parse::<u32>().map_err(|_| invalid("Invalid VPN MTU")))
      .transpose()?
      .filter(|mtu| *mtu != 0);
    let mut includes = Vec::new();
    let mut excludes = Vec::new();
    for (family, prefix) in [(4, "CISCO_SPLIT"), (6, "CISCO_IPV6_SPLIT")] {
      if !addresses
        .iter()
        .any(|address| address.addr().is_ipv4() == (family == 4))
      {
        continue;
      }
      let included = routes(&values, &format!("{prefix}_INC"), family)?;
      if included.is_empty() {
        // Preserve the physical default route, using the two more-specific halves.
        let defaults: &[&str] = if family == 4 {
          &["0.0.0.0/1", "128.0.0.0/1"]
        } else {
          &["::/1", "8000::/1"]
        };
        includes.extend(defaults.iter().map(|route| route.parse::<IpNet>().unwrap()));
      } else {
        includes.extend(included);
      }
      excludes.extend(routes(&values, &format!("{prefix}_EXC"), family)?);
    }
    let mut servers = values
      .get("INTERNAL_IP4_DNS")
      .into_iter()
      .flat_map(|text| text.split_whitespace())
      .map(|address| address.parse().map_err(|_| invalid("Invalid VPN DNS server")))
      .collect::<io::Result<Vec<IpAddr>>>()?;
    let advertised_dns = !servers.is_empty();
    servers.retain(|server| {
      addresses
        .iter()
        .any(|address| address.addr().is_ipv4() == server.is_ipv4())
    });
    if advertised_dns && servers.is_empty() {
      return Err(invalid("No VPN DNS server matches an assigned address family"));
    }
    let domains = names(values.get("CISCO_DEF_DOMAIN").copied().unwrap_or(""));
    let split_dns = names(values.get("CISCO_SPLIT_DNS").copied().unwrap_or(""));
    Ok(Self {
      interface,
      gateway,
      addresses,
      mtu,
      includes,
      excludes,
      servers,
      domains,
      split_dns,
    })
  }
}
fn names(text: &str) -> Vec<String> {
  text
    .split(|ch: char| ch == ',' || ch.is_whitespace())
    .filter(|part| !part.is_empty())
    .map(str::to_owned)
    .collect()
}
fn routes(values: &HashMap<&str, &str>, key: &str, family: u8) -> io::Result<Vec<IpNet>> {
  let count = values
    .get(key)
    .map(|value| {
      value
        .parse::<u32>()
        .map_err(|_| invalid(format!("Invalid {key} count")))
    })
    .transpose()?
    .unwrap_or(0);
  (0..count)
    .map(|index| {
      let address = values
        .get(format!("{key}_{index}_ADDR").as_str())
        .ok_or_else(|| invalid(format!("Missing {key} address")))?;
      let prefix = values
        .get(format!("{key}_{index}_MASKLEN").as_str())
        .ok_or_else(|| invalid(format!("Missing {key} prefix")))?;
      let route: IpNet = format!("{address}/{prefix}")
        .parse()
        .map_err(|_| invalid(format!("Invalid {key} route")))?;
      if route.addr().is_ipv4() != (family == 4) {
        return Err(invalid(format!("Wrong {key} route family")));
      }
      Ok(route.trunc())
    })
    .collect()
}

#[cfg(test)]
mod tests {
  use super::*;
  fn environment(extra: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
    [
      ("TUNIDX", "16"),
      ("VPNGATEWAY", "198.51.100.1"),
      ("INTERNAL_IP4_ADDRESS", "10.20.30.4"),
    ]
    .into_iter()
    .chain(extra.iter().copied())
    .map(|(key, value)| (key.into(), Some(value.into())))
    .collect()
  }
  #[test]
  fn full_tunnel_keeps_assigned_addresses_and_routes_both_families() {
    let plan = Configuration::parse(&environment(&[
      ("INTERNAL_IP4_NETMASKLEN", "24"),
      ("INTERNAL_IP6_ADDRESS", "fd00::4"),
      ("INTERNAL_IP6_NETMASK", "fd00::4/64"),
      ("INTERNAL_IP4_MTU", "1406"),
      ("INTERNAL_IP4_DNS", "10.20.30.1 fd00::1"),
    ]))
    .unwrap();
    assert_eq!(
      plan.addresses.iter().map(ToString::to_string).collect::<Vec<_>>(),
      ["10.20.30.4/24", "fd00::4/64"]
    );
    assert_eq!(plan.includes.len(), 4);
    assert_eq!(plan.servers.len(), 2);
    assert_eq!(plan.mtu, Some(1406));
    assert_eq!(plan.interface, 16);
    assert_eq!(plan.gateway.to_string(), "198.51.100.1");
  }
  #[test]
  fn split_routes_and_dns_preserve_gateway_configuration() {
    let plan = Configuration::parse(&environment(&[
      ("CISCO_SPLIT_INC", "1"),
      ("CISCO_SPLIT_INC_0_ADDR", "172.16.5.9"),
      ("CISCO_SPLIT_INC_0_MASKLEN", "16"),
      ("CISCO_SPLIT_EXC", "1"),
      ("CISCO_SPLIT_EXC_0_ADDR", "172.16.10.0"),
      ("CISCO_SPLIT_EXC_0_MASKLEN", "24"),
      ("CISCO_DEF_DOMAIN", "corp.example"),
      ("CISCO_SPLIT_DNS", "corp.example, internal.example"),
    ]))
    .unwrap();
    assert_eq!(
      plan.includes.iter().map(ToString::to_string).collect::<Vec<_>>(),
      ["172.16.0.0/16"]
    );
    assert_eq!(plan.excludes[0].to_string(), "172.16.10.0/24");
    assert_eq!(plan.domains, ["corp.example"]);
    assert_eq!(plan.split_dns, ["corp.example", "internal.example"]);
  }
  #[test]
  fn unassigned_address_family_does_not_install_routes_or_resolvers() {
    let plan = Configuration::parse(&environment(&[
      ("CISCO_IPV6_SPLIT_INC", "1"),
      ("CISCO_IPV6_SPLIT_INC_0_ADDR", "fd00::"),
      ("CISCO_IPV6_SPLIT_INC_0_MASKLEN", "64"),
      ("INTERNAL_IP4_DNS", "10.20.30.1 fd00::1"),
    ]))
    .unwrap();
    assert!(plan.includes.iter().all(|route| route.addr().is_ipv4()));
    assert_eq!(
      plan.servers.iter().map(ToString::to_string).collect::<Vec<_>>(),
      ["10.20.30.1"]
    );
    assert!(Configuration::parse(&environment(&[("INTERNAL_IP4_DNS", "fd00::1")])).is_err());
  }

  #[test]
  fn malformed_network_data_is_rejected_before_changes() {
    for extra in [
      vec![("TUNIDX", "0")],
      vec![("INTERNAL_IP4_NETMASKLEN", "99")],
      vec![("INTERNAL_IP4_DNS", "not-an-address")],
      vec![("CISCO_SPLIT_INC", "1")],
    ] {
      assert!(Configuration::parse(&environment(&extra)).is_err());
    }
  }
}
