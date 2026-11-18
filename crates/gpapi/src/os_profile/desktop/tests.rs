use super::*;

#[test]
fn controlled_identity_collection_propagates_cancellation() {
  let control = || Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
  let error = collect_identity_with_control(&control).unwrap_err();
  assert_eq!(error.kind(), io::ErrorKind::Interrupted);
  let error = collect_with_host_id_and_control(Some("supplied"), &control).unwrap_err();
  assert_eq!(error.kind(), io::ErrorKind::Interrupted);
}

#[test]
fn collect_produces_non_empty_identity() {
  let identity = collect_identity();

  assert!(!identity.computer().is_empty());
  assert!(!identity.host_id().is_empty());
  assert!(!identity.serialno().is_empty());
  assert!(!identity.mac_addr().is_empty());
  assert_mac(identity.mac_addr(), ':');
}

#[test]
fn collect_with_host_id_uses_provided_host_id() {
  let identity = collect_with_host_id(Some("provided-host-id"));

  assert_eq!(identity.host_id(), "provided-host-id");
  assert!(!identity.computer().is_empty());
  assert!(!identity.serialno().is_empty());
  assert!(!identity.mac_addr().is_empty());
}

#[test]
fn collect_with_blank_host_id_falls_back_to_runtime_host_id() {
  let identity = collect_with_host_id(Some(" "));

  assert!(!identity.host_id().is_empty());
  assert_ne!(identity.host_id(), " ");
}

#[test]
fn resolve_host_id_does_not_collect_when_override_is_provided() {
  let host_id = resolve_host_id(Some("provided-host-id"), || {
    panic!("runtime host id should not be collected")
  });

  assert_eq!(host_id, "provided-host-id");
}

#[test]
fn host_identity_normalizes_mac_to_lowercase_colon_format() {
  let identity = identity_from_parts(
    "computer".to_string(),
    "host-id".to_string(),
    "serial-number".to_string(),
    "AA-BB-CC-DD-EE-FF".to_string(),
  );

  assert_eq!(identity.mac_addr(), "aa:bb:cc:dd:ee:ff");
}

#[test]
fn native_snapshot_uses_host_id_as_derivation_seed() {
  let native =
    RuntimeNativeHostIdentity::collect_with_control(None, &CollectionBudget::new(Duration::from_secs(60))).unwrap();

  assert!(!native.host_id.is_empty());
}

#[test]
fn serialization_keeps_globalprotect_field_names() {
  let identity = identity_from_parts(
    "computer".to_string(),
    "host-id".to_string(),
    "serial-number".to_string(),
    "aa:bb:cc:dd:ee:ff".to_string(),
  );
  let value = serde_json::to_value(identity).unwrap();

  assert_eq!(value["hostId"], "host-id");
  assert_eq!(value["computer"], "computer");
  assert!(value.get("osVersion").is_none());
  assert!(value.get("softwareVersion").is_none());
  assert_eq!(value["serialno"], "serial-number");
  assert_eq!(value["macAddr"], "aa:bb:cc:dd:ee:ff");
}

#[test]
fn native_identity_falls_back_to_derived_values_when_missing() {
  let native = NativeHostIdentitySnapshot {
    host_id: "native-host-id".to_string(),
    computer: "native-computer".to_string(),
    serial_number: None,
    mac_address: None,
  };

  let identity = native.into_host_identity();

  assert_eq!(identity.computer(), "native-computer");
  assert_eq!(identity.host_id(), "native-host-id");
  assert!(identity.serialno().starts_with("VMware-"));
  assert_mac(identity.mac_addr(), ':');
}

#[test]
fn derived_serial_uses_vmware_uuid_byte_order_when_host_id_is_uuid() {
  let identity = identity_from_parts(
    "computer".to_string(),
    "5a784d56-6461-19ac-9ea9-d36a3b9c6cef".to_string(),
    String::new(),
    "aa:bb:cc:dd:ee:ff".to_string(),
  );

  assert_eq!(
    identity.serialno(),
    "VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef"
  );
}

#[test]
fn empty_host_id_falls_back_to_non_empty_value() {
  let identity = identity_from_parts(String::new(), String::new(), String::new(), String::new());

  assert!(!identity.host_id().is_empty());
  assert!(!identity.computer().is_empty());
  assert!(!identity.serialno().is_empty());
  assert!(!identity.mac_addr().is_empty());
}

fn assert_mac(value: &str, separator: char) {
  let parts: Vec<&str> = value.split(separator).collect();
  assert_eq!(parts.len(), 6, "MAC should have 6 octets");
  for part in &parts {
    assert_eq!(part.len(), 2);
    assert!(part.chars().all(|c| c.is_ascii_hexdigit()));
  }
}

#[test]
fn builder_preserves_supplied_hardware_fields() {
  let expected = collect_identity();
  let profile = collect_profile(
    OsProfileBuilder::new(runtime_client_os()).host_identity(expected.clone()),
    None,
  );

  assert!(
    profile
      .host_identity()
      .computer()
      .eq_ignore_ascii_case(expected.computer())
  );
  assert_eq!(profile.host_identity().serialno(), expected.serialno());
  assert_eq!(profile.host_identity().mac_addr(), expected.mac_addr());
  if runtime_client_os() != ClientOs::Windows {
    assert_eq!(profile.host_identity().host_id(), expected.host_id());
  }
}

#[test]
fn builder_host_id_override_uses_supplied_runtime_identity_seed() {
  let profile = collect_profile(OsProfileBuilder::new(runtime_client_os()), Some("provided-host-id"));

  assert_eq!(profile.host_identity().host_id(), "provided-host-id");
}

#[test]
fn builder_host_id_override_is_used_as_projected_identity_seed() {
  let client_os = match runtime_client_os() {
    ClientOs::Linux => ClientOs::Mac,
    ClientOs::Mac | ClientOs::Windows | ClientOs::Ios => ClientOs::Linux,
  };

  let first = collect_profile(OsProfileBuilder::new(client_os), Some("provided-host-id"));
  let second = collect_profile(OsProfileBuilder::new(client_os), Some("provided-host-id"));

  assert_eq!(first.host_identity().host_id(), "provided-host-id");
  assert_eq!(first.host_id(), second.host_id());
  assert_ne!(first.host_id(), "provided-host-id");
}

#[test]
fn controlled_profile_collection_propagates_cancellation_and_deadline() {
  let cancelled = || Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
  let error = collect_profile_with_control(OsProfileBuilder::new(ClientOs::Mac), None, &cancelled).unwrap_err();
  assert_eq!(error.kind(), io::ErrorKind::Interrupted);
  let expired = CollectionBudget::new(Duration::ZERO);
  let error = collect_profile_with_control(OsProfileBuilder::new(ClientOs::Linux), None, &expired).unwrap_err();
  assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[cfg(windows)]
#[test]
fn native_windows_profile_uses_installation_id_and_emulation_keeps_its_seed() {
  let identity = HostIdentity::from_parts(
    "test-host".into(),
    "hardware-seed".into(),
    "serial".into(),
    "00:11:22:33:44:55".into(),
  );
  let control = CollectionBudget::new(Duration::from_secs(60));
  let native = collect_profile_with_control(
    OsProfile::builder(ClientOs::Windows).host_identity(identity.clone()),
    None,
    &control,
  )
  .unwrap();
  // Read independently through the registry CLI to check the native API's source and view.
  let output = std::process::Command::new("reg")
    .args([
      "query",
      r"HKLM\SOFTWARE\Microsoft\Cryptography",
      "/v",
      "MachineGuid",
      "/reg:64",
    ])
    .output()
    .unwrap();
  assert!(output.status.success());
  let text = String::from_utf8(output.stdout).unwrap();
  let expected = text
    .lines()
    .find_map(|line| {
      let mut parts = line.split_whitespace();
      (parts.next()? == "MachineGuid").then(|| parts.last().unwrap())
    })
    .unwrap();
  assert_eq!(native.host_id(), expected);
  assert_eq!(native.host_identity().host_id(), expected);
  assert!(native.computer().eq_ignore_ascii_case(&fallback_hostname()));
  let with_host_id = collect_profile_with_control(
    OsProfile::builder(ClientOs::Windows).host_identity(identity.clone()),
    Some("explicit-host-id"),
    &control,
  )
  .unwrap();
  assert_eq!(with_host_id.host_id(), "explicit-host-id");
  assert_eq!(with_host_id.computer(), native.computer());
  let overridden = collect_profile_with_control(
    OsProfile::builder(ClientOs::Windows)
      .host_identity(identity.clone())
      .computer_name_override("Custom-Host"),
    None,
    &control,
  )
  .unwrap();
  assert_eq!(overridden.computer(), "Custom-Host");

  for client_os in [ClientOs::Mac, ClientOs::Linux] {
    let builder = OsProfile::builder(client_os).host_identity(identity.clone());
    let expected = builder.clone().build(HostSnapshot {
      runtime_os: ClientOs::Windows,
      identity: identity.clone(),
      os_version: String::new(),
      software_version: String::new(),
    });
    let actual = collect_profile_with_control(builder, None, &control).unwrap();
    assert_eq!(actual.host_identity(), &identity);
    assert_eq!(actual.host_id(), expected.host_id());
    assert_eq!(actual.serialno(), expected.serialno());
    assert_eq!(actual.mac_addr(), expected.mac_addr());
  }
}
