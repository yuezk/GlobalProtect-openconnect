use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{
  os_profile::{ClientOs, HostIdentity, OsProfile},
  utils::request::create_identity,
};

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type, Default)]
#[serde(rename_all = "lowercase")]
pub enum CscMode {
  #[default]
  Auto,
  Yes,
  No,
}

impl CscMode {
  pub fn effective(self, client_os: ClientOs) -> bool {
    match self {
      CscMode::Auto => client_os.default_csc_support(),
      CscMode::Yes => true,
      CscMode::No => false,
    }
  }

  pub fn as_str(self) -> &'static str {
    match self {
      CscMode::Auto => "auto",
      CscMode::Yes => "yes",
      CscMode::No => "no",
    }
  }
}

#[derive(Debug, Clone)]
pub struct GpParams {
  os_profile: OsProfile,
  // Transport
  ignore_tls_errors: bool,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
  // Feature
  csc_mode: CscMode,
  // Per-request state
  is_gateway: bool,
  input_str: Option<String>,
  otp: Option<String>,
}

impl GpParams {
  pub fn builder(os_profile: OsProfile) -> GpParamsBuilder {
    GpParamsBuilder::new(os_profile)
  }

  pub(crate) fn is_gateway(&self) -> bool {
    self.is_gateway
  }

  pub fn set_is_gateway(&mut self, is_gateway: bool) {
    self.is_gateway = is_gateway;
  }

  pub(crate) fn user_agent(&self) -> &str {
    self.os_profile.user_agent()
  }

  pub fn ignore_tls_errors(&self) -> bool {
    self.ignore_tls_errors
  }

  // ─── OsProfile accessor ─────────────────────────────────────────────────

  pub fn os_profile(&self) -> &OsProfile {
    &self.os_profile
  }

  // ─── Delegated identity accessors ────────────────────────────────────────

  pub fn client_os(&self) -> &str {
    self.os_profile.client_os().as_str()
  }

  pub(crate) fn computer(&self) -> &str {
    self.os_profile.computer()
  }

  pub fn os_version(&self) -> Option<&str> {
    Some(self.os_profile.os_version())
  }

  pub fn client_version(&self) -> Option<&str> {
    Some(self.os_profile.client_version())
  }

  pub(crate) fn host_id(&self) -> &str {
    self.os_profile.host_id()
  }

  pub(crate) fn serialno(&self) -> &str {
    self.os_profile.serialno()
  }

  pub fn host_identity(&self) -> &HostIdentity {
    self.os_profile.host_identity()
  }

  // ─── Feature accessors ──────────────────────────────────────────────────

  pub(crate) fn effective_csc_support(&self) -> bool {
    self.csc_mode.effective(self.os_profile.client_os())
  }

  pub fn csc_mode(&self) -> CscMode {
    self.csc_mode
  }

  // ─── Per-request state ──────────────────────────────────────────────────

  pub(crate) fn input_str(&self) -> Option<&str> {
    self.input_str.as_deref()
  }

  pub fn set_input_str(&mut self, input_str: &str) {
    self.input_str = Some(input_str.to_string());
  }

  pub fn set_otp(&mut self, otp: &str) {
    self.otp = Some(otp.to_string());
  }

  pub(crate) fn otp(&self) -> Option<&str> {
    self.otp.as_deref()
  }
}

pub struct GpParamsBuilder {
  is_gateway: bool,
  os_profile: OsProfile,
  csc_mode: CscMode,
  ignore_tls_errors: bool,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
}

impl GpParamsBuilder {
  pub fn new(os_profile: OsProfile) -> Self {
    Self {
      is_gateway: false,
      os_profile,
      csc_mode: Default::default(),
      ignore_tls_errors: false,
      certificate: Default::default(),
      sslkey: Default::default(),
      key_password: Default::default(),
    }
  }

  pub fn is_gateway(&mut self, is_gateway: bool) -> &mut Self {
    self.is_gateway = is_gateway;
    self
  }

  pub fn csc_mode(&mut self, csc_mode: CscMode) -> &mut Self {
    self.csc_mode = csc_mode;
    self
  }

  pub fn ignore_tls_errors(&mut self, ignore_tls_errors: bool) -> &mut Self {
    self.ignore_tls_errors = ignore_tls_errors;
    self
  }

  pub fn certificate<T: Into<Option<String>>>(&mut self, certificate: T) -> &mut Self {
    self.certificate = certificate.into();
    self
  }

  pub fn sslkey<T: Into<Option<String>>>(&mut self, sslkey: T) -> &mut Self {
    self.sslkey = sslkey.into();
    self
  }

  pub fn key_password<T: Into<Option<String>>>(&mut self, password: T) -> &mut Self {
    self.key_password = password.into();
    self
  }

  pub fn build(&self) -> GpParams {
    GpParams {
      os_profile: self.os_profile.clone(),
      ignore_tls_errors: self.ignore_tls_errors,
      certificate: self.certificate.clone(),
      sslkey: self.sslkey.clone(),
      key_password: self.key_password.clone(),
      csc_mode: self.csc_mode,
      is_gateway: self.is_gateway,
      input_str: Default::default(),
      otp: Default::default(),
    }
  }
}

impl TryFrom<&GpParams> for Client {
  type Error = anyhow::Error;

  fn try_from(value: &GpParams) -> Result<Self, Self::Error> {
    let mut builder = Client::builder()
      .danger_accept_invalid_certs(value.ignore_tls_errors)
      .user_agent(value.user_agent());

    if let Some(cert) = value.certificate.as_deref() {
      info!("Using client certificate authentication...");
      let identity = create_identity(cert, value.sslkey.as_deref(), value.key_password.as_deref())?;
      builder = builder.identity(identity);
    }

    let client = builder.build()?;
    Ok(client)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::os_profile::runtime_client_os;

  fn profile(client_os: ClientOs) -> OsProfile {
    OsProfile::builder(client_os).build()
  }

  #[test]
  fn default_client_versions_match_observed_official_clients() {
    assert_eq!(ClientOs::Linux.default_client_version(), "6.3.3-619");
    assert_eq!(ClientOs::Windows.default_client_version(), "6.3.3-650");
    assert_eq!(ClientOs::Mac.default_client_version(), "6.3.3-915");
  }

  #[test]
  fn builder_uses_os_profile_client_version_when_unspecified() {
    let params = GpParams::builder(profile(ClientOs::Mac)).build();

    assert_eq!(params.client_version(), Some("6.3.3-915"));
  }

  #[test]
  fn builder_keeps_explicit_client_version() {
    let profile = OsProfile::builder(ClientOs::Mac)
      .client_version("6.2.9-1".to_string())
      .build();
    let params = GpParams::builder(profile).build();

    assert_eq!(params.client_version(), Some("6.2.9-1"));
  }

  #[test]
  fn builder_uses_os_profile_version_when_unspecified() {
    let params = GpParams::builder(profile(ClientOs::Mac)).build();
    let default_os_version = ClientOs::Mac.default_os_version();

    assert_eq!(params.os_version(), Some(default_os_version.as_str()));
  }

  #[test]
  fn os_profile_accessor_returns_profile() {
    let params = GpParams::builder(profile(ClientOs::Linux)).build();
    let profile = params.os_profile();

    assert_eq!(profile.client_os(), ClientOs::Linux);
  }

  #[test]
  fn delegates_identity_to_os_profile() {
    let identity = HostIdentity::new(
      "test-computer".to_string(),
      "test-host-id".to_string(),
      "test-serial".to_string(),
      "aa:bb:cc:dd:ee:ff".to_string(),
    );
    let params = GpParams::builder(OsProfile::builder(ClientOs::Linux).host_identity(identity).build()).build();

    assert!(!params.host_id().is_empty());
    assert_eq!(params.host_identity().host_id(), "test-host-id");
    assert_eq!(params.host_identity().serialno(), "test-serial");
    assert_eq!(params.host_identity().mac_addr(), "aa:bb:cc:dd:ee:ff");
  }

  #[test]
  fn builder_accepts_pre_built_os_profile() {
    let profile = OsProfile::builder(ClientOs::Mac)
      .computer_name_override("MyMac")
      .build();
    let expected_os_version = profile.os_version().to_string();

    let params = GpParams::builder(profile).build();

    assert_eq!(params.computer(), "MyMac");
    assert_eq!(params.os_version(), Some(expected_os_version.as_str()));
    assert_eq!(params.client_os(), "Mac");
  }

  // ─── Delegation tests (Requirement 4.1) ──────────────────────────────────

  #[test]
  fn delegates_computer_to_os_profile() {
    let profile = OsProfile::builder(ClientOs::Mac)
      .computer_name_override("MyComputer")
      .build();
    let params = GpParams::builder(profile).build();

    assert_eq!(params.computer(), "MyComputer");
    assert_eq!(params.computer(), params.os_profile().computer());
  }

  #[test]
  fn delegates_client_os_string_to_os_profile() {
    let params = GpParams::builder(profile(ClientOs::Windows)).build();

    assert_eq!(params.client_os(), "Windows");
    assert_eq!(params.client_os(), params.os_profile().client_os().as_str());
  }

  #[test]
  fn delegates_host_identity_to_os_profile() {
    let identity = HostIdentity::new(
      "delegated-computer".to_string(),
      "delegated-host".to_string(),
      "delegated-serial".to_string(),
      "01:02:03:04:05:06".to_string(),
    );
    let params = GpParams::builder(
      OsProfile::builder(ClientOs::Linux)
        .host_identity(identity.clone())
        .build(),
    )
    .build();

    // The accessor returns the same HostIdentity stored on the inner OsProfile
    assert_eq!(params.host_identity(), params.os_profile().host_identity());
    assert_eq!(params.host_identity().host_id(), "delegated-host");
    assert_eq!(params.host_identity().serialno(), "delegated-serial");
    assert_eq!(params.host_identity().mac_addr(), "01:02:03:04:05:06");
  }

  // ─── Builder constructs OsProfile from individual fields (Req 4.1) ───────

  #[test]
  fn builder_uses_provided_os_profile() {
    let identity = HostIdentity::new(
      "identity-host".to_string(),
      "host-7".to_string(),
      "serial-7".to_string(),
      "aa:bb:cc:11:22:33".to_string(),
    );
    let profile = OsProfile::builder(ClientOs::Linux)
      .computer_name_override("linux-host")
      .client_version("6.0.0-1".to_string())
      .host_identity(identity)
      .build();
    let params = GpParams::builder(profile).build();

    let profile = params.os_profile();
    assert_eq!(profile.client_os(), ClientOs::Linux);
    assert_eq!(profile.computer(), "linux-host");
    assert_eq!(profile.os_version(), ClientOs::Linux.default_os_version());
    assert_eq!(profile.client_version(), "6.0.0-1");
    assert_eq!(profile.host_identity().host_id(), "host-7");
    assert_eq!(profile.host_identity().serialno(), "serial-7");
    assert_eq!(profile.host_identity().mac_addr(), "aa:bb:cc:11:22:33");
    assert!(!profile.host_id().is_empty());
    assert!(!profile.serialno().is_empty());
    assert!(!profile.mac_addr().is_empty());
  }

  // ─── TryFrom<&GpParams> for Client (Requirement 4.7) ─────────────────────

  #[test]
  fn try_from_gp_params_constructs_reqwest_client() {
    let params = GpParams::builder(profile(runtime_client_os())).build();
    let client = Client::try_from(&params);

    assert!(client.is_ok(), "Client construction should succeed with default params");
  }

  #[test]
  fn try_from_gp_params_with_ignore_tls_errors() {
    let params = GpParams::builder(profile(runtime_client_os()))
      .ignore_tls_errors(true)
      .build();
    let client = Client::try_from(&params);

    assert!(
      client.is_ok(),
      "Client should construct successfully with ignore_tls_errors enabled"
    );
  }

  #[test]
  fn try_from_gp_params_with_custom_user_agent() {
    let params = GpParams::builder(
      OsProfile::builder(runtime_client_os())
        .user_agent("custom-agent/1.0")
        .build(),
    )
    .build();
    let client = Client::try_from(&params);

    assert!(client.is_ok(), "Client should construct with a custom user agent");
  }

  #[test]
  fn try_from_gp_params_with_invalid_certificate_returns_error() {
    // When a certificate path is provided but the file does not exist or is malformed,
    // Client construction should fail rather than silently succeed.
    let params = GpParams::builder(profile(runtime_client_os()))
      .certificate("/nonexistent/path/to/cert.pem".to_string())
      .build();
    let client = Client::try_from(&params);

    assert!(
      client.is_err(),
      "Client construction should fail when certificate path is invalid"
    );
  }

  // ─── User Agent Delegation Tests (Requirements 3.2, 3.3, 3.4, 3.5, 3.6) ──

  #[test]
  fn user_agent_delegates_to_os_profile_for_default_build() {
    let params = GpParams::builder(profile(runtime_client_os())).build();

    assert_eq!(
      params.user_agent(),
      params.os_profile().user_agent(),
      "GpParams::user_agent() should delegate to os_profile().user_agent()"
    );
  }

  #[test]
  fn builder_uses_profile_user_agent() {
    let params = GpParams::builder(OsProfile::builder(runtime_client_os()).user_agent("custom").build()).build();

    assert_eq!(params.user_agent(), "custom");
  }

  #[test]
  fn builder_os_profile_without_user_agent_delegates_to_profile() {
    let profile = OsProfile::builder(ClientOs::Mac).client_version("6.1.0").build();
    let expected_ua = profile.user_agent().to_string();

    let params = GpParams::builder(profile).build();

    assert_eq!(params.user_agent(), expected_ua);
  }

  #[test]
  fn profile_user_agent_override_wins_before_gp_params_construction() {
    let profile = OsProfile::builder(ClientOs::Mac)
      .client_version("6.1.0")
      .user_agent("override")
      .build();
    let params = GpParams::builder(profile).build();

    assert_eq!(params.user_agent(), "override");
  }

  #[test]
  fn try_from_gp_params_succeeds_with_delegated_user_agent() {
    let params = GpParams::builder(
      OsProfile::builder(runtime_client_os())
        .user_agent("delegated-agent/2.0")
        .build(),
    )
    .build();
    let client = Client::try_from(&params);

    assert!(
      client.is_ok(),
      "TryFrom<&GpParams> for Client should succeed with delegated user_agent"
    );
  }

  // ─── Property-Based Tests ─────────────────────────────────────────────────

  mod prop_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_client_os() -> impl Strategy<Value = ClientOs> {
      prop_oneof![Just(ClientOs::Linux), Just(ClientOs::Windows), Just(ClientOs::Mac),]
    }

    // **Validates: Requirements 3.2, 3.4**
    //
    // Property 4: GpParams delegates user_agent to OsProfile
    // For any GpParams instance (whether built from individual fields or from a
    // pre-built OsProfile, with or without a user_agent override),
    // gp_params.user_agent() == gp_params.os_profile().user_agent().
    proptest! {
      #![proptest_config(ProptestConfig::with_cases(100))]
      #[test]
      fn gp_params_delegates_user_agent_to_os_profile(
        client_os in arb_client_os(),
        user_agent_override in proptest::option::of("[a-zA-Z0-9/_.-]{1,30}"),
      ) {
        let identity = HostIdentity::new(
          "prop-computer".to_string(),
          "prop-host".to_string(),
          "prop-serial".to_string(),
          "aa:bb:cc:dd:ee:ff".to_string(),
        );

        let mut profile_builder = OsProfile::builder(client_os)
          .client_version("6.1.0-1")
          .computer_name_override("test-host")
          .host_identity(identity);
        if let Some(ref ua) = user_agent_override {
          profile_builder = profile_builder.user_agent(ua);
        }
        let params = GpParams::builder(profile_builder.build()).build();

        prop_assert_eq!(
          params.user_agent(),
          params.os_profile().user_agent(),
          "GpParams::user_agent() must always equal GpParams::os_profile().user_agent()"
        );
      }
    }

    // **Validates: Requirements 3.3**
    //
    // Property 5: GpParams uses the user agent from OsProfile.
    // For any non-empty string set via OsProfileBuilder::user_agent(), the resulting
    // GpParams::user_agent() returns that override value.
    proptest! {
      #[test]
      fn builder_uses_profile_user_agent(ua in "[^\\s]{1,50}") {
        let profile = OsProfile::builder(runtime_client_os()).user_agent(&ua).build();
        let params = GpParams::builder(profile).build();

        prop_assert_eq!(params.user_agent(), ua.as_str());
      }
    }
  }
}
