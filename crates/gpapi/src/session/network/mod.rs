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
    ensure!(
      binding.interface() == self.physical_interface()?.name,
      "Gateway uses a different physical uplink"
    );
    self.validate_internal(detection, cancellation).await
  }

  fn physical_interface(&self) -> anyhow::Result<&Interface> {
    let defaults = self
      .snapshot
      .routes
      .iter()
      .filter(|route| matches!(route.destination.as_str(), "default" | "0.0.0.0/0" | "::/0"))
      .map(|route| route.interface.as_str())
      .collect::<BTreeSet<_>>();
    ensure!(
      defaults.len() == 1,
      "Non-tunnel sessions require one unambiguous physical uplink"
    );
    let name = defaults.first().context("Physical default route is missing")?;
    let interface = self
      .interfaces
      .iter()
      .find(|interface| interface.name == *name)
      .context("Physical interface is missing")?;
    ensure!(
      matches!(
        interface.if_type,
        netdev::prelude::InterfaceType::Ethernet
          | netdev::prelude::InterfaceType::Wireless80211
          | netdev::prelude::InterfaceType::FastEthernetT
          | netdev::prelude::InterfaceType::FastEthernetFx
          | netdev::prelude::InterfaceType::GigabitEthernet
      ),
      "Unsupported physical interface type"
    );
    Ok(interface)
  }

  pub async fn validate_internal(
    &self,
    detection: Option<&InternalHostDetection>,
    cancellation: &CancellationToken,
  ) -> anyhow::Result<()> {
    self.ensure_current(cancellation).await?;
    let interface = self.physical_interface()?;
    let mut resolver_count = 0;
    for resolver in &self.snapshot.resolvers {
      if resolver.servers.is_empty() {
        continue;
      }
      ensure!(
        resolver.interface.as_deref().is_none_or(|name| name == interface.name),
        "Split physical resolver attribution is unsupported"
      );
      for server in &resolver.servers {
        let server = if server.contains(':') {
          format!("[{server}]:53")
        } else {
          format!("{server}:53")
        };
        let endpoint: SocketAddr = server.parse().context("Scoped resolver attribution is unsupported")?;
        ensure!(
          !endpoint.ip().is_loopback() && !endpoint.ip().is_unspecified(),
          "Unattributed local DNS stub is unsupported"
        );
        let socket = tokio::net::UdpSocket::bind(if endpoint.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).await?;
        socket.connect(endpoint).await?;
        let source = socket.local_addr()?.ip();
        ensure!(
          interface.ipv4.iter().any(|ip| IpAddr::V4(ip.addr()) == source)
            || interface.ipv6.iter().any(|ip| IpAddr::V6(ip.addr()) == source),
          "Resolver does not use the physical uplink"
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
    "Authenticated peer is not a resolved gateway endpoint; proxied non-tunnel sessions are unsupported"
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
  let scope = [source_address, endpoint]
    .into_iter()
    .find_map(|address| match address {
      SocketAddr::V6(address) if address.scope_id() != 0 => Some(address.scope_id()),
      _ => None,
    });
  let mut matches = interfaces.iter().filter(|interface| {
    scope.is_none_or(|scope| scope == interface.index)
      && (interface.ipv4.iter().any(|ip| IpAddr::V4(ip.addr()) == source)
        || interface.ipv6.iter().any(|ip| IpAddr::V6(ip.addr()) == source))
  });
  let interface = matches
    .next()
    .context("Gateway route has no matching physical interface")?;
  ensure!(
    matches.next().is_none(),
    "Gateway route has ambiguous physical interfaces"
  );
  ensure!(
    !source.is_unspecified() && !source.is_loopback(),
    "Gateway route has no source address"
  );
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
