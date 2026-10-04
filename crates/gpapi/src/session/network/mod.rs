use std::{
  collections::BTreeSet,
  io,
  net::{IpAddr, SocketAddr},
  time::Duration,
};

use anyhow::{Context, ensure};
use netdev::Interface;
use tokio_util::sync::CancellationToken;

use super::ClientAddresses;
use crate::process::collection::{CollectionBudget, CollectionControl, CollectorCommands};

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
mod bsd;
#[cfg(any(target_os = "freebsd", target_os = "openbsd", target_os = "macos", test))]
mod bsd_routes;
#[cfg(any(target_os = "linux", test))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "freebsd", target_os = "openbsd"))]
use bsd as platform;
#[cfg(target_os = "linux")]
use linux as platform;
#[cfg(target_os = "macos")]
use macos as platform;

mod binding;
mod detection;
pub mod native;
pub use binding::GatewayBinding;
pub use detection::InternalHostDetection;

const INSPECTION_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
struct InterfaceIdentity {
  index: u32,
  name: String,
  mac: Option<String>,
  flags: u32,
  addresses: BTreeSet<(String, u32)>,
}

impl From<&Interface> for InterfaceIdentity {
  fn from(interface: &Interface) -> Self {
    let mut addresses = interface
      .ipv4
      .iter()
      .map(|ip| (ip.to_string(), 0))
      .collect::<BTreeSet<_>>();
    addresses.extend(interface.ipv6.iter().enumerate().map(|(index, ip)| {
      (
        ip.to_string(),
        interface.ipv6_scope_ids.get(index).copied().unwrap_or_default(),
      )
    }));
    Self {
      index: interface.index,
      name: interface.name.clone(),
      mac: interface.mac_addr.map(|mac| mac.to_string()),
      flags: interface.flags,
      addresses,
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
struct RouteContext {
  destination: String,
  gateway: String,
  interface: String,
  table: String,
  metric: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
struct ResolverContext {
  interface: Option<String>,
  servers: Vec<String>,
  domains: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct NetworkSnapshot {
  interfaces: BTreeSet<InterfaceIdentity>,
  routes: BTreeSet<RouteContext>,
  resolvers: Vec<ResolverContext>,
}

/// Captured before this attempt installs a tunnel. Interface counters and
/// timestamps do not participate in identity; routes and resolver context do.
#[derive(Clone)]
pub struct PhysicalNetwork {
  snapshot: NetworkSnapshot,
  interfaces: Vec<Interface>,
}

impl PhysicalNetwork {
  pub async fn bind_authenticated_endpoint(
    &self,
    server: &str,
    endpoint: SocketAddr,
    disable_ipv6: bool,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<GatewayBinding> {
    self.ensure_current(cancellation).await?;
    let binding = resolve_binding(
      &self.interfaces,
      Some(self.fingerprint()?),
      server,
      disable_ipv6,
      cancellation,
      endpoint,
    )
    .await?;
    self.ensure_current(cancellation).await?;
    Ok(binding)
  }
  pub async fn validate_internal_binding(
    &self,
    binding: &GatewayBinding,
    detection: Option<&InternalHostDetection>,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<()> {
    self.validate_binding(binding, cancellation).await?;
    physical_interface_for_source(&self.interfaces, binding.source(), binding.endpoint())?;
    self.validate_internal(detection, cancellation).await
  }

  pub async fn validate_internal(
    &self,
    detection: Option<&InternalHostDetection>,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<()> {
    self.ensure_current(cancellation).await?;
    let mut resolver_count = 0;
    for resolver in &self.snapshot.resolvers {
      if resolver.servers.is_empty() {
        continue;
      }
      for server in &resolver.servers {
        let endpoint = resolver_endpoint(server, resolver.interface.as_deref(), &self.interfaces)?;
        ensure!(
          !endpoint.ip().is_loopback() && !endpoint.ip().is_unspecified(),
          "Unattributed local DNS stub is unsupported"
        );
        let socket = tokio::net::UdpSocket::bind(if endpoint.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).await?;
        if let Some(name) = &resolver.interface {
          let interface = self
            .interfaces
            .iter()
            .find(|interface| interface.name == *name)
            .context("Configured resolver interface is missing")?;
          platform::scope_resolver_socket(&socket, interface, endpoint)?;
        }
        socket.connect(endpoint).await?;
        let interface = physical_interface_for_source(&self.interfaces, socket.local_addr()?, endpoint)?;
        ensure!(
          resolver.interface.as_deref().is_none_or(|name| name == interface.name),
          "Resolver route differs from its configured interface"
        );
        resolver_count += 1;
      }
    }
    ensure!(resolver_count > 0, "Physical resolver context is unavailable");
    if let Some(detection) = detection {
      ensure!(
        detection.detect(cancellation).await?,
        "Internal host detection no longer matches"
      );
    }
    self.ensure_current(cancellation).await
  }
  pub async fn capture_controlled(cancellation: &CancellationToken) -> anyhow::Result<Self> {
    let cancellation = cancellation.clone();
    tokio::task::spawn_blocking(move || {
      let budget = CollectionBudget::new(INSPECTION_TIMEOUT);
      let check = || {
        if cancellation.is_cancelled() {
          return Err(io::Error::new(io::ErrorKind::Interrupted, "Network capture cancelled"));
        }
        budget.check()
      };
      Self::capture(&check)
    })
    .await?
  }

  pub async fn validate_binding(
    &self,
    binding: &GatewayBinding,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<()> {
    self.ensure_current(cancellation).await?;
    ensure!(
      binding.network_fingerprint() == Some(self.fingerprint()?.as_str()),
      "Physical network changed since authentication"
    );
    let interface = self
      .interfaces
      .iter()
      .find(|interface| interface.index == binding.interface_index() && interface.name == binding.interface())
      .context("Authenticated gateway interface is no longer present")?;
    let addresses = binding.addresses();
    ensure!(
      addresses
        .ipv4
        .is_none_or(|ip| interface.ipv4.iter().any(|address| address.addr() == ip)),
      "Authenticated IPv4 address changed"
    );
    ensure!(
      addresses
        .ipv6
        .is_none_or(|ip| interface.ipv6.iter().any(|address| address.addr() == ip)),
      "Authenticated IPv6 address changed"
    );
    Ok(())
  }

  fn fingerprint(&self) -> anyhow::Result<String> {
    Ok(sha256::digest(serde_json::to_vec(&self.snapshot)?))
  }

  pub fn capture(control: &dyn CollectionControl) -> anyhow::Result<Self> {
    control.check()?;
    let mut interfaces = netdev::get_interfaces();
    for interface in &mut interfaces {
      interface.ipv4.sort();
      // The address and its zone must move together when native enumeration
      // order changes between captures.
      let mut ipv6 = interface
        .ipv6
        .iter()
        .copied()
        .enumerate()
        .map(|(index, address)| {
          (
            address,
            interface.ipv6_scope_ids.get(index).copied().unwrap_or_default(),
          )
        })
        .collect::<Vec<_>>();
      ipv6.sort();
      interface.ipv6 = ipv6.iter().map(|(address, _)| *address).collect();
      interface.ipv6_scope_ids = ipv6.into_iter().map(|(_, scope)| scope).collect();
    }
    let snapshot = snapshot(&interfaces, control)?;
    control.check()?;
    Ok(Self { snapshot, interfaces })
  }

  /// Validate endpoint selection against the captured network. The inspection
  /// is joined even when cancellation occurs, so native collectors cannot outlive
  /// the connection attempt.
  pub async fn ensure_current(&self, cancellation: &CancellationToken) -> anyhow::Result<()> {
    let current = inspect(cancellation.clone()).await?;
    ensure!(
      current == self.snapshot,
      "Physical network changed during gateway setup"
    );
    Ok(())
  }

  /// The owner must cancel and await this monitor, including its blocking
  /// inspection. Dropping a selected future would detach the native collector.
  pub async fn wait_for_change(
    &self,
    detection: Option<InternalHostDetection>,
    cancellation: CancellationToken,
  ) -> anyhow::Result<bool> {
    loop {
      tokio::select! {
        biased;
        _ = cancellation.cancelled() => return Ok(false),
        _ = tokio::time::sleep(POLL_INTERVAL) => {},
      }
      let current = self.validate_internal(detection.as_ref(), &cancellation).await;
      if cancellation.is_cancelled() {
        return Ok(false);
      }
      if current.is_err() {
        return Ok(true);
      }
    }
  }
}

async fn resolve_binding(
  interfaces: &[Interface],
  fingerprint: Option<String>,
  server: &str,
  disable_ipv6: bool,
  cancellation: &CancellationToken,
  authenticated_endpoint: SocketAddr,
) -> anyhow::Result<GatewayBinding> {
  let origin = super::transport::gateway_origin(server)?;
  let host = origin.host_str().context("Gateway host is missing")?;
  let port = origin.port_or_known_default().context("Gateway port is missing")?;
  let literal = literal_gateway_address(&origin)?;
  let endpoints = if let Some(ip) = literal {
    vec![SocketAddr::new(ip, port)]
  } else {
    native::resolve(host, port, disable_ipv6, cancellation).await?
  };
  ensure!(
    endpoints.contains(&authenticated_endpoint),
    "Authenticated peer differs from the gateway's current DNS addresses; check for a proxy or changed DNS answer"
  );
  let endpoint = authenticated_endpoint;
  ensure!(
    !(disable_ipv6 && endpoint.is_ipv6()),
    "Authenticated gateway uses disabled IPv6"
  );
  let bind = if endpoint.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" };
  let socket = tokio::net::UdpSocket::bind(bind).await?;
  socket.connect(endpoint).await?;
  let mut source_address = socket.local_addr()?;
  source_address.set_port(0);
  let source = source_address.ip();
  let interface = physical_interface_for_source(interfaces, source_address, endpoint)?;
  let ipv4 = match source {
    IpAddr::V4(ip) => Some(ip),
    _ => interface
      .ipv4
      .iter()
      .map(|ip| ip.addr())
      .find(|ip| !ip.is_loopback() && !ip.is_unspecified()),
  };
  let ipv6 = if disable_ipv6 {
    None
  } else {
    match source {
      IpAddr::V6(ip) => Some(ip),
      _ => interface
        .ipv6
        .iter()
        .map(|ip| ip.addr())
        .find(|ip| !ip.is_loopback() && !ip.is_unspecified() && !ip.is_unicast_link_local()),
    }
  };
  if let std::net::SocketAddr::V6(address) = &mut source_address {
    if address.ip().is_unicast_link_local() && address.scope_id() == 0 {
      address.set_scope_id(interface.index);
    }
  }
  GatewayBinding::new(
    endpoint,
    source_address,
    interface.name.clone(),
    interface.index,
    ClientAddresses { ipv4, ipv6 },
    fingerprint.clone(),
  )
}

// Default-route inventories include scoped, backup, and unrelated address-family
// routes. Attribute each actual destination by its selected source instead.
fn physical_interface_for_source(
  interfaces: &[Interface],
  source: SocketAddr,
  endpoint: SocketAddr,
) -> anyhow::Result<&Interface> {
  let mut scope = None;
  for address in [source, endpoint] {
    if let SocketAddr::V6(address) = address {
      if address.scope_id() != 0 {
        ensure!(
          scope.is_none_or(|scope| scope == address.scope_id()),
          "Network source and destination have different interface scopes"
        );
        scope = Some(address.scope_id());
      }
    }
  }
  let source = source.ip();
  ensure!(
    !source.is_unspecified() && !source.is_loopback(),
    "Network route has no physical source address"
  );
  let mut matches = interfaces.iter().filter(|interface| {
    scope.is_none_or(|scope| scope == interface.index)
      && (interface.ipv4.iter().any(|ip| IpAddr::V4(ip.addr()) == source)
        || interface.ipv6.iter().any(|ip| IpAddr::V6(ip.addr()) == source))
  });
  let interface = matches
    .next()
    .context("Network route has no matching physical interface")?;
  ensure!(
    matches.next().is_none(),
    "Network route has ambiguous physical interfaces"
  );
  ensure!(
    matches!(
      interface.if_type,
      netdev::prelude::InterfaceType::Ethernet
        | netdev::prelude::InterfaceType::Wireless80211
        | netdev::prelude::InterfaceType::FastEthernetT
        | netdev::prelude::InterfaceType::FastEthernetFx
        | netdev::prelude::InterfaceType::GigabitEthernet
    ),
    "Network route uses an unsupported physical interface: {}",
    interface.name
  );
  Ok(interface)
}

fn resolver_endpoint(server: &str, interface: Option<&str>, interfaces: &[Interface]) -> anyhow::Result<SocketAddr> {
  let (address, zone) = server
    .split_once('%')
    .map_or((server, None), |(ip, zone)| (ip, Some(zone)));
  let ip: IpAddr = address.parse().context("Invalid resolver address")?;
  let mut endpoint = SocketAddr::new(ip, 53);
  if let SocketAddr::V6(address) = &mut endpoint {
    let zone = if zone.is_none() && address.ip().is_unicast_link_local() {
      interface
    } else {
      zone
    };
    if let Some(zone) = zone {
      let index = interfaces
        .iter()
        .find(|candidate| candidate.name == zone || candidate.index.to_string() == zone)
        .context("Resolver IPv6 zone has no matching interface")?
        .index;
      address.set_scope_id(index);
    }
    ensure!(
      !address.ip().is_unicast_link_local() || address.scope_id() != 0,
      "Link-local resolver has no interface scope"
    );
  } else {
    ensure!(zone.is_none(), "IPv4 resolver cannot have an interface scope");
  }
  Ok(endpoint)
}

async fn inspect(cancellation: CancellationToken) -> anyhow::Result<NetworkSnapshot> {
  tokio::task::spawn_blocking(move || {
    let budget = CollectionBudget::new(INSPECTION_TIMEOUT);
    let check = || {
      if cancellation.is_cancelled() {
        return Err(io::Error::new(
          io::ErrorKind::Interrupted,
          "Network inspection cancelled",
        ));
      }
      budget.check()
    };
    let result = snapshot(&netdev::get_interfaces(), &check)?;
    check()?;
    Ok(result)
  })
  .await?
}

fn literal_gateway_address(origin: &url::Url) -> anyhow::Result<Option<IpAddr>> {
  Ok(match origin.host().context("Gateway host is missing")? {
    url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
    url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
    url::Host::Domain(_) => None,
  })
}

fn snapshot(interfaces: &[Interface], control: &dyn CollectionControl) -> anyhow::Result<NetworkSnapshot> {
  control.check()?;
  Ok(NetworkSnapshot {
    interfaces: interfaces.iter().map(InterfaceIdentity::from).collect(),
    routes: platform::routes(control)?.into_iter().collect(),
    resolvers: platform::resolvers(control)?,
  })
}

fn command(name: &str, args: &[&str], control: &dyn CollectionControl) -> anyhow::Result<String> {
  let output = CollectorCommands::new(control)
    .run(name, args)?
    .with_context(|| format!("Required network tool {name} is unavailable"))?;
  ensure!(output.status.success(), "Network tool {name} failed");
  String::from_utf8(output.stdout).context("Network tool output is not UTF-8")
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
fn resolv_conf(control: &dyn CollectionControl) -> anyhow::Result<Vec<ResolverContext>> {
  control.check()?;
  let file = std::fs::File::open("/etc/resolv.conf")?;
  use std::io::Read;
  let mut contents = String::new();
  file.take(64 * 1024 + 1).read_to_string(&mut contents)?;
  ensure!(contents.len() <= 64 * 1024, "Resolver configuration is too large");
  control.check()?;
  Ok(vec![parse_resolv_conf(&contents)?])
}

#[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd", test))]
fn parse_resolv_conf(contents: &str) -> anyhow::Result<ResolverContext> {
  let mut resolver = ResolverContext {
    interface: None,
    servers: Vec::new(),
    domains: Vec::new(),
  };
  for line in contents.lines() {
    let line = line.split(['#', ';']).next().unwrap_or_default();
    let mut fields = line.split_whitespace();
    match fields.next() {
      Some("nameserver") => {
        if let Some(server) = fields.next() {
          // IPv6 zone identifiers are retained for scoped physical resolvers.
          server
            .split('%')
            .next()
            .unwrap_or_default()
            .parse::<IpAddr>()
            .context("Invalid resolver address")?;
          resolver.servers.push(server.to_owned());
        }
      }
      Some("search" | "domain") => resolver
        .domains
        .extend(fields.map(|domain| domain.to_ascii_lowercase())),
      _ => {}
    }
  }
  Ok(resolver)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ethernet(name: &str, index: u32, address: &str) -> Interface {
    let mut interface = Interface::dummy();
    interface.name = name.into();
    interface.index = index;
    interface.if_type = netdev::prelude::InterfaceType::Ethernet;
    interface.ipv4 = vec![address.parse().unwrap()];
    interface.ipv6 = vec!["fe80::1/64".parse().unwrap()];
    interface
  }

  #[test]
  fn attributes_each_destination_without_requiring_a_single_uplink() {
    let ethernet = ethernet("en0", 7, "192.0.2.10/24");
    let mut wifi = ethernet.clone();
    wifi.name = "en1".into();
    wifi.index = 8;
    wifi.if_type = netdev::prelude::InterfaceType::Wireless80211;
    wifi.ipv4 = vec!["198.51.100.10/24".parse().unwrap()];
    let mut tunnel = Interface::dummy();
    tunnel.name = "utun0".into();
    tunnel.index = 9;
    tunnel.ipv4 = vec!["10.0.0.1/24".parse().unwrap()];
    let interfaces = [ethernet, wifi, tunnel];
    for (source, endpoint, expected) in [
      ("192.0.2.10:0", "203.0.113.1:443", "en0"),
      ("198.51.100.10:0", "198.51.100.53:53", "en1"),
      ("[fe80::1%8]:0", "[fe80::53%8]:53", "en1"),
    ] {
      assert_eq!(
        physical_interface_for_source(&interfaces, source.parse().unwrap(), endpoint.parse().unwrap())
          .unwrap()
          .name,
        expected
      );
    }
    // Virtual interfaces may coexist, but must never supply the selected source.
    assert!(
      physical_interface_for_source(
        &interfaces,
        "10.0.0.1:0".parse().unwrap(),
        "10.0.0.53:53".parse().unwrap()
      )
      .is_err()
    );
    // Identical link-local addresses without scope are ambiguous.
    assert!(
      physical_interface_for_source(
        &interfaces,
        "[fe80::1]:0".parse().unwrap(),
        "[fe80::53]:53".parse().unwrap()
      )
      .is_err()
    );
    assert!(
      physical_interface_for_source(
        &interfaces,
        "[fe80::1%7]:0".parse().unwrap(),
        "[fe80::53%8]:53".parse().unwrap()
      )
      .is_err()
    );
  }

  #[test]
  fn rejects_ambiguous_or_unattributed_sources() {
    let interfaces = [ethernet("en0", 7, "192.0.2.10/24"), ethernet("en1", 8, "192.0.2.10/24")];
    for source in ["192.0.2.10:0", "192.0.2.11:0", "127.0.0.1:0", "0.0.0.0:0"] {
      assert!(
        physical_interface_for_source(&interfaces, source.parse().unwrap(), "192.0.2.53:53".parse().unwrap()).is_err()
      );
    }
  }

  #[test]
  fn resolver_zones_support_native_names_and_numeric_indices() {
    let interfaces = [ethernet("en0", 7, "192.0.2.10/24")];
    let expected: SocketAddr = "[fe80::53%7]:53".parse().unwrap();
    assert_eq!(resolver_endpoint("fe80::53%en0", None, &interfaces).unwrap(), expected);
    assert_eq!(resolver_endpoint("fe80::53%7", None, &interfaces).unwrap(), expected);
    assert_eq!(
      resolver_endpoint("fe80::53", Some("en0"), &interfaces).unwrap(),
      expected
    );
    assert_eq!(
      resolver_endpoint("192.0.2.53", Some("en0"), &interfaces).unwrap(),
      "192.0.2.53:53".parse().unwrap()
    );
    for server in ["fe80::53", "fe80::53%missing", "fe80::53%8", "192.0.2.53%en0"] {
      assert!(
        resolver_endpoint(server, None, &interfaces).is_err(),
        "accepted {server}"
      );
    }
  }

  #[test]
  fn bracketed_ipv6_gateway_is_a_literal_endpoint() {
    let origin = super::super::transport::gateway_origin("https://[2001:db8::1]:444").unwrap();
    assert_eq!(
      literal_gateway_address(&origin).unwrap(),
      Some("2001:db8::1".parse().unwrap())
    );
  }

  #[test]
  fn resolver_order_comments_and_case_do_not_change_network_identity() {
    assert_eq!(
      parse_resolv_conf("nameserver 192.0.2.53\nsearch CORP.EXAMPLE\n# comment").unwrap(),
      parse_resolv_conf("search corp.example\nnameserver 192.0.2.53 # DHCP\n").unwrap()
    );
    assert_ne!(
      parse_resolv_conf("nameserver 192.0.2.53").unwrap(),
      parse_resolv_conf("nameserver 192.0.2.54").unwrap()
    );
  }

  #[test]
  fn interface_identity_ignores_counters_and_address_enumeration_order() {
    let mut interface = Interface::dummy();
    interface.index = 7;
    interface.name = "bridge0".into();
    interface.ipv4 = vec!["192.0.2.10/24".parse().unwrap(), "192.0.2.11/24".parse().unwrap()];
    let original = InterfaceIdentity::from(&interface);
    interface.ipv4.reverse();
    interface.mtu = Some(1200);
    interface.default = !interface.default;
    assert_eq!(original, InterfaceIdentity::from(&interface));
    interface.index = 8;
    assert_ne!(original, InterfaceIdentity::from(&interface));
  }
}

#[cfg(test)]
pub(crate) fn test_binding() -> GatewayBinding {
  GatewayBinding::new(
    "192.0.2.10:443".parse().unwrap(),
    "192.0.2.5:0".parse().unwrap(),
    "fixture".into(),
    1,
    ClientAddresses {
      ipv4: Some("192.0.2.5".parse().unwrap()),
      ipv6: None,
    },
    "0".repeat(64),
  )
  .unwrap()
}
