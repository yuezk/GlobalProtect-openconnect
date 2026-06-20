use log::info;
use xmltree::Element;

use crate::utils::xml::ElementExt;

use super::{Gateway, GatewayKind, PriorityRule};

pub(crate) fn parse_gateways(element: &Element, prefer_internal: bool) -> Option<Vec<Gateway>> {
  let node_gateways = element.descendant("gateways")?;
  let internal_gateway_list = if prefer_internal {
    info!("Try to parse the internal gateways...");
    node_gateways
      .descendant("internal")
      .and_then(|n| n.child("list"))
      .map(|list| (list, GatewayKind::Internal))
  } else {
    None
  };

  let gateway_list = internal_gateway_list.or_else(|| {
    info!("Try to parse the external gateways...");
    node_gateways
      .descendant("external")
      .and_then(|n| n.child("list"))
      .map(|list| (list, GatewayKind::External))
  })?;
  let (gateway_list, kind) = gateway_list;

  let gateways = gateway_list
    .children("entry")
    .iter()
    .map(|gateway_item| {
      let address = gateway_item.attr("name").map(|s| s.to_string()).unwrap_or_default();
      let name = gateway_item.child_text("description").unwrap_or_default();
      let priority = parse_priority(gateway_item);
      let priority_rules = gateway_item
        .child("priority-rule")
        .map(parse_priority_rules)
        .unwrap_or_default();

      Gateway {
        name,
        address,
        kind,
        priority,
        priority_rules,
      }
    })
    .collect();

  Some(gateways)
}

fn parse_priority_rules(element: &Element) -> Vec<PriorityRule> {
  element
    .children("entry")
    .iter()
    .map(|n| {
      let name = n.attr("name").map(|s| s.to_string()).unwrap_or_default();
      let priority: u32 = parse_priority(n);

      PriorityRule { name, priority }
    })
    .collect()
}

fn parse_priority(element: &Element) -> u32 {
  element
    .child_text("priority")
    .and_then(|s| s.parse().ok())
    .unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_external_gateways_marks_gateway_kind() {
    let xml = r#"
<policy>
  <gateways>
    <external>
      <list>
        <entry name="us1.vpn.example.com">
          <description>US_East</description>
        </entry>
      </list>
    </external>
  </gateways>
</policy>
"#;
    let root = Element::parse(xml.as_bytes()).unwrap();

    let gateways = parse_gateways(&root, false).unwrap();

    assert_eq!(gateways[0].name(), "US_East");
    assert_eq!(gateways[0].server(), "us1.vpn.example.com");
    assert_eq!(gateways[0].kind(), GatewayKind::External);
  }

  #[test]
  fn parse_internal_gateways_marks_gateway_kind_when_preferred() {
    let xml = r#"
<policy>
  <gateways>
    <internal>
      <list>
        <entry name="internal.vpn.example.com">
          <description>Internal</description>
        </entry>
      </list>
    </internal>
  </gateways>
</policy>
"#;
    let root = Element::parse(xml.as_bytes()).unwrap();

    let gateways = parse_gateways(&root, true).unwrap();

    assert_eq!(gateways[0].kind(), GatewayKind::Internal);
  }
}
