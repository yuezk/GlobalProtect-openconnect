use roxmltree::Document;

use super::{Gateway, PriorityRule};

pub(crate) fn parse_gateways(doc: &Document) -> Option<Vec<Gateway>> {
  let node_gateways = doc.descendants().find(|n| n.has_tag_name("gateways"))?;
  let list_gateway = node_gateways.descendants().find(|n| n.has_tag_name("list"))?;

  let gateways = list_gateway
    .children()
    .filter_map(|gateway_item| {
      if !gateway_item.has_tag_name("entry") {
        return None;
      }
      let address = gateway_item.attribute("name").unwrap_or("").to_string();
      let name = gateway_item
        .children()
        .find(|n| n.has_tag_name("description"))
        .and_then(|n| n.text())
        .unwrap_or("")
        .to_string();
      let priority = gateway_item
        .children()
        .find(|n| n.has_tag_name("priority"))
        .and_then(|n| n.text())
        .and_then(|s| s.parse().ok())
        .unwrap_or(u32::MAX);
      let priority_rules = gateway_item
        .children()
        .find(|n| n.has_tag_name("priority-rule"))
        .map(|n| {
          n.children()
            .filter_map(|n| {
              if !n.has_tag_name("entry") {
                return None;
              }
              let name = n.attribute("name").unwrap_or("").to_string();
              let priority: u32 = n
                .children()
                .find(|n| n.has_tag_name("priority"))
                .and_then(|n| n.text())
                .and_then(|s| s.parse().ok())
                .unwrap_or(u32::MAX);

              Some(PriorityRule { name, priority })
            })
            .collect()
        })
        .unwrap_or_default();

      Some(Gateway {
        name,
        address,
        priority,
        priority_rules,
      })
    })
    .collect();

  Some(gateways)
}
