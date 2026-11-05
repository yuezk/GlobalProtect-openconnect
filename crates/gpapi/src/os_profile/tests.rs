use super::*;

#[test]
fn negotiated_os_version_is_used_in_profile_and_user_agent() {
  let identity = HostIdentity::from_parts(
    "device".into(),
    "host".into(),
    "serial".into(),
    "00:11:22:33:44:55".into(),
  );
  let profile = OsProfileBuilder::new(ClientOs::Mac)
    .host_identity(identity)
    .client_version("6.3.3")
    .os_version("negotiated-system-version")
    .build(test_snapshot());
  assert_eq!(profile.os_version(), "negotiated-system-version");
  assert!(profile.user_agent().contains("negotiated-system-version"));
}

#[test]
fn csc_support_defaults_match_official_profiles() {
  assert!(!ClientOs::Linux.default_csc_support());
  assert!(ClientOs::Mac.default_csc_support());
  assert!(ClientOs::Windows.default_csc_support());
}

#[test]
fn prelogin_param_location_matches_official_profiles() {
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .prelogin_param_location(),
    PreloginParamLocation::Body
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .prelogin_param_location(),
    PreloginParamLocation::Body
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .prelogin_param_location(),
    PreloginParamLocation::Query
  );
}

#[test]
fn embedded_portal_default_browser_is_minus_ten_for_all() {
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .portal_default_browser(PreloginBrowserMode::Embedded),
    "-10"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .portal_default_browser(PreloginBrowserMode::Embedded),
    "-10"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .portal_default_browser(PreloginBrowserMode::Embedded),
    "-10"
  );
}

#[test]
fn embedded_gateway_default_browser_is_zero_for_all() {
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .gateway_default_browser(PreloginBrowserMode::Embedded),
    "0"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .gateway_default_browser(PreloginBrowserMode::Embedded),
    "0"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .gateway_default_browser(PreloginBrowserMode::Embedded),
    "0"
  );
}

#[test]
fn external_default_browser_values_match_official_profiles() {
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .portal_default_browser(PreloginBrowserMode::External),
    "4"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .portal_default_browser(PreloginBrowserMode::External),
    "3"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .portal_default_browser(PreloginBrowserMode::External),
    "2"
  );

  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .gateway_default_browser(PreloginBrowserMode::External),
    "4"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .gateway_default_browser(PreloginBrowserMode::External),
    "3"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .gateway_default_browser(PreloginBrowserMode::External),
    "2"
  );
}

#[test]
fn saml_password_matches_official_profiles() {
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .saml_password(),
    "SAMLPASS"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .saml_password(),
    ""
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .saml_password(),
    ""
  );
}

#[test]
fn gateway_server_uses_resolved_ipv4_for_all() {
  assert!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .gateway_server_uses_resolved_ipv4()
  );
  assert!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .gateway_server_uses_resolved_ipv4()
  );
  assert!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .gateway_server_uses_resolved_ipv4()
  );
}

#[test]
fn kerberos_support_in_query_matches_official_profiles() {
  assert!(
    !OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .kerberos_support_in_query()
  );
  assert!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .kerberos_support_in_query()
  );
  assert!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .kerberos_support_in_query()
  );
}

#[test]
fn os_vendor_matches_official_profiles() {
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .os_vendor(),
    "Linux"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Mac).build(test_snapshot()).os_vendor(),
    "Apple"
  );
  assert_eq!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .os_vendor(),
    "Microsoft"
  );
}

#[test]
fn supports_macos_plist_csc_only_for_mac() {
  assert!(
    !OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .supports_macos_plist_csc()
  );
  assert!(
    OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .supports_macos_plist_csc()
  );
  assert!(
    !OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .supports_macos_plist_csc()
  );
}

#[test]
fn supports_linux_process_csc_only_for_linux() {
  assert!(
    OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .supports_linux_process_csc()
  );
  assert!(
    !OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .supports_linux_process_csc()
  );
  assert!(
    !OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .supports_linux_process_csc()
  );
}

#[test]
fn supports_windows_registry_csc_only_for_windows() {
  assert!(
    !OsProfileBuilder::new(ClientOs::Linux)
      .build(test_snapshot())
      .supports_windows_registry_csc()
  );
  assert!(
    !OsProfileBuilder::new(ClientOs::Mac)
      .build(test_snapshot())
      .supports_windows_registry_csc()
  );
  assert!(
    OsProfileBuilder::new(ClientOs::Windows)
      .build(test_snapshot())
      .supports_windows_registry_csc()
  );
}

// ─── User Agent Tests ───────────────────────────────────────────────────────

#[test]
fn user_agent_derived_format_without_override() {
  let profile = OsProfileBuilder::new(ClientOs::Linux)
    .client_version("6.0.0")
    .build(test_snapshot());

  assert_eq!(
    profile.user_agent(),
    format!("PAN GlobalProtect/6.0.0 ({})", defaults::os_version(ClientOs::Linux))
  );
}

#[test]
fn user_agent_explicit_override_returns_custom_value() {
  let profile = OsProfileBuilder::new(ClientOs::Linux)
    .client_version("6.0.0")
    .user_agent("custom/1.0")
    .build(test_snapshot());

  assert_eq!(profile.user_agent(), "custom/1.0");
}

#[test]
fn user_agent_empty_override_falls_back_to_derived() {
  let profile = OsProfileBuilder::new(ClientOs::Linux)
    .client_version("6.0.0")
    .user_agent("")
    .build(test_snapshot());

  assert_eq!(
    profile.user_agent(),
    format!("PAN GlobalProtect/6.0.0 ({})", defaults::os_version(ClientOs::Linux))
  );
}

#[test]
fn user_agent_whitespace_only_override_falls_back_to_derived() {
  let profile = OsProfileBuilder::new(ClientOs::Linux)
    .client_version("6.0.0")
    .user_agent("   ")
    .build(test_snapshot());

  assert_eq!(
    profile.user_agent(),
    format!("PAN GlobalProtect/6.0.0 ({})", defaults::os_version(ClientOs::Linux))
  );
}

#[test]
fn user_agent_derived_is_non_empty_for_all_client_os_variants() {
  for client_os in [ClientOs::Linux, ClientOs::Windows, ClientOs::Mac] {
    let profile = OsProfileBuilder::new(client_os).build(test_snapshot());
    assert!(
      !profile.user_agent().is_empty(),
      "user_agent() should be non-empty for {:?}",
      client_os
    );
  }
}

#[test]
fn native_webview_user_agent_uses_native_default() {
  let profile = OsProfileBuilder::new(ClientOs::Mac).build(test_snapshot());
  let user_agent = profile.webview_user_agent();

  assert_eq!(
    user_agent,
    WebviewUserAgent::Native {
      prefix: profile.user_agent().to_string(),
      transform: target::webview_user_agent_transform(ClientOs::Mac)
    }
  );
}

#[test]
fn projected_linux_webview_user_agent_uses_linux_default() {
  if ClientOs::Mac == ClientOs::Linux {
    return;
  }

  let client_os = ClientOs::Linux;
  let profile = OsProfileBuilder::new(client_os).build(test_snapshot());
  let user_agent = profile.webview_user_agent();

  assert_eq!(
    user_agent,
    WebviewUserAgent::Projected {
      prefix: profile.user_agent().to_string(),
      default_user_agent: DEFAULT_LINUX_WEBVIEW_USER_AGENT.to_string()
    }
  );
}

#[test]
fn projected_macos_webview_user_agent_uses_macos_default() {
  if ClientOs::Mac == ClientOs::Mac {
    return;
  }

  let client_os = ClientOs::Mac;
  let profile = OsProfileBuilder::new(client_os).build(test_snapshot());
  let user_agent = profile.webview_user_agent();

  assert_eq!(
    user_agent,
    WebviewUserAgent::Projected {
      prefix: profile.user_agent().to_string(),
      default_user_agent: DEFAULT_MACOS_WEBVIEW_USER_AGENT.to_string()
    }
  );
}

#[test]
fn projected_windows_webview_user_agent_uses_windows_default() {
  if ClientOs::Mac == ClientOs::Windows {
    return;
  }

  let client_os = ClientOs::Windows;
  let profile = OsProfileBuilder::new(client_os).build(test_snapshot());
  let user_agent = profile.webview_user_agent();

  assert_eq!(
    user_agent,
    WebviewUserAgent::Projected {
      prefix: profile.user_agent().to_string(),
      default_user_agent: DEFAULT_WINDOWS_WEBVIEW_USER_AGENT.to_string()
    }
  );
}

#[test]
fn os_version_strings_match_profiles() {
  assert!(defaults::os_version(ClientOs::Linux).starts_with("Linux "));
  assert!(defaults::os_version(ClientOs::Mac).starts_with("Apple Mac OS X "));
  assert!(defaults::os_version(ClientOs::Windows).starts_with("Microsoft "));
  assert_eq!(defaults::software_version(ClientOs::Linux), "");
  assert!(!defaults::software_version(ClientOs::Mac).is_empty());
  assert!(!defaults::software_version(ClientOs::Windows).is_empty());
}

#[test]
fn builder_fills_defaults_when_nothing_specified() {
  let profile = OsProfileBuilder::new(ClientOs::Linux).build(test_snapshot());

  assert_eq!(profile.client_os(), ClientOs::Linux);
  assert!(!profile.computer().is_empty());
  assert!(profile.os_version().starts_with("Linux "));
  assert_eq!(profile.client_version(), GP_CLIENT_VERSION_LINUX);
  assert!(!profile.host_id().is_empty());
}

#[test]
fn builder_uses_explicit_values_when_provided() {
  let identity = HostIdentity::from_parts(
    "identity-computer".to_string(),
    "test-host-id".to_string(),
    "test-serial".to_string(),
    "aa:bb:cc:dd:ee:ff".to_string(),
  );
  let profile = OsProfileBuilder::new(ClientOs::Mac)
    .computer_name_override("MyMac")
    .client_version("6.2.0-100")
    .host_identity(identity.clone())
    .build(test_snapshot());

  assert_eq!(profile.client_os(), ClientOs::Mac);
  assert_eq!(profile.computer(), "MyMac");
  assert!(profile.os_version().starts_with("Apple Mac OS X "));
  assert_eq!(profile.client_version(), "6.2.0-100");
  assert_eq!(profile.host_identity(), &identity);
  assert!(!profile.host_id().is_empty());
  assert!(!profile.serialno().is_empty());
  assert!(!profile.mac_addr().is_empty());
}

#[test]
fn builder_ignores_empty_strings() {
  let profile = OsProfileBuilder::new(ClientOs::Windows)
    .computer_name_override("")
    .client_version("")
    .build(test_snapshot());

  // Should fall back to defaults, not use empty strings
  assert!(!profile.computer().is_empty());
  assert!(profile.os_version().starts_with("Microsoft "));
  assert_eq!(profile.client_version(), GP_CLIENT_VERSION_WINDOWS);
}

#[test]
fn identity_accessors_delegate_to_host_identity() {
  let identity = HostIdentity::from_parts(
    "test-computer".to_string(),
    "host-123".to_string(),
    "serial-456".to_string(),
    "11:22:33:44:55:66".to_string(),
  );
  let profile = OsProfileBuilder::new(ClientOs::Mac)
    .host_identity(identity)
    .build(test_snapshot());

  assert!(!profile.host_id().is_empty());
  assert_eq!(profile.serialno(), "serial-456");
  assert!(!profile.mac_addr().is_empty());
}

#[test]
fn local_hostname_override_changes_only_projected_computer() {
  let identity = HostIdentity::from_parts(
    "real-computer".to_string(),
    "real-host-id".to_string(),
    "real-serial".to_string(),
    "11:22:33:44:55:66".to_string(),
  );

  let profile = OsProfileBuilder::new(ClientOs::Mac)
    .host_identity(identity)
    .computer_name_override("profile-computer")
    .build(test_snapshot());

  assert_eq!(profile.computer(), "profile-computer");
  assert_eq!(profile.host_identity().computer(), "real-computer");
}

#[test]
fn simulated_profile_identity_is_derived_from_real_host_id() {
  let client_os = non_native_client_os();
  let identity = HostIdentity::from_parts(
    "real-computer".to_string(),
    "stable-real-host-id".to_string(),
    "real-serial".to_string(),
    "11:22:33:44:55:66".to_string(),
  );

  let first = OsProfileBuilder::new(client_os)
    .host_identity(identity.clone())
    .build(test_snapshot());
  let second = OsProfileBuilder::new(client_os)
    .host_identity(identity)
    .build(test_snapshot());

  assert_eq!(first.host_identity().host_id(), "stable-real-host-id");
  assert_ne!(first.host_id(), "stable-real-host-id");
  assert_eq!(first.host_id(), second.host_id());
  assert_eq!(first.serialno(), second.serialno());
  assert_eq!(first.mac_addr(), second.mac_addr());
}

#[test]
fn simulated_linux_serial_uses_vmware_style_uuid_bytes() {
  if ClientOs::Mac == ClientOs::Linux {
    return;
  }

  let identity = HostIdentity::from_parts(
    "real-computer".to_string(),
    "stable-real-host-id".to_string(),
    "real-serial".to_string(),
    "11:22:33:44:55:66".to_string(),
  );

  let profile = OsProfileBuilder::new(ClientOs::Linux)
    .host_identity(identity)
    .build(test_snapshot());

  assert!(profile.serialno().starts_with("VMware-"));
  assert_eq!(
    profile.serialno().len(),
    "VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef".len()
  );
}

#[test]
fn profile_os_versions_are_not_host_identity_fields() {
  let identity = HostIdentity::from_parts(
    "real-computer".to_string(),
    "stable-real-host-id".to_string(),
    "real-serial".to_string(),
    "11:22:33:44:55:66".to_string(),
  );
  let profile = OsProfileBuilder::new(ClientOs::Mac)
    .host_identity(identity)
    .build(test_snapshot());
  let value = serde_json::to_value(profile.host_identity()).unwrap();

  assert!(value.get("osVersion").is_none());
  assert!(value.get("softwareVersion").is_none());
  assert!(profile.os_version().starts_with("Apple Mac OS X "));
  assert!(!profile.software_version().is_empty());
}

fn non_native_client_os() -> ClientOs {
  match ClientOs::Mac {
    ClientOs::Linux => ClientOs::Mac,
    ClientOs::Mac | ClientOs::Windows | ClientOs::Ios => ClientOs::Linux,
  }
}

// ─── Property-Based Tests ─────────────────────────────────────────────────

mod prop_tests {
  use super::*;
  use proptest::prelude::*;

  // **Validates: Requirements 1.1, 1.2, 1.3, 1.4, 2.3, 12.1, 12.2**
  //
  // Property 1: Derived user agent format
  // For any valid OsProfile built without a user agent override (with arbitrary
  // non-empty client_version), user_agent() returns a non-empty string matching
  // the format "PAN GlobalProtect/{client_version} ({profile.os_version})".
  proptest! {
    #[test]
    fn derived_user_agent_format(
      cv in "\\S{1,50}",
    ) {
      let profile = OsProfileBuilder::new(ClientOs::Linux)
        .client_version(cv.clone())
        .build(test_snapshot());

      let expected = format!("PAN GlobalProtect/{} ({})", cv, profile.os_version());
      prop_assert_eq!(profile.user_agent(), expected.as_str());
      prop_assert!(!profile.user_agent().is_empty());
    }
  }

  // **Validates: Requirements 2.2, 12.3**
  //
  // Property 2: Override returns verbatim
  // For any non-whitespace-only string provided to OsProfileBuilder::user_agent(),
  // the resulting OsProfile::user_agent() returns the exact input.
  proptest! {
    #[test]
    fn override_returns_verbatim(ua in "[^\\s][\\s\\S]{0,49}") {
      let profile = OsProfileBuilder::new(ClientOs::Linux)
        .user_agent(ua.clone())
        .build(test_snapshot());

      prop_assert_eq!(profile.user_agent(), ua.as_str());
    }
  }

  // **Validates: Requirements 2.4**
  //
  // Property 3: Whitespace override falls back to derived
  // For any string composed entirely of whitespace characters provided to
  // OsProfileBuilder::user_agent(), the resulting OsProfile::user_agent()
  // returns the derived format (not the whitespace string).
  proptest! {
    #[test]
    fn whitespace_override_falls_back_to_derived(ws in "[ \\t\\n]{1,20}") {
      let profile = OsProfileBuilder::new(ClientOs::Linux)
        .client_version("6.0.0")
        .user_agent(ws.clone())
        .build(test_snapshot());

      // The result must be the derived format, not the whitespace string
      let expected = format!("PAN GlobalProtect/6.0.0 ({})", profile.os_version());
      prop_assert_eq!(profile.user_agent(), expected.as_str());
      prop_assert_ne!(profile.user_agent(), ws.as_str());
    }
  }
}
