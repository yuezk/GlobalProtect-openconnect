use log::info;
use xmltree::Element;

use crate::utils::xml::ElementExt;

use super::{Gateway, PriorityRule};

pub(crate) fn parse_gateways(root: &Element, prefer_internal: bool) -> Option<Vec<Gateway>> {
  let node_gateways = root.descendant("gateways")?;
  let internal_gateway_list = if prefer_internal {
    info!("Try to parse the internal gateways...");
    node_gateways.descendant("internal").and_then(|node| node.child("list"))
  } else {
    None
  };

  let gateway_list = internal_gateway_list.or_else(|| {
    info!("Try to parse the external gateways...");
    node_gateways.descendant("external").and_then(|node| node.child("list"))
  })?;

  let gateways = gateway_list
    .children("entry")
    .iter()
    .map(|gateway_item| {
      let address = gateway_item.attr("name").unwrap_or_default().to_string();
      let name = gateway_item.child_text("description").unwrap_or_default();
      let priority = gateway_item
        .child_text("priority")
        .and_then(|s| s.parse().ok())
        .unwrap_or(u32::MAX);
      let priority_rules = gateway_item
        .child("priority-rule")
        .map(|node| {
          node
            .children("entry")
            .iter()
            .map(|entry| {
              let name = entry.attr("name").unwrap_or_default().to_string();
              let priority: u32 = entry
                .child_text("priority")
                .and_then(|s| s.parse().ok())
                .unwrap_or(u32::MAX);

              PriorityRule { name, priority }
            })
            .collect()
        })
        .unwrap_or_default();

      Gateway {
        name,
        address,
        priority,
        priority_rules,
      }
    })
    .collect();

  Some(gateways)
}
