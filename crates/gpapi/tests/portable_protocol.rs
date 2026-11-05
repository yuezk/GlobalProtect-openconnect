use gpapi::{
  gp_params::GpParams,
  os_profile::{ClientOs, HostIdentity, OsProfile},
};

#[test]
fn mobile_profile_preserves_supplied_identity_without_hardware_claims() {
  let profile = OsProfile::builder(ClientOs::Ios).build(gpapi::os_profile::HostSnapshot {
    runtime_os: ClientOs::Ios,
    identity: HostIdentity::application("Phone".into(), "installation-id".into()),
    os_version: "Apple iOS 27.0.1".into(),
    software_version: String::new(),
  });
  assert_eq!(profile.client_os().as_str(), "iOS");
  assert_eq!(profile.client_os().to_openconnect_os(), "apple-ios");
  assert_eq!(profile.host_id(), "installation-id");
  assert_eq!(profile.computer(), "Phone");
  assert_eq!(profile.os_version(), "Apple iOS 27.0.1");
  assert!(profile.serialno().is_empty());
  assert!(profile.mac_addr().is_empty());
  assert!(!GpParams::builder(profile).build().ignore_tls_errors());
}

#[test]
fn reported_profile_does_not_change_runtime_platform() {
  let profile = OsProfile::builder(ClientOs::Mac).build(gpapi::os_profile::HostSnapshot {
    runtime_os: ClientOs::Ios,
    identity: HostIdentity::application("Phone".into(), "installation-id".into()),
    os_version: "Apple iOS 27.0.1".into(),
    software_version: String::new(),
  });
  assert!(!profile.is_native());
  assert!(profile.os_version().starts_with("Apple Mac OS X "));
  assert_eq!(profile.host_identity().host_id(), "installation-id");
}
