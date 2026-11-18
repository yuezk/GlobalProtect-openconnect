//! Windows posture facts and their HIP v4 representation.
use serde::Deserialize;
use xmltree::{Element, XMLNode};

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct WindowsReport {
  pub anti_malware: Vec<AntiMalware>,
  pub disk_backup: Vec<Backup>,
  pub disk_encryption: Vec<Encryption>,
  pub firewall: Vec<ProductState>,
  pub patch_management: Vec<ProductState>,
  pub missing_patches: Option<Vec<MissingPatch>>,
  pub patches_checked_at: Option<u64>,
  pub unavailable: Vec<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Product {
  pub name: String,
  pub vendor: String,
  pub version: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct AntiMalware {
  pub product: Product,
  pub enabled: Option<bool>,
  pub definition_version: Option<String>,
  pub engine_version: Option<String>,
  pub definition_date: Option<String>,
  pub last_scan: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Backup {
  pub product: Product,
  pub last_backup: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct Encryption {
  pub product: Product,
  pub drives: Vec<Drive>,
}

#[derive(Deserialize)]
pub(super) struct Drive {
  pub name: String,
  pub conversion_status: Option<u32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct ProductState {
  pub product: Product,
  pub enabled: Option<bool>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub(super) struct MissingPatch {
  pub title: String,
  pub description: String,
  pub kb: String,
  pub category: String,
  pub vendor: String,
  pub msrc_severity: Option<String>,
}

impl MissingPatch {
  fn severity(&self) -> i32 {
    match self.msrc_severity.as_deref().map(str::to_ascii_lowercase).as_deref() {
      Some("critical") => 3,
      Some("important") => 2,
      Some("moderate") => 1,
      Some("low") => 0,
      _ => -1,
    }
  }
}

impl WindowsReport {
  pub(super) fn update_status(&self) -> gpapi::hip::WindowsUpdateStatus {
    let mut status = gpapi::hip::WindowsUpdateStatus::default();
    if self.missing_patches.is_some() {
      status.checked_at = self.patches_checked_at;
    } else {
      status.omitted_snapshot_at = self.patches_checked_at;
    }
    status
  }

  pub fn render(&self) -> std::io::Result<String> {
    let mut categories = Vec::new();
    categories.push(category(
      "anti-malware",
      self.anti_malware.iter().map(|item| {
        let mut info = product(&item.product);
        let prod = info.get_mut_child("Prod").unwrap();
        prod.attributes.insert("prodType".into(), "3".into());
        prod.attributes.insert("osType".into(), "1".into());
        for (name, value) in [("defver", &item.definition_version), ("engver", &item.engine_version)] {
          if let Some(value) = value {
            prod.attributes.insert(name.into(), value.clone());
          }
        }
        if let Some(date) = item
          .definition_date
          .as_deref()
          .and_then(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        {
          use chrono::Datelike;
          for (name, value) in [
            ("dateyear", date.year().to_string()),
            ("datemon", date.month().to_string()),
            ("dateday", date.day().to_string()),
          ] {
            prod.attributes.insert(name.into(), value);
          }
        }
        field(&mut info, "real-time-protection", yes_no(item.enabled));
        field(
          &mut info,
          "last-full-scan-time",
          item.last_scan.as_deref().unwrap_or("n/a"),
        );
        info
      }),
    ));
    categories.push(category(
      "disk-backup",
      self.disk_backup.iter().map(|item| {
        let mut info = product(&item.product);
        field(
          &mut info,
          "last-backup-time",
          item.last_backup.as_deref().unwrap_or("n/a"),
        );
        info
      }),
    ));
    categories.push(category(
      "disk-encryption",
      self.disk_encryption.iter().map(|item| {
        let mut info = product(&item.product);
        let mut drives = Element::new("drives");
        for drive in &item.drives {
          let mut entry = Element::new("entry");
          field(&mut entry, "drive-name", &drive.name);
          field(
            &mut entry,
            "enc-state",
            match drive.conversion_status {
              Some(0) => "unencrypted",
              Some(1) => "encrypted",
              _ => "n/a",
            },
          );
          drives.children.push(XMLNode::Element(entry));
        }
        info.children.push(XMLNode::Element(drives));
        info
      }),
    ));
    categories.push(category("firewall", self.firewall.iter().map(state)));
    let mut patches = category("patch-management", self.patch_management.iter().map(state));
    // A failed search is not evidence that the machine has no missing updates.
    if let Some(missing) = &self.missing_patches {
      let mut list = Element::new("missing-patches");
      for patch in missing {
        let mut entry = Element::new("entry");
        field(&mut entry, "title", &patch.title);
        field(&mut entry, "description", &patch.description);
        field(&mut entry, "product", "");
        field(&mut entry, "vendor", &patch.vendor);
        field(&mut entry, "info-url", "");
        field(&mut entry, "kb-article-id", &patch.kb);
        field(&mut entry, "security-bulletin-id", "");
        field(&mut entry, "severity", &patch.severity().to_string());
        field(&mut entry, "category", &patch.category);
        field(&mut entry, "is-installed", "no");
        list.children.push(XMLNode::Element(entry));
      }
      patches.children.push(XMLNode::Element(list));
    }
    categories.push(patches);
    let mut output = Vec::new();
    for category in categories {
      category
        .write_with_config(
          &mut output,
          xmltree::EmitterConfig::new().write_document_declaration(false),
        )
        .map_err(std::io::Error::other)?;
    }
    String::from_utf8(output).map_err(std::io::Error::other)
  }
}

fn yes_no(enabled: Option<bool>) -> &'static str {
  match enabled {
    Some(true) => "yes",
    Some(false) => "no",
    None => "n/a",
  }
}

fn field(parent: &mut Element, name: &str, value: &str) {
  let mut child = Element::new(name);
  child.children.push(XMLNode::Text(value.into()));
  parent.children.push(XMLNode::Element(child));
}

fn product(product: &Product) -> Element {
  let mut info = Element::new("ProductInfo");
  let mut prod = Element::new("Prod");
  for (name, value) in [
    ("name", &product.name),
    ("vendor", &product.vendor),
    ("version", &product.version),
  ] {
    if !value.is_empty() {
      prod.attributes.insert(name.into(), value.clone());
    }
  }
  info.children.push(XMLNode::Element(prod));
  info
}

fn state(item: &ProductState) -> Element {
  let mut info = product(&item.product);
  field(&mut info, "is-enabled", yes_no(item.enabled));
  info
}

fn category(name: &str, products: impl Iterator<Item = Element>) -> Element {
  let mut category = Element::new("entry");
  category.attributes.insert("name".into(), name.into());
  let mut list = Element::new("list");
  for product in products {
    let mut entry = Element::new("entry");
    entry.children.push(XMLNode::Element(product));
    list.children.push(XMLNode::Element(entry));
  }
  category.children.push(XMLNode::Element(list));
  category
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn omitted_snapshot_can_be_refreshed_without_claiming_patch_details() {
    let mut report = WindowsReport::default();
    assert_eq!(report.update_status().refresh_cursor(), None);
    report.patches_checked_at = Some(123);
    let status = report.update_status();
    assert_eq!(status.checked_at, None);
    assert_eq!(status.refresh_cursor(), Some(123));
    assert!(!report.render().unwrap().contains("missing-patches"));
    report.missing_patches = Some(Vec::new());
    assert_eq!(report.update_status().checked_at, Some(123));
    assert_eq!(report.update_status().refresh_cursor(), Some(123));
  }

  #[test]
  fn real_posture_preserves_unavailable_values_and_escapes_product_text() {
    let report: WindowsReport = serde_json::from_str(
      r#"{
      "anti_malware":[{"product":{"name":"Agent & Endpoint","version":"25.1"},"enabled":true}],
      "disk_encryption":[{"product":{"name":"BitLocker"},"drives":[{"name":"C:\\","conversion_status":0}]}],
      "missing_patches":null
    }"#,
    )
    .unwrap();
    let xml = report.render().unwrap();
    let root = Element::parse(format!("<categories>{xml}</categories>").as_bytes()).unwrap();
    assert_eq!(root.children.len(), 5);
    assert!(xml.contains("Agent &amp; Endpoint"));
    assert!(xml.contains("<enc-state>unencrypted</enc-state>"));
    assert!(xml.contains("<last-full-scan-time>n/a</last-full-scan-time>"));
    assert!(!xml.contains("missing-patches"));
    assert!(!xml.contains("McAfee"));
  }

  #[test]
  fn missing_patch_severity_is_unknown_without_an_observed_msrc_rating() {
    for category in ["driver", "update", "security_update", "critical_update"] {
      let mut patch = MissingPatch {
        category: category.into(),
        ..Default::default()
      };
      assert_eq!(patch.severity(), -1);
      patch.msrc_severity = Some("Critical".into());
      assert_eq!(patch.severity(), 3);
      patch.msrc_severity = Some("Unrecognized".into());
      assert_eq!(patch.severity(), -1);
    }
  }

  #[test]
  fn successful_update_search_distinguishes_empty_and_missing_updates() {
    let mut report = WindowsReport {
      missing_patches: Some(vec![]),
      ..Default::default()
    };
    assert!(report.render().unwrap().contains("<missing-patches />"));
    report.missing_patches = Some(vec![MissingPatch {
      title: "Security update".into(),
      kb: "123".into(),
      ..Default::default()
    }]);
    let xml = report.render().unwrap();
    assert!(xml.contains("<kb-article-id>123</kb-article-id>"));
    assert!(xml.contains("<is-installed>no</is-installed>"));
  }
}
