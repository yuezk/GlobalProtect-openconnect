use super::*;

#[cfg(target_os = "linux")]
pub(super) fn scope_resolver_socket(
  socket: &tokio::net::UdpSocket,
  interface: &Interface,
  _endpoint: SocketAddr,
) -> anyhow::Result<()> {
  socket2::SockRef::from(socket).bind_device(Some(interface.name.as_bytes()))?;
  Ok(())
}

#[cfg(target_os = "linux")]
pub(super) fn routes(control: &dyn CollectionControl) -> anyhow::Result<Vec<RouteContext>> {
  let mut routes = Vec::new();
  for family in ["-4", "-6"] {
    routes.extend(parse_routes(&command(
      "ip",
      &[family, "-j", "route", "show", "table", "all"],
      control,
    )?)?);
  }
  Ok(routes)
}

fn parse_routes(contents: &str) -> anyhow::Result<Vec<RouteContext>> {
  let value: Vec<serde_json::Value> = serde_json::from_str(contents)?;
  let mut routes = Vec::new();
  for route in value {
    // Kernel cache and neighbor lifetimes are traffic, not network changes.
    if route.get("cache").is_some() {
      continue;
    }
    let Some(interface) = route["dev"].as_str() else {
      continue;
    };
    let field = |name| {
      route
        .get(name)
        .map(|value| value.as_str().map(str::to_owned).unwrap_or_else(|| value.to_string()))
        .unwrap_or_default()
    };
    routes.push(RouteContext {
      destination: field("dst"),
      gateway: field("gateway"),
      interface: interface.into(),
      table: field("table"),
      metric: field("metric"),
    });
  }
  Ok(routes)
}

#[cfg(target_os = "linux")]
pub(super) fn resolvers(control: &dyn CollectionControl) -> anyhow::Result<Vec<ResolverContext>> {
  let path = std::fs::canonicalize("/etc/resolv.conf")?;
  if path.starts_with("/run/systemd/resolve") {
    let dns = command("resolvectl", &["dns"], control)?;
    let domains = command("resolvectl", &["domain"], control)?;
    return parse_resolved(&dns, &domains);
  }
  resolv_conf(control)
}

fn parse_resolved(dns: &str, domains: &str) -> anyhow::Result<Vec<ResolverContext>> {
  let mut resolvers = std::collections::BTreeMap::<Option<String>, ResolverContext>::new();
  for (contents, is_dns) in [(dns, true), (domains, false)] {
    for line in contents.lines().filter(|line| !line.trim().is_empty()) {
      let (label, values) = line.split_once(':').context("Invalid systemd resolver output")?;
      let interface = if label == "Global" {
        None
      } else {
        let (_, name) = label.split_once('(').context("Invalid systemd resolver interface")?;
        Some(
          name
            .strip_suffix(')')
            .context("Invalid systemd resolver interface")?
            .to_owned(),
        )
      };
      let resolver = resolvers.entry(interface.clone()).or_insert_with(|| ResolverContext {
        interface,
        servers: Vec::new(),
        domains: Vec::new(),
      });
      if is_dns {
        for server in values.split_whitespace() {
          server
            .split('%')
            .next()
            .unwrap_or_default()
            .parse::<IpAddr>()
            .context("Invalid systemd resolver address")?;
          resolver.servers.push(server.to_owned());
        }
      } else {
        resolver
          .domains
          .extend(values.split_whitespace().map(|domain| domain.to_ascii_lowercase()));
      }
    }
  }
  Ok(resolvers.into_values().collect())
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn route_identity_retains_ipv6_and_ignores_cache() {
    let routes = parse_routes(r#"[{"dst":"default","gateway":"2001:db8::1","dev":"eth0","metric":600,"table":"main"},{"dst":"2001:db8::5","dev":"eth0","cache":[]}]"#).unwrap();
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0].gateway, "2001:db8::1");
    assert_eq!(routes[0].metric, "600");
  }
  #[test]
  fn resolved_preserves_interface_domains_and_empty_global_context() {
    let resolvers = parse_resolved(
      "Global:\nLink 2 (eth0): 192.0.2.53\n\nLink 3 (tun0): 198.51.100.53\n",
      "Global:\nLink 2 (eth0): CORP.EXAMPLE\nLink 3 (tun0): ~.\n",
    )
    .unwrap();
    assert_eq!(resolvers.len(), 3);
    assert!(
      resolvers
        .iter()
        .any(|resolver| resolver.interface.as_deref() == Some("eth0")
          && resolver.domains.iter().any(|domain| domain == "corp.example"))
    );
  }
}
