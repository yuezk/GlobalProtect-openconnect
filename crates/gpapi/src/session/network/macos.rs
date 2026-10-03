pub(super) use super::bsd_routes::routes;
use super::*;

pub(super) fn resolvers(control: &dyn CollectionControl) -> anyhow::Result<Vec<ResolverContext>> {
  parse_resolvers(&command("scutil", &["--dns"], control)?)
}

fn parse_resolvers(contents: &str) -> anyhow::Result<Vec<ResolverContext>> {
  let mut resolvers = Vec::new();
  let mut current: Option<ResolverContext> = None;
  for line in contents.lines().map(str::trim) {
    if line.starts_with("resolver #") {
      if let Some(resolver) = current.take() {
        resolvers.push(resolver);
      }
      current = Some(ResolverContext {
        interface: None,
        servers: Vec::new(),
        domains: Vec::new(),
      });
      continue;
    }
    let Some(resolver) = &mut current else {
      continue;
    };
    let Some((field, value)) = line.split_once(':') else {
      continue;
    };
    let field = field.trim();
    let value = value.trim();
    if field.starts_with("nameserver[") {
      value
        .split('%')
        .next()
        .unwrap_or_default()
        .parse::<IpAddr>()
        .context("Invalid macOS resolver address")?;
      resolver.servers.push(value.to_owned());
    } else if field == "domain" || field.starts_with("search domain[") {
      resolver.domains.push(value.to_ascii_lowercase());
    } else if field == "if_index" {
      if let Some((_, name)) = value.split_once('(') {
        resolver.interface = Some(
          name
            .strip_suffix(')')
            .context("Invalid macOS resolver interface")?
            .to_owned(),
        );
      }
    }
  }
  if let Some(resolver) = current {
    resolvers.push(resolver);
  }
  Ok(resolvers)
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn resolver_identity_is_scoped_and_ignores_reachability_counters() {
    let a = parse_resolvers(
      "resolver #1\n nameserver[0] : 192.0.2.53\n if_index : 4 (en0)\n search domain[0] : CORP.EXAMPLE\n reach : 2",
    )
    .unwrap();
    let b = parse_resolvers(
      "resolver #9\n search domain[0] : corp.example\n if_index : 4 (en0)\n nameserver[0] : 192.0.2.53\n reach : 3",
    )
    .unwrap();
    assert_eq!(a, b);
    assert_eq!(a[0].interface.as_deref(), Some("en0"));
  }
}
