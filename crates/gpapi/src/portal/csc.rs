use std::{
  collections::HashMap,
  fs,
  process::Command,
  time::{SystemTime, UNIX_EPOCH},
};

use xmltree::{Element, EmitterConfig, XMLNode};

use crate::{
  gp_params::GpParams,
  utils::{host_utils, xml::ElementExt},
};

const DIGEST_PLACEHOLDER: &str = "__#_PAN_CSC_DATA_DIGEST_#__";

pub(super) struct CscRequest {
  pub(super) auth_cookie: String,
  pub(super) config_digest: String,
  pub(super) csc_digest: String,
  pub(super) csc_data: String,
  host_id: String,
  serial_number: String,
}

pub(super) trait PreferenceReader {
  fn read_preference(&self, domain: &str, key: &str) -> Option<String>;
}

struct SystemPreferenceReader;

impl PreferenceReader for SystemPreferenceReader {
  fn read_preference(&self, domain: &str, key: &str) -> Option<String> {
    read_macos_preference(domain, key)
  }
}

pub(super) fn swg_nonce() -> String {
  let millis = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_millis())
    .unwrap_or_default();

  (millis % 2_147_483_647).to_string()
}

pub(super) fn is_config_criteria(root: &Element) -> bool {
  root.name == "config-criteria"
}

pub(super) fn build_csc_request(root: &Element, username: &str, gp_params: &GpParams) -> anyhow::Result<CscRequest> {
  let reader = SystemPreferenceReader;
  build_csc_request_with_reader(root, username, gp_params, &reader)
}

pub(super) fn build_csc_request_with_reader(
  root: &Element,
  username: &str,
  gp_params: &GpParams,
  reader: &dyn PreferenceReader,
) -> anyhow::Result<CscRequest> {
  let auth_cookie = root
    .descendant_text("portal-csc-auth-cookie")
    .filter(|value| !value.is_empty())
    .ok_or_else(|| anyhow::anyhow!("CSC criteria response does not contain portal-csc-auth-cookie"))?;
  let config_digest = root.descendant_text("config-digest").unwrap_or_default();
  let host_id = collect_host_id(gp_params);
  let serial_number = collect_serial_number();

  let csc_data = build_csc_data(
    root,
    &auth_cookie,
    username,
    gp_params,
    &host_id,
    &serial_number,
    DIGEST_PLACEHOLDER,
    reader,
  )?;
  let csc_digest = format!("{:x}", md5::compute(csc_data.as_bytes()));
  let csc_data = csc_data.replace(DIGEST_PLACEHOLDER, &csc_digest);

  Ok(CscRequest {
    auth_cookie,
    config_digest,
    csc_digest,
    csc_data,
    host_id,
    serial_number,
  })
}

pub(super) fn csc_params<'a>(
  req: &'a CscRequest,
  username: &'a str,
  gp_params: &'a GpParams,
  swg_nonce: &'a str,
) -> HashMap<&'a str, &'a str> {
  let mut params = HashMap::new();

  params.insert("user", username);
  params.insert("clientVer", "4100");
  params.insert("clientos", gp_params.client_os());
  params.insert("os-version", gp_params.os_version().unwrap_or_default());
  params.insert("hostid", req.host_id.as_str());
  params.insert("serialno", req.serial_number.as_str());
  params.insert("portal-cc-auth-cookie", req.auth_cookie.as_str());
  params.insert("config-digest", req.config_digest.as_str());
  params.insert("csc-digest", req.csc_digest.as_str());
  params.insert("csc-data", req.csc_data.as_str());
  params.insert("swg-auth-token", "0");
  params.insert("swg-nonce", swg_nonce);

  params
}

fn build_csc_data(
  criteria: &Element,
  auth_cookie: &str,
  username: &str,
  gp_params: &GpParams,
  host_id: &str,
  serial_number: &str,
  csc_digest: &str,
  reader: &dyn PreferenceReader,
) -> anyhow::Result<String> {
  let mut root = Element::new("hip-report");
  push_text(&mut root, "portal-csc-auth-cookie", auth_cookie);
  push_text(&mut root, "user-name", &urlencoding::encode(username));
  push_text(&mut root, "client-os", &client_os_report_value(gp_params));
  push_text(&mut root, "host-id", host_id);
  push_text(&mut root, "device-serial-number", serial_number);
  push_text(&mut root, "csc-digest", csc_digest);
  root.children.push(XMLNode::Element(Element::new("categories")));
  root
    .children
    .push(XMLNode::Element(build_custom_checks(criteria, gp_params, reader)));

  let mut bytes = Vec::new();
  root.write_with_config(
    &mut bytes,
    EmitterConfig::new()
      .perform_indent(true)
      .write_document_declaration(true),
  )?;

  String::from_utf8(bytes).map_err(Into::into)
}

fn build_custom_checks(criteria: &Element, gp_params: &GpParams, reader: &dyn PreferenceReader) -> Element {
  let mut custom_checks = Element::new("custom-checks");
  let Some(requested_checks) = criteria.descendant("custom-checks") else {
    return custom_checks;
  };

  if let Some(mac_os) = requested_checks.child("mac-os") {
    if let Some(plist) = mac_os.child("plist") {
      custom_checks.children.push(XMLNode::Element(build_plist_checks(
        plist,
        gp_params.client_os() == "Mac",
        reader,
      )));
    }
  }

  custom_checks
}

fn build_plist_checks(plist: &Element, can_read: bool, reader: &dyn PreferenceReader) -> Element {
  let mut out = Element::new("plist");

  for entry in plist.children("entry") {
    let domain = entry.attr("name").unwrap_or_default();
    let mut out_entry = Element::new("entry");
    out_entry.attributes.insert("name".to_string(), domain.to_string());

    let keys = plist_entry_keys(entry);
    let values: Vec<_> = keys
      .iter()
      .map(|key| {
        let value = if can_read {
          reader.read_preference(domain, key)
        } else {
          None
        };
        (key.as_str(), value)
      })
      .collect();
    let plist_exists = values.iter().any(|(_, value)| value.is_some());

    push_text(&mut out_entry, "exist", yes_no(plist_exists));
    push_text(&mut out_entry, "value", "");

    let mut preference_value = Element::new("preference-value");
    for (key, value) in values {
      let mut pref_entry = Element::new("entry");
      pref_entry.attributes.insert("name".to_string(), key.to_string());
      push_text(&mut pref_entry, "exist", yes_no(value.is_some()));
      push_text(&mut pref_entry, "value", value.as_deref().unwrap_or_default());
      preference_value.children.push(XMLNode::Element(pref_entry));
    }

    out_entry.children.push(XMLNode::Element(preference_value));
    out.children.push(XMLNode::Element(out_entry));
  }

  out
}

fn plist_entry_keys(entry: &Element) -> Vec<String> {
  let keys: Vec<String> = entry
    .descendants("member")
    .into_iter()
    .filter_map(|element| element.get_text())
    .map(|text| text.to_string())
    .collect();

  if keys.is_empty() {
    entry
      .attr("name")
      .map(|name| vec![name.to_string()])
      .unwrap_or_default()
  } else {
    keys
  }
}

fn collect_host_id(gp_params: &GpParams) -> String {
  match gp_params.client_os() {
    "Mac" => netdev::get_default_interface()
      .ok()
      .and_then(|iface| iface.mac_addr.map(|mac| mac.address()))
      .unwrap_or_default(),
    "Windows" => host_utils::get_machine_id().to_string(),
    _ => host_utils::derive_uuid(&[]),
  }
}

fn collect_serial_number() -> String {
  collect_macos_serial_number()
    .or_else(collect_linux_serial_number)
    .or_else(collect_windows_serial_number)
    .unwrap_or_default()
}

fn collect_macos_serial_number() -> Option<String> {
  let output = Command::new("ioreg")
    .args(["-rd1", "-c", "IOPlatformExpertDevice"])
    .output()
    .ok()?;
  let stdout = String::from_utf8(output.stdout).ok()?;

  stdout.lines().find_map(|line| {
    line
      .split_once("IOPlatformSerialNumber")
      .and_then(|(_, value)| value.split_once('='))
      .map(|(_, value)| value.trim().trim_matches('"').to_string())
      .filter(|value| !value.is_empty())
  })
}

fn collect_linux_serial_number() -> Option<String> {
  ["/sys/class/dmi/id/product_serial", "/sys/class/dmi/id/product_uuid"]
    .into_iter()
    .filter_map(|path| fs::read_to_string(path).ok())
    .map(|value| value.trim().to_string())
    .find(|value| !value.is_empty() && value != "None" && value != "To Be Filled By O.E.M.")
}

fn collect_windows_serial_number() -> Option<String> {
  let output = Command::new("wmic")
    .args(["bios", "get", "serialnumber"])
    .output()
    .ok()?;
  let stdout = String::from_utf8(output.stdout).ok()?;

  stdout
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty() && *line != "SerialNumber")
    .map(str::to_string)
    .next()
}

fn read_macos_preference(domain: &str, key: &str) -> Option<String> {
  if cfg!(not(target_os = "macos")) {
    return None;
  }

  let output = Command::new("defaults").args(["read", domain, key]).output().ok()?;
  if !output.status.success() {
    return None;
  }

  String::from_utf8(output.stdout)
    .ok()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn client_os_report_value(gp_params: &GpParams) -> String {
  gp_params
    .os_version()
    .unwrap_or_else(|| gp_params.client_os())
    .replace(' ', "+")
}

fn push_text(parent: &mut Element, name: &str, value: &str) {
  let mut element = Element::new(name);
  element.children.push(XMLNode::Text(value.to_string()));
  parent.children.push(XMLNode::Element(element));
}

fn yes_no(value: bool) -> &'static str {
  if value { "yes" } else { "no" }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::gp_params::{ClientOs, GpParams};

  struct TestPreferenceReader {
    values: HashMap<String, String>,
  }

  impl PreferenceReader for TestPreferenceReader {
    fn read_preference(&self, domain: &str, key: &str) -> Option<String> {
      self.values.get(&format!("{domain}:{key}")).cloned()
    }
  }

  fn parse_xml(xml: &str) -> Element {
    Element::parse(xml.as_bytes()).unwrap()
  }

  fn gp_params(client_os: ClientOs) -> GpParams {
    GpParams::builder()
      .client_os(client_os)
      .os_version(Some("Apple Mac OS X 26.4.1".to_string()))
      .build()
  }

  #[test]
  fn detects_config_criteria_response() {
    assert!(is_config_criteria(&parse_xml("<config-criteria/>")));
    assert!(!is_config_criteria(&parse_xml("<policy/>")));
  }

  #[test]
  fn builds_macos_plist_csc_report_with_existing_value() {
    let root = parse_xml(
      r#"<config-criteria>
        <custom-checks>
          <mac-os>
            <plist>
              <entry name="com.paloaltonetworks.GlobalProtect.settings">
                <key><member>HipID</member></key>
              </entry>
            </plist>
          </mac-os>
        </custom-checks>
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestPreferenceReader {
      values: HashMap::from([(
        "com.paloaltonetworks.GlobalProtect.settings:HipID".to_string(),
        "MicroStrategyGP".to_string(),
      )]),
    };

    let req = build_csc_request_with_reader(&root, "alice@example.com", &gp_params(ClientOs::Mac), &reader).unwrap();

    assert_eq!(req.auth_cookie, "cookie");
    assert!(req.csc_data.contains("<user-name>alice%40example.com</user-name>"));
    assert!(req.csc_data.contains("<client-os>Apple+Mac+OS+X+26.4.1</client-os>"));
    assert!(req.csc_data.contains("<exist>yes</exist>"));
    assert!(req.csc_data.contains("<value>MicroStrategyGP</value>"));
    assert!(
      req
        .csc_data
        .contains(&format!("<csc-digest>{}</csc-digest>", req.csc_digest))
    );
    assert!(!req.csc_data.contains(DIGEST_PLACEHOLDER));
  }

  #[test]
  fn reports_missing_macos_plist_value() {
    let root = parse_xml(
      r#"<config-criteria>
        <custom-checks>
          <mac-os>
            <plist>
              <entry name="com.example"><key><member>Missing</member></key></entry>
            </plist>
          </mac-os>
        </custom-checks>
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestPreferenceReader { values: HashMap::new() };

    let req = build_csc_request_with_reader(&root, "alice", &gp_params(ClientOs::Mac), &reader).unwrap();

    assert!(req.csc_data.contains("<exist>no</exist>"));
    assert!(req.csc_data.contains("<value />") || req.csc_data.contains("<value></value>"));
  }

  #[test]
  fn unsupported_client_os_reports_macos_plist_missing() {
    let root = parse_xml(
      r#"<config-criteria>
        <custom-checks>
          <mac-os>
            <plist>
              <entry name="com.example"><key><member>HipID</member></key></entry>
            </plist>
          </mac-os>
        </custom-checks>
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestPreferenceReader {
      values: HashMap::from([("com.example:HipID".to_string(), "present".to_string())]),
    };

    let req = build_csc_request_with_reader(&root, "alice", &gp_params(ClientOs::Linux), &reader).unwrap();

    assert!(req.csc_data.contains("<exist>no</exist>"));
    assert!(!req.csc_data.contains("present"));
  }

  #[test]
  fn builds_getconfig_csc_params() {
    let req = CscRequest {
      auth_cookie: "cookie".to_string(),
      config_digest: "config-digest".to_string(),
      csc_digest: "csc-digest".to_string(),
      csc_data: "<hip-report><host-id>host</host-id><device-serial-number>serial</device-serial-number></hip-report>"
        .to_string(),
      host_id: "host".to_string(),
      serial_number: "serial".to_string(),
    };
    let nonce = "123";
    let gp_params = gp_params(ClientOs::Mac);
    let params = csc_params(&req, "alice", &gp_params, nonce);

    assert_eq!(params.get("user"), Some(&"alice"));
    assert_eq!(params.get("hostid"), Some(&"host"));
    assert_eq!(params.get("serialno"), Some(&"serial"));
    assert_eq!(params.get("portal-cc-auth-cookie"), Some(&"cookie"));
    assert_eq!(params.get("config-digest"), Some(&"config-digest"));
    assert_eq!(params.get("csc-digest"), Some(&"csc-digest"));
    assert_eq!(params.get("csc-data"), Some(&req.csc_data.as_str()));
    assert_eq!(params.get("swg-auth-token"), Some(&"0"));
    assert_eq!(params.get("swg-nonce"), Some(&"123"));
  }
}
