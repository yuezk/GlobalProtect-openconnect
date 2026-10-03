use super::*;

pub(super) fn routes(control: &dyn CollectionControl) -> anyhow::Result<Vec<RouteContext>> {
  let mut routes = Vec::new();
  for family in ["inet", "inet6"] {
    routes.extend(parse_routes(&command("netstat", &["-rn", "-f", family], control)?)?);
  }
  Ok(routes)
}

fn parse_routes(contents: &str) -> anyhow::Result<Vec<RouteContext>> {
  let mut interface_column = None;
  let mut routes = Vec::new();
  for line in contents.lines() {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.first() == Some(&"Destination") {
      interface_column = Some(
        fields
          .iter()
          .position(|field| matches!(*field, "Netif" | "Iface"))
          .context("Route table has no interface column")?,
      );
      continue;
    }
    let Some(column) = interface_column else {
      continue;
    };
    if fields.len() <= column {
      continue;
    }
    // L/W entries are neighbor-cache or cloned host routes. Their presence,
    // gateway MAC and expiry depend on traffic and must not cause rediscovery.
    if fields[2].contains('L') || fields[2].contains('W') {
      continue;
    }
    routes.push(RouteContext {
      destination: fields[0].into(),
      gateway: fields[1].into(),
      interface: fields[column].into(),
      table: String::new(),
      metric: String::new(),
    });
  }
  ensure!(interface_column.is_some(), "Route table header is missing");
  Ok(routes)
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn ignores_route_use_expiry_and_platform_column_positions() {
    let darwin = parse_routes("Routing tables\nInternet:\nDestination Gateway Flags Netif Expire\ndefault 192.0.2.1 UGScg en0\n192.0.2.5 aa:bb:cc:dd:ee:ff UHLWI en0 123").unwrap();
    let bsd = parse_routes("Destination Gateway Flags Refs Use Mtu Netif Expire\ndefault 192.0.2.1 UGS 4 22 1500 en0\n192.0.2.5 aa:bb:cc:dd:ee:ff UHL 0 33 1500 en0 99").unwrap();
    assert_eq!(darwin, bsd);
    assert_eq!(darwin.len(), 1);
    assert!(parse_routes("unrecognized table").is_err());
  }
}
