use std::{
  collections::HashMap,
  fmt::Write,
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
  summary: CscBuildSummary,
  host_id: String,
  serial_number: String,
}

impl CscRequest {
  pub(super) fn summary(&self) -> String {
    format!(
      "digest_len={}, xml_len={}, plist_entries={}, plist_present={}, plist_missing={}, process_entries={}, process_present={}, process_missing={}",
      self.csc_digest.len(),
      self.csc_data.len(),
      self.summary.plist_entries,
      self.summary.plist_present,
      self.summary.plist_missing,
      self.summary.process_entries,
      self.summary.process_present,
      self.summary.process_missing,
    )
  }
}

#[derive(Default)]
struct CscBuildSummary {
  plist_entries: usize,
  plist_present: usize,
  plist_missing: usize,
  process_entries: usize,
  process_present: usize,
  process_missing: usize,
}

struct CscDataInput<'a> {
  criteria: &'a Element,
  auth_cookie: &'a str,
  username: &'a str,
  gp_params: &'a GpParams,
  host_id: &'a str,
  serial_number: &'a str,
  csc_digest: &'a str,
}

pub(super) trait CscDataReader {
  fn read_preference(&self, domain: &str, key: &str) -> Option<String>;
  fn process_exists(&self, name: &str) -> bool;
}

struct SystemCscDataReader;

impl CscDataReader for SystemCscDataReader {
  fn read_preference(&self, domain: &str, key: &str) -> Option<String> {
    read_macos_preference(domain, key)
  }

  fn process_exists(&self, name: &str) -> bool {
    linux_process_names().iter().any(|process_name| process_name == name)
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
  let reader = SystemCscDataReader;
  build_csc_request_with_reader(root, username, gp_params, &reader)
}

pub(super) fn build_csc_request_with_reader(
  root: &Element,
  username: &str,
  gp_params: &GpParams,
  reader: &dyn CscDataReader,
) -> anyhow::Result<CscRequest> {
  let auth_cookie = root
    .descendant_text("portal-csc-auth-cookie")
    .filter(|value| !value.is_empty())
    .ok_or_else(|| anyhow::anyhow!("CSC criteria response does not contain portal-csc-auth-cookie"))?;
  let config_digest = root.descendant_text("config-digest").unwrap_or_default();
  let host_id = collect_host_id(gp_params);
  let serial_number = collect_serial_number();

  let mut summary = CscBuildSummary::default();
  let csc_data = build_csc_data(
    CscDataInput {
      criteria: root,
      auth_cookie: &auth_cookie,
      username,
      gp_params,
      host_id: &host_id,
      serial_number: &serial_number,
      csc_digest: DIGEST_PLACEHOLDER,
    },
    reader,
    &mut summary,
  )?;
  let csc_digest = pan_md5_hex(csc_data.as_bytes());
  let csc_data = csc_data.replace(DIGEST_PLACEHOLDER, &csc_digest);

  Ok(CscRequest {
    auth_cookie,
    config_digest,
    csc_digest,
    csc_data,
    summary,
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
  input: CscDataInput<'_>,
  reader: &dyn CscDataReader,
  summary: &mut CscBuildSummary,
) -> anyhow::Result<String> {
  let mut root = Element::new("hip-report");
  push_text(&mut root, "portal-csc-auth-cookie", input.auth_cookie);
  push_text(&mut root, "user-name", &urlencoding::encode(input.username));
  push_text(&mut root, "client-os", &client_os_report_value(input.gp_params));
  push_text(&mut root, "host-id", input.host_id);
  push_text(&mut root, "device-serial-number", input.serial_number);
  push_text(&mut root, "csc-digest", input.csc_digest);
  root.children.push(XMLNode::Element(Element::new("categories")));
  root.children.push(XMLNode::Element(build_custom_checks(
    input.criteria,
    input.gp_params,
    reader,
    summary,
  )));

  let mut bytes = Vec::new();
  root.write_with_config(
    &mut bytes,
    EmitterConfig::new()
      .perform_indent(true)
      .write_document_declaration(true),
  )?;

  String::from_utf8(bytes).map_err(Into::into)
}

fn build_custom_checks(
  criteria: &Element,
  gp_params: &GpParams,
  reader: &dyn CscDataReader,
  summary: &mut CscBuildSummary,
) -> Element {
  let mut custom_checks = Element::new("custom-checks");
  let Some(requested_checks) = criteria.descendant("custom-checks") else {
    return custom_checks;
  };

  if gp_params.client_os() == "Mac"
    && let Some(plist) = requested_checks
      .child("mac-os")
      .and_then(|mac_os| mac_os.child("plist"))
  {
    custom_checks
      .children
      .push(XMLNode::Element(build_plist_checks(plist, gp_params, reader, summary)));
  }

  if gp_params.client_os() == "Linux"
    && let Some(process_list) = requested_checks
      .child("linux")
      .and_then(|linux| linux.child("process-list"))
      .or_else(|| requested_checks.child("process-list"))
  {
    custom_checks.children.push(XMLNode::Element(build_process_list_checks(
      process_list,
      reader,
      summary,
    )));
  }

  custom_checks
}

fn build_plist_checks(
  plist: &Element,
  gp_params: &GpParams,
  reader: &dyn CscDataReader,
  summary: &mut CscBuildSummary,
) -> Element {
  let mut out = Element::new("plist");

  for entry in plist.children("entry") {
    let domain = entry.attr("name").unwrap_or_default();
    let mut out_entry = Element::new("entry");
    out_entry.attributes.insert("name".to_string(), domain.to_string());

    let keys = plist_entry_keys(entry);
    let values: Vec<_> = keys
      .iter()
      .map(|key| {
        let value = gp_params
          .csc_preference_override(domain, key)
          .map(str::to_string)
          .or_else(|| reader.read_preference(domain, key));
        (key.as_str(), value)
      })
      .collect();
    let plist_exists = values.iter().any(|(_, value)| value.is_some());
    summary.plist_entries += keys.len();
    summary.plist_present += values.iter().filter(|(_, value)| value.is_some()).count();
    summary.plist_missing += values.iter().filter(|(_, value)| value.is_none()).count();

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

fn build_process_list_checks(
  process_list: &Element,
  reader: &dyn CscDataReader,
  summary: &mut CscBuildSummary,
) -> Element {
  let mut out = Element::new("process-list");

  for process_name in process_list_names(process_list) {
    let exists = reader.process_exists(&process_name);
    summary.process_entries += 1;
    if exists {
      summary.process_present += 1;
    } else {
      summary.process_missing += 1;
    }

    let mut entry = Element::new("entry");
    entry.attributes.insert("name".to_string(), process_name.clone());
    push_text(&mut entry, "exist", yes_no(exists));
    push_text(&mut entry, "value", if exists { &process_name } else { "" });
    out.children.push(XMLNode::Element(entry));
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

fn process_list_names(process_list: &Element) -> Vec<String> {
  let mut names: Vec<String> = process_list
    .descendants("member")
    .into_iter()
    .filter_map(|element| element.get_text())
    .map(|text| text.trim().to_string())
    .filter(|text| !text.is_empty())
    .collect();

  names.extend(
    process_list
      .children("entry")
      .into_iter()
      .filter_map(|entry| entry.attr("name"))
      .map(str::to_string),
  );

  names.sort();
  names.dedup();
  names
}

fn pan_md5_hex(value: &[u8]) -> String {
  let digest = md5::compute(value);
  let mut out = String::new();
  for byte in digest.0 {
    write!(&mut out, "{:x}", byte).expect("writing to String should not fail");
  }
  out
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

fn linux_process_names() -> Vec<String> {
  let Ok(entries) = fs::read_dir("/proc") else {
    return Vec::new();
  };

  entries
    .filter_map(Result::ok)
    .filter_map(|entry| {
      entry
        .file_name()
        .to_str()
        .filter(|name| name.chars().all(|ch| ch.is_ascii_digit()))
        .map(|_| entry.path().join("stat"))
    })
    .filter_map(|path| fs::read_to_string(path).ok())
    .filter_map(|stat| {
      let start = stat.find('(')?;
      let end = stat.rfind(')')?;
      stat.get(start + 1..end).map(str::to_string)
    })
    .filter(|name| !name.is_empty())
    .collect()
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

  struct TestCscDataReader {
    values: HashMap<String, String>,
    processes: Vec<String>,
  }

  impl CscDataReader for TestCscDataReader {
    fn read_preference(&self, domain: &str, key: &str) -> Option<String> {
      self.values.get(&format!("{domain}:{key}")).cloned()
    }

    fn process_exists(&self, name: &str) -> bool {
      self.processes.iter().any(|process| process == name)
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
              <entry name="com.example.settings">
                <key><member>ExampleKey</member></key>
              </entry>
            </plist>
          </mac-os>
        </custom-checks>
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestCscDataReader {
      values: HashMap::from([(
        "com.example.settings:ExampleKey".to_string(),
        "ExampleValue".to_string(),
      )]),
      processes: Vec::new(),
    };

    let req = build_csc_request_with_reader(&root, "alice@example.com", &gp_params(ClientOs::Mac), &reader).unwrap();

    assert_eq!(req.auth_cookie, "cookie");
    assert!(req.csc_data.contains("<user-name>alice%40example.com</user-name>"));
    assert!(req.csc_data.contains("<client-os>Apple+Mac+OS+X+26.4.1</client-os>"));
    assert!(req.csc_data.contains("<exist>yes</exist>"));
    assert!(req.csc_data.contains("<value>ExampleValue</value>"));
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
    let reader = TestCscDataReader {
      values: HashMap::new(),
      processes: Vec::new(),
    };

    let req = build_csc_request_with_reader(&root, "alice", &gp_params(ClientOs::Mac), &reader).unwrap();

    assert!(req.csc_data.contains("<exist>no</exist>"));
    assert!(req.csc_data.contains("<value />") || req.csc_data.contains("<value></value>"));
  }

  #[test]
  fn macos_plist_override_works_without_native_preference() {
    let root = parse_xml(
      r#"<config-criteria>
        <custom-checks>
          <mac-os>
            <plist>
              <entry name="com.example.settings"><key><member>ExampleKey</member></key></entry>
            </plist>
          </mac-os>
        </custom-checks>
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestCscDataReader {
      values: HashMap::new(),
      processes: Vec::new(),
    };
    let gp_params = GpParams::builder()
      .client_os(ClientOs::Mac)
      .os_version(Some("Apple Mac OS X 26.4.1".to_string()))
      .csc_preference_override("com.example.settings", "ExampleKey", "ExampleValue")
      .build();

    let req = build_csc_request_with_reader(&root, "alice", &gp_params, &reader).unwrap();

    assert!(req.csc_data.contains("<exist>yes</exist>"));
    assert!(req.csc_data.contains("<value>ExampleValue</value>"));
  }

  #[test]
  fn unsupported_client_os_ignores_macos_plist_branch() {
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
    let reader = TestCscDataReader {
      values: HashMap::from([("com.example:HipID".to_string(), "present".to_string())]),
      processes: Vec::new(),
    };

    let req = build_csc_request_with_reader(&root, "alice", &gp_params(ClientOs::Linux), &reader).unwrap();

    assert!(!req.csc_data.contains("<plist>"));
    assert!(!req.csc_data.contains("<exist>no</exist>"));
    assert!(!req.csc_data.contains("present"));
  }

  #[test]
  fn builds_linux_process_list_report() {
    let root = parse_xml(
      r#"<config-criteria>
        <custom-checks>
          <linux>
            <process-list>
              <entry name="present-process" />
              <entry name="missing-process" />
            </process-list>
          </linux>
        </custom-checks>
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestCscDataReader {
      values: HashMap::new(),
      processes: vec!["present-process".to_string()],
    };

    let req = build_csc_request_with_reader(&root, "alice", &gp_params(ClientOs::Linux), &reader).unwrap();

    assert!(req.csc_data.contains(r#"<entry name="present-process">"#));
    assert!(req.csc_data.contains("<value>present-process</value>"));
    assert!(req.csc_data.contains(r#"<entry name="missing-process">"#));
    assert!(req.summary().contains("process_entries=2"));
    assert!(req.summary().contains("process_present=1"));
    assert!(req.summary().contains("process_missing=1"));
  }

  #[test]
  fn uses_pan_non_padded_md5_digest() {
    assert_eq!(pan_md5_hex(b""), "d41d8cd98f0b24e980998ecf8427e");
    assert_ne!(pan_md5_hex(b""), format!("{:x}", md5::compute(b"")));
  }

  #[test]
  fn builds_getconfig_csc_params() {
    let req = CscRequest {
      auth_cookie: "cookie".to_string(),
      config_digest: "config-digest".to_string(),
      csc_digest: "csc-digest".to_string(),
      csc_data: "<hip-report><host-id>host</host-id><device-serial-number>serial</device-serial-number></hip-report>"
        .to_string(),
      summary: CscBuildSummary::default(),
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

  #[test]
  fn generated_getconfig_csc_params_include_final_digest_and_data() {
    let root = parse_xml(
      r#"<config-criteria>
        <custom-checks />
        <portal-csc-auth-cookie>cookie</portal-csc-auth-cookie>
      </config-criteria>"#,
    );
    let reader = TestCscDataReader {
      values: HashMap::new(),
      processes: Vec::new(),
    };
    let gp_params = gp_params(ClientOs::Linux);
    let req = build_csc_request_with_reader(&root, "alice", &gp_params, &reader).unwrap();
    let params = csc_params(&req, "alice", &gp_params, "123");

    assert_eq!(params.get("csc-digest"), Some(&req.csc_digest.as_str()));
    assert_eq!(params.get("csc-data"), Some(&req.csc_data.as_str()));
    assert!(
      req
        .csc_data
        .contains(&format!("<csc-digest>{}</csc-digest>", req.csc_digest))
    );
    assert!(!req.csc_data.contains(DIGEST_PLACEHOLDER));
  }
}
