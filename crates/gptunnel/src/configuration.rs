use std::net::{Ipv4Addr, Ipv6Addr};

use gpapi::session::tunnel::{MAX_CONFIGURATION_BYTES, TunnelAddress, TunnelConfiguration, TunnelList, TunnelState};
use log::{debug, info};
use openconnect::tunnel::{Configuration, NativeList};

pub(crate) fn normalize(configuration: Configuration) -> TunnelConfiguration {
  let ipv4 = configuration.ipv4.map(|address| {
    let prefix_length = address
      .parse::<Ipv4Addr>()
      .ok()
      .and_then(|_| configuration.netmask.as_deref().and_then(ipv4_prefix));
    TunnelAddress {
      address,
      netmask: configuration.netmask,
      prefix_length,
    }
  });
  let ipv6 = configuration.ipv6.map(|address| {
    let prefix_length = configuration
      .netmask6
      .as_deref()
      .and_then(|mask| ipv6_prefix(&address, mask));
    TunnelAddress {
      address,
      netmask: configuration.netmask6,
      prefix_length,
    }
  });
  let mut configuration = TunnelConfiguration {
    interface: configuration.interface,
    ipv4,
    ipv6,
    server_ip: configuration.server_ip,
    dns: list(configuration.dns),
    search_domains: list(configuration.search_domains),
    split_dns: list(configuration.split_dns),
    included_routes: list(configuration.included_routes),
    excluded_routes: list(configuration.excluded_routes),
    mtu: configuration.mtu,
    truncated: configuration.truncated,
  };
  assert!(configuration.limit_encoded_size(MAX_CONFIGURATION_BYTES));
  configuration
}

fn list(list: NativeList) -> TunnelList {
  TunnelList {
    items: list.items,
    total_count: list.total_count,
    truncated: list.truncated,
  }
}

fn ipv4_prefix(mask: &str) -> Option<u8> {
  let mask = u32::from(mask.parse::<Ipv4Addr>().ok()?);
  let prefix = mask.leading_ones();
  let expected = if prefix == 0 { 0 } else { u32::MAX << (32 - prefix) };
  (mask == expected).then_some(prefix as u8)
}

fn ipv6_prefix(address: &str, mask: &str) -> Option<u8> {
  let (network_address, prefix) = mask.split_once('/')?;
  if network_address.parse::<Ipv6Addr>().ok()? != address.parse::<Ipv6Addr>().ok()? {
    return None;
  }
  prefix.parse::<u8>().ok().filter(|prefix| *prefix <= 128)
}

fn escaped(value: &str) -> String {
  value.chars().flat_map(char::escape_debug).collect()
}

fn optional(value: Option<&str>) -> String {
  value.map(escaped).unwrap_or_else(|| "none".into())
}

fn addresses(address: Option<&TunnelAddress>) -> String {
  match address {
    Some(address) => escaped(&address.display()),
    None => "none".into(),
  }
}

fn values(list: &TunnelList) -> String {
  let mut value = if list.items.is_empty() {
    "none".into()
  } else {
    list
      .items
      .iter()
      .map(|item| escaped(item))
      .collect::<Vec<_>>()
      .join(", ")
  };
  if list.truncated {
    value.push_str(&format!(
      " ({} of {} shown; truncated)",
      list.items.len(),
      list.total_count
    ));
  }
  value
}

pub(crate) fn log_configuration(state: &TunnelState) {
  debug!(
    "VPN configuration: tunnel {}, revision {}",
    state.tunnel_id, state.configuration_revision
  );
  let Some(configuration) = &state.configuration else {
    info!("VPN configuration: unavailable");
    return;
  };
  info!("VPN interface: {}", optional(configuration.interface.as_deref()));
  info!("VPN IPv4: {}", addresses(configuration.ipv4.as_ref()));
  info!("VPN IPv6: {}", addresses(configuration.ipv6.as_ref()));
  info!("VPN gateway IP: {}", optional(configuration.server_ip.as_deref()));
  info!("VPN DNS: {}", values(&configuration.dns));
  info!("VPN search domains: {}", values(&configuration.search_domains));
  info!("VPN split DNS: {}", values(&configuration.split_dns));
  info!(
    "VPN routes: {} included, {} excluded",
    configuration.included_routes.total_count, configuration.excluded_routes.total_count
  );
  for (kind, routes) in [
    ("included", &configuration.included_routes),
    ("excluded", &configuration.excluded_routes),
  ] {
    for route in &routes.items {
      debug!("VPN {kind} route: {}", escaped(route));
    }
    if routes.truncated {
      debug!(
        "VPN {kind} routes truncated ({} of {} shown)",
        routes.items.len(),
        routes.total_count
      );
    }
  }
  let mtu = configuration
    .mtu
    .map(|mtu| mtu.to_string())
    .unwrap_or_else(|| "unavailable".into());
  info!("VPN MTU: {mtu}");
  if configuration.truncated {
    info!("VPN configuration: truncated");
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn masks_preserve_missing_or_invalid_prefixes() {
    let configuration = normalize(Configuration {
      ipv4: Some("10.0.0.2".into()),
      netmask: Some("255.0.255.0".into()),
      ipv6: Some("2001:db8::2".into()),
      netmask6: Some("2001:db8::2/64".into()),
      ..Default::default()
    });
    let ipv4 = configuration.ipv4.unwrap();
    assert_eq!(ipv4.prefix_length, None);
    assert_eq!(ipv4.netmask.as_deref(), Some("255.0.255.0"));
    assert_eq!(ipv4.display(), "10.0.0.2 (mask 255.0.255.0)");
    assert_eq!(configuration.ipv6.unwrap().prefix_length, Some(64));
    assert_eq!(ipv4_prefix("0.0.0.0"), Some(0));
    assert_eq!(ipv4_prefix("255.255.255.255"), Some(32));
    assert_eq!(ipv6_prefix("2001:db8::2", "2001:db8::3/64"), None);
    assert_eq!(
      normalize(Configuration {
        ipv4: Some("invalid".into()),
        netmask: Some("255.255.255.0".into()),
        ..Default::default()
      })
      .ipv4
      .unwrap()
      .prefix_length,
      None
    );
    assert_eq!(
      normalize(Configuration {
        ipv4: Some("10.0.0.2".into()),
        ..Default::default()
      })
      .ipv4
      .unwrap()
      .prefix_length,
      None
    );
  }

  #[test]
  fn escaped_route_lists_fit_the_contract_budget_without_claiming_completeness() {
    let routes = NativeList {
      items: vec!["\u{1}".repeat(1024); 16],
      total_count: 20,
      truncated: true,
    };
    let configuration = normalize(Configuration {
      included_routes: routes,
      ..Default::default()
    });
    assert!(serde_json::to_vec(&configuration).unwrap().len() <= MAX_CONFIGURATION_BYTES);
    assert_eq!(configuration.included_routes.total_count, 20);
    assert!(configuration.included_routes.truncated);
    assert!(configuration.truncated);
    assert!(configuration.included_routes.items.len() < 16);
  }

  #[test]
  fn control_characters_cannot_create_log_lines_or_terminal_sequences() {
    assert_eq!(escaped("corp\n\u{1b}[2J"), "corp\\n\\u{1b}[2J");
  }
}
