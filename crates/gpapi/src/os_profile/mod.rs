pub mod host_identity;
mod serial_number;

pub use host_identity::HostIdentity;

use common::constants::{GP_CLIENT_VERSION_LINUX, GP_CLIENT_VERSION_MACOS, GP_CLIENT_VERSION_WINDOWS, GP_USER_AGENT};
use serde::{Deserialize, Serialize};
use specta::Type;

const DEFAULT_MACOS_VERSION: &str = "13.4.0";
const DEFAULT_LINUX_DISTRO: &str = "Ubuntu 24.04.3 LTS";
const DEFAULT_WINDOWS_DISTRO: &str = "Windows 11 Pro";
const DEFAULT_WINDOWS_VERSION: &str = "10.0.22631.0";
const DEFAULT_LINUX_WEBVIEW_USER_AGENT: &str =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/602.1 (KHTML, like Gecko) PanGPUI Version/10.0 Safari/602.1";
const DEFAULT_MACOS_WEBVIEW_USER_AGENT: &str =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
const DEFAULT_WINDOWS_WEBVIEW_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36 Edg/144.0.0.0";

// ─── ClientOs ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
pub enum ClientOs {
  Linux,
  Windows,
  Mac,
}

impl From<&str> for ClientOs {
  fn from(os: &str) -> Self {
    match os {
      "Linux" => ClientOs::Linux,
      "Windows" => ClientOs::Windows,
      "Mac" => ClientOs::Mac,
      _ => ClientOs::Linux,
    }
  }
}

impl ClientOs {
  pub fn as_str(&self) -> &'static str {
    match self {
      ClientOs::Linux => "Linux",
      ClientOs::Windows => "Windows",
      ClientOs::Mac => "Mac",
    }
  }

  pub fn to_openconnect_os(&self) -> &str {
    match self {
      ClientOs::Linux => "linux",
      ClientOs::Windows => "win",
      ClientOs::Mac => "mac-intel",
    }
  }

  pub fn default_client_version(&self) -> &'static str {
    match self {
      ClientOs::Linux => GP_CLIENT_VERSION_LINUX,
      ClientOs::Windows => GP_CLIENT_VERSION_WINDOWS,
      ClientOs::Mac => GP_CLIENT_VERSION_MACOS,
    }
  }

  pub fn default_os_version(&self) -> String {
    default_os_version(self)
  }

  pub(crate) fn default_csc_support(&self) -> bool {
    match self {
      ClientOs::Linux => false,
      ClientOs::Mac | ClientOs::Windows => true,
    }
  }
}

// ─── PreloginParamLocation ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreloginParamLocation {
  Body,
  Query,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreloginBrowserMode {
  Embedded,
  External,
}

// ─── WebviewUserAgent ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebviewUserAgent {
  Native {
    prefix: String,
    transform: WebviewUserAgentTransform,
  },
  Projected {
    prefix: String,
    default_user_agent: String,
  },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebviewUserAgentTransform {
  None,
  LinuxPanGpuiVersion,
}

// ─── Target behavior ────────────────────────────────────────────────────────

mod target {
  use super::*;

  pub(super) fn os_version(client_os: ClientOs) -> String {
    match client_os {
      ClientOs::Linux => linux_os_string(),
      ClientOs::Windows => windows_os_string(),
      ClientOs::Mac => macos_os_string(),
    }
  }

  pub(super) fn software_version(client_os: ClientOs) -> String {
    match client_os {
      ClientOs::Linux => String::new(),
      ClientOs::Windows => windows_version(),
      ClientOs::Mac => macos_version(),
    }
  }

  pub(super) fn prelogin_param_location(client_os: ClientOs) -> PreloginParamLocation {
    match client_os {
      ClientOs::Linux | ClientOs::Mac => PreloginParamLocation::Body,
      ClientOs::Windows => PreloginParamLocation::Query,
    }
  }

  pub(super) fn saml_password(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => "SAMLPASS",
      ClientOs::Mac | ClientOs::Windows => "",
    }
  }

  pub(super) fn external_browser_os_value(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => "4",
      ClientOs::Mac => "3",
      ClientOs::Windows => "2",
    }
  }

  pub(super) fn supports_macos_plist_csc(client_os: ClientOs) -> bool {
    matches!(client_os, ClientOs::Mac)
  }

  pub(super) fn supports_linux_process_csc(client_os: ClientOs) -> bool {
    matches!(client_os, ClientOs::Linux)
  }

  pub(super) fn supports_windows_registry_csc(client_os: ClientOs) -> bool {
    matches!(client_os, ClientOs::Windows)
  }

  pub(super) fn kerberos_support_in_query(client_os: ClientOs) -> bool {
    match client_os {
      ClientOs::Linux => false,
      ClientOs::Mac | ClientOs::Windows => true,
    }
  }

  pub(super) fn os_vendor(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => "Linux",
      ClientOs::Mac => "Apple",
      ClientOs::Windows => "Microsoft",
    }
  }

  pub(super) fn webview_user_agent_default(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => DEFAULT_LINUX_WEBVIEW_USER_AGENT,
      ClientOs::Mac => DEFAULT_MACOS_WEBVIEW_USER_AGENT,
      ClientOs::Windows => DEFAULT_WINDOWS_WEBVIEW_USER_AGENT,
    }
  }

  pub(super) fn webview_user_agent_transform(client_os: ClientOs) -> WebviewUserAgentTransform {
    match client_os {
      ClientOs::Linux => WebviewUserAgentTransform::LinuxPanGpuiVersion,
      ClientOs::Mac | ClientOs::Windows => WebviewUserAgentTransform::None,
    }
  }
}

// ─── ProfileHostIdentity ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProfileHostIdentity {
  computer: String,
  host_id: String,
  serial_number: String,
  mac_address: String,
}

impl ProfileHostIdentity {
  fn for_client_os(client_os: ClientOs, host_identity: &HostIdentity, computer_override: Option<String>) -> Self {
    target_identity::project(client_os, host_identity, computer_override)
  }

  fn computer(&self) -> &str {
    &self.computer
  }

  fn host_id(&self) -> &str {
    &self.host_id
  }

  fn serialno(&self) -> &str {
    &self.serial_number
  }

  fn mac_addr(&self) -> &str {
    &self.mac_address
  }
}

// ─── OsProfile ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OsProfile {
  client_os: ClientOs,
  os_version: String,
  software_version: String,
  os_vendor: &'static str,
  client_version: String,
  host_identity: HostIdentity,
  profile_identity: ProfileHostIdentity,
  user_agent: String,
}

impl OsProfile {
  pub fn builder(client_os: ClientOs) -> OsProfileBuilder {
    OsProfileBuilder::new(client_os)
  }

  // ─── Identity accessors ──────────────────────────────────────────────────

  pub fn client_os(&self) -> ClientOs {
    self.client_os
  }

  pub fn computer(&self) -> &str {
    self.profile_identity.computer()
  }

  pub fn os_version(&self) -> &str {
    &self.os_version
  }

  pub fn client_version(&self) -> &str {
    &self.client_version
  }

  pub fn host_identity(&self) -> &HostIdentity {
    &self.host_identity
  }

  pub fn user_agent(&self) -> &str {
    &self.user_agent
  }

  pub fn webview_user_agent(&self) -> WebviewUserAgent {
    if self.is_native() {
      WebviewUserAgent::Native {
        prefix: self.user_agent.clone(),
        transform: target::webview_user_agent_transform(self.client_os),
      }
    } else {
      WebviewUserAgent::Projected {
        prefix: self.user_agent.clone(),
        default_user_agent: target::webview_user_agent_default(self.client_os).to_string(),
      }
    }
  }

  pub fn host_id(&self) -> &str {
    self.profile_identity.host_id()
  }

  pub fn serialno(&self) -> &str {
    self.profile_identity.serialno()
  }

  pub fn mac_addr(&self) -> &str {
    self.profile_identity.mac_addr()
  }

  /// Returns true when the target client OS matches the runtime OS.
  ///
  /// Used to decide whether to consult real OS-level state (e.g. the actual
  /// network interface) versus simulated values for emulation.
  pub fn is_native(&self) -> bool {
    runtime_client_os() == self.client_os
  }

  /// Default software version string for the target OS, matching the
  /// official client values.
  pub fn software_version(&self) -> &str {
    &self.software_version
  }

  // ─── Behavioral methods ──────────────────────────────────────────────────

  pub(crate) fn prelogin_param_location(&self) -> PreloginParamLocation {
    target::prelogin_param_location(self.client_os)
  }

  pub(crate) fn portal_default_browser(&self, browser_mode: PreloginBrowserMode) -> &'static str {
    match browser_mode {
      PreloginBrowserMode::Embedded => "-10",
      PreloginBrowserMode::External => target::external_browser_os_value(self.client_os),
    }
  }

  pub(crate) fn gateway_default_browser(&self, browser_mode: PreloginBrowserMode) -> &'static str {
    match browser_mode {
      PreloginBrowserMode::Embedded => "0",
      PreloginBrowserMode::External => target::external_browser_os_value(self.client_os),
    }
  }

  pub(crate) fn saml_password(&self) -> &'static str {
    target::saml_password(self.client_os)
  }

  pub(crate) fn gateway_server_uses_resolved_ipv4(&self) -> bool {
    true
  }

  pub(crate) fn supports_macos_plist_csc(&self) -> bool {
    target::supports_macos_plist_csc(self.client_os)
  }

  pub(crate) fn supports_linux_process_csc(&self) -> bool {
    target::supports_linux_process_csc(self.client_os)
  }

  pub(crate) fn supports_windows_registry_csc(&self) -> bool {
    target::supports_windows_registry_csc(self.client_os)
  }

  pub(crate) fn kerberos_support_in_query(&self) -> bool {
    target::kerberos_support_in_query(self.client_os)
  }

  pub fn os_vendor(&self) -> &'static str {
    self.os_vendor
  }
}

// ─── OsProfileBuilder ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OsProfileBuilder {
  client_os: ClientOs,
  client_version: Option<String>,
  computer_name_override: Option<String>,
  host_id_override: Option<String>,
  host_identity: Option<HostIdentity>,
  user_agent: Option<String>,
}

impl OsProfileBuilder {
  pub fn new(client_os: ClientOs) -> Self {
    Self {
      client_os,
      client_version: None,
      computer_name_override: None,
      host_id_override: None,
      host_identity: None,
      user_agent: None,
    }
  }

  pub fn computer_name_override(mut self, name: impl Into<String>) -> Self {
    self.computer_name_override = Some(name.into());
    self
  }

  pub fn client_version(mut self, version: impl Into<String>) -> Self {
    self.client_version = Some(version.into());
    self
  }

  pub fn host_identity(mut self, identity: HostIdentity) -> Self {
    self.host_identity = Some(identity);
    self
  }

  pub fn host_id_override(mut self, host_id: impl Into<String>) -> Self {
    let host_id = host_id.into();
    self.host_id_override = if host_id.trim().is_empty() { None } else { Some(host_id) };
    self
  }

  pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
    let ua = ua.into();
    self.user_agent = if ua.trim().is_empty() { None } else { Some(ua) };
    self
  }

  pub fn build(self) -> OsProfile {
    let client_version = self
      .client_version
      .filter(|v| !v.trim().is_empty())
      .unwrap_or_else(|| self.client_os.default_client_version().to_string());

    let host_identity = match (self.host_identity, self.host_id_override) {
      (Some(identity), _) => identity,
      (None, host_id) => HostIdentity::collect_with_host_id(host_id.as_deref()),
    };
    let profile_identity =
      ProfileHostIdentity::for_client_os(self.client_os, &host_identity, self.computer_name_override);
    let os_version = target::os_version(self.client_os);
    let software_version = target::software_version(self.client_os);
    let os_vendor = target::os_vendor(self.client_os);

    let user_agent = self
      .user_agent
      .unwrap_or_else(|| format!("{}/{} ({})", GP_USER_AGENT, client_version, os_version));

    OsProfile {
      client_os: self.client_os,
      os_version,
      software_version,
      os_vendor,
      client_version,
      host_identity,
      profile_identity,
      user_agent,
    }
  }
}

// ─── Default helpers ─────────────────────────────────────────────────────────

pub fn default_computer_name(client_os: &ClientOs) -> String {
  ProfileHostIdentity::for_client_os(*client_os, &HostIdentity::collect(), None)
    .computer()
    .to_string()
}

pub fn default_os_version(client_os: &ClientOs) -> String {
  target::os_version(*client_os)
}

pub fn default_software_version(client_os: &ClientOs) -> String {
  target::software_version(*client_os)
}

pub fn runtime_client_os() -> ClientOs {
  HostIdentity::runtime_os()
}

fn linux_os_string() -> String {
  if runtime_client_os() == ClientOs::Linux {
    format!("Linux {}", runtime_distro_or(DEFAULT_LINUX_DISTRO))
  } else {
    format!("Linux {}", DEFAULT_LINUX_DISTRO)
  }
}

fn windows_version() -> String {
  if runtime_client_os() == ClientOs::Windows {
    match os_info::get().version() {
      os_info::Version::Unknown => DEFAULT_WINDOWS_VERSION.to_string(),
      v => format!("{}.0", v),
    }
  } else {
    DEFAULT_WINDOWS_VERSION.to_string()
  }
}

fn windows_os_string() -> String {
  if runtime_client_os() == ClientOs::Windows {
    let edition = os_info::get()
      .edition()
      .map(|e| e.to_string())
      .unwrap_or_else(|| runtime_distro_or(DEFAULT_WINDOWS_DISTRO));
    format!("Microsoft {} , 64-bit", edition)
  } else {
    format!("Microsoft {} , 64-bit", DEFAULT_WINDOWS_DISTRO)
  }
}

fn runtime_distro_or(fallback: &str) -> String {
  whoami::distro().unwrap_or_else(|_| fallback.to_string())
}

fn macos_version() -> String {
  if runtime_client_os() == ClientOs::Mac {
    match os_info::get().version() {
      os_info::Version::Unknown => DEFAULT_MACOS_VERSION.to_string(),
      v => v.to_string(),
    }
  } else {
    DEFAULT_MACOS_VERSION.to_string()
  }
}

fn macos_os_string() -> String {
  format!("Apple Mac OS X {}", macos_version())
}

fn derive_uuid_from_seed(seed: &str, parts: &[&str]) -> String {
  use uuid::Uuid;

  let namespace = Uuid::NAMESPACE_DNS;
  let name = format!("{}-{}", seed, parts.join("-"));

  Uuid::new_v5(&namespace, name.as_bytes()).hyphenated().to_string()
}

fn derived_profile_mac(seed: &str, client_os: ClientOs) -> String {
  let uuid = derive_uuid_from_seed(seed, &["os-profile", client_os.as_str(), "mac"]);
  let compact = uuid.replace('-', "");
  let bytes = compact
    .as_bytes()
    .chunks(2)
    .take(6)
    .map(|chunk| std::str::from_utf8(chunk).unwrap_or("00").to_lowercase())
    .collect::<Vec<_>>();

  match client_os {
    ClientOs::Linux | ClientOs::Windows => bytes.join("-"),
    ClientOs::Mac => bytes.join(":"),
  }
}

fn compact_uuid(seed: &str, client_os: ClientOs, suffix: &str) -> String {
  derive_uuid_from_seed(seed, &["os-profile", client_os.as_str(), suffix])
    .replace('-', "")
    .to_uppercase()
}

mod target_identity {
  use super::*;

  pub(super) fn project(
    client_os: ClientOs,
    host_identity: &HostIdentity,
    computer_override: Option<String>,
  ) -> ProfileHostIdentity {
    if client_os == runtime_client_os() {
      return native::project(client_os, host_identity, computer_override);
    }

    match client_os {
      ClientOs::Linux => linux::project(host_identity, computer_override),
      ClientOs::Windows => windows::project(host_identity, computer_override),
      ClientOs::Mac => macos::project(host_identity, computer_override),
    }
  }

  fn computer(host_identity: &HostIdentity, computer_override: Option<String>) -> String {
    computer_override
      .filter(|value| !value.trim().is_empty())
      .unwrap_or_else(|| host_identity.computer().to_string())
  }

  fn profile_mac_for(client_os: ClientOs, mac_address: &str) -> String {
    match client_os {
      ClientOs::Linux | ClientOs::Windows => mac_address.replace(':', "-"),
      ClientOs::Mac => mac_address.replace('-', ":"),
    }
  }

  mod native {
    use super::*;

    pub(super) fn project(
      client_os: ClientOs,
      host_identity: &HostIdentity,
      computer_override: Option<String>,
    ) -> ProfileHostIdentity {
      ProfileHostIdentity {
        computer: computer(host_identity, computer_override),
        host_id: match client_os {
          ClientOs::Mac => host_identity.host_id().replace('-', ":"),
          ClientOs::Linux | ClientOs::Windows => host_identity.host_id().to_string(),
        },
        serial_number: host_identity.serialno().to_string(),
        mac_address: profile_mac_for(client_os, host_identity.mac_addr()),
      }
    }
  }

  mod linux {
    use super::*;

    pub(super) fn project(host_identity: &HostIdentity, computer_override: Option<String>) -> ProfileHostIdentity {
      let seed = host_identity.host_id();
      let host_id = derive_uuid_from_seed(seed, &["os-profile", ClientOs::Linux.as_str(), "host-id"]);
      ProfileHostIdentity {
        computer: computer(host_identity, computer_override),
        serial_number: serial_number::vmware_from_uuid(&host_id).expect("projected Linux host ID should be a UUID"),
        host_id,
        mac_address: derived_profile_mac(seed, ClientOs::Linux),
      }
    }
  }

  mod windows {
    use super::*;

    pub(super) fn project(host_identity: &HostIdentity, computer_override: Option<String>) -> ProfileHostIdentity {
      let seed = host_identity.host_id();
      ProfileHostIdentity {
        computer: computer(host_identity, computer_override),
        host_id: derive_uuid_from_seed(seed, &["os-profile", ClientOs::Windows.as_str(), "host-id"]),
        serial_number: serial_number::vmware_from_compact_hex(&compact_uuid(seed, ClientOs::Windows, "serialno"))
          .expect("projected Windows serial seed should be compact hex"),
        mac_address: derived_profile_mac(seed, ClientOs::Windows),
      }
    }
  }

  mod macos {
    use super::*;

    pub(super) fn project(host_identity: &HostIdentity, computer_override: Option<String>) -> ProfileHostIdentity {
      let seed = host_identity.host_id();
      let mac_address = derived_profile_mac(seed, ClientOs::Mac);
      ProfileHostIdentity {
        computer: computer(host_identity, computer_override),
        host_id: mac_address.clone(),
        serial_number: compact_uuid(seed, ClientOs::Mac, "serialno").chars().take(10).collect(),
        mac_address,
      }
    }
  }

  #[cfg(test)]
  mod tests {
    use super::*;

    fn host_identity() -> HostIdentity {
      HostIdentity::new(
        "real-computer".to_string(),
        "real-host-id".to_string(),
        "real-serial".to_string(),
        "AA-BB-CC-DD-EE-FF".to_string(),
      )
    }

    #[test]
    fn native_linux_projection_uses_real_identity_with_dash_mac() {
      let profile = native::project(ClientOs::Linux, &host_identity(), None);

      assert_eq!(profile.computer(), "real-computer");
      assert_eq!(profile.host_id(), "real-host-id");
      assert_eq!(profile.serialno(), "real-serial");
      assert_eq!(profile.mac_addr(), "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn native_windows_projection_uses_real_identity_with_dash_mac() {
      let profile = native::project(ClientOs::Windows, &host_identity(), None);

      assert_eq!(profile.computer(), "real-computer");
      assert_eq!(profile.host_id(), "real-host-id");
      assert_eq!(profile.serialno(), "real-serial");
      assert_eq!(profile.mac_addr(), "aa-bb-cc-dd-ee-ff");
    }

    #[test]
    fn native_macos_projection_uses_real_identity_with_colon_mac() {
      let profile = native::project(ClientOs::Mac, &host_identity(), None);

      assert_eq!(profile.computer(), "real-computer");
      assert_eq!(profile.host_id(), "real:host:id");
      assert_eq!(profile.serialno(), "real-serial");
      assert_eq!(profile.mac_addr(), "aa:bb:cc:dd:ee:ff");
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn csc_support_defaults_match_official_profiles() {
    assert!(!ClientOs::Linux.default_csc_support());
    assert!(ClientOs::Mac.default_csc_support());
    assert!(ClientOs::Windows.default_csc_support());
  }

  #[test]
  fn prelogin_param_location_matches_official_profiles() {
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Linux).build().prelogin_param_location(),
      PreloginParamLocation::Body
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Mac).build().prelogin_param_location(),
      PreloginParamLocation::Body
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .prelogin_param_location(),
      PreloginParamLocation::Query
    );
  }

  #[test]
  fn embedded_portal_default_browser_is_minus_ten_for_all() {
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .portal_default_browser(PreloginBrowserMode::Embedded),
      "-10"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .portal_default_browser(PreloginBrowserMode::Embedded),
      "-10"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .portal_default_browser(PreloginBrowserMode::Embedded),
      "-10"
    );
  }

  #[test]
  fn embedded_gateway_default_browser_is_zero_for_all() {
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .gateway_default_browser(PreloginBrowserMode::Embedded),
      "0"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .gateway_default_browser(PreloginBrowserMode::Embedded),
      "0"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .gateway_default_browser(PreloginBrowserMode::Embedded),
      "0"
    );
  }

  #[test]
  fn external_default_browser_values_match_official_profiles() {
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .portal_default_browser(PreloginBrowserMode::External),
      "4"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .portal_default_browser(PreloginBrowserMode::External),
      "3"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .portal_default_browser(PreloginBrowserMode::External),
      "2"
    );

    assert_eq!(
      OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .gateway_default_browser(PreloginBrowserMode::External),
      "4"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .gateway_default_browser(PreloginBrowserMode::External),
      "3"
    );
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .gateway_default_browser(PreloginBrowserMode::External),
      "2"
    );
  }

  #[test]
  fn saml_password_matches_official_profiles() {
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Linux).build().saml_password(),
      "SAMLPASS"
    );
    assert_eq!(OsProfileBuilder::new(ClientOs::Mac).build().saml_password(), "");
    assert_eq!(OsProfileBuilder::new(ClientOs::Windows).build().saml_password(), "");
  }

  #[test]
  fn gateway_server_uses_resolved_ipv4_for_all() {
    assert!(
      OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .gateway_server_uses_resolved_ipv4()
    );
    assert!(
      OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .gateway_server_uses_resolved_ipv4()
    );
    assert!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .gateway_server_uses_resolved_ipv4()
    );
  }

  #[test]
  fn kerberos_support_in_query_matches_official_profiles() {
    assert!(
      !OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .kerberos_support_in_query()
    );
    assert!(OsProfileBuilder::new(ClientOs::Mac).build().kerberos_support_in_query());
    assert!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .kerberos_support_in_query()
    );
  }

  #[test]
  fn os_vendor_matches_official_profiles() {
    assert_eq!(OsProfileBuilder::new(ClientOs::Linux).build().os_vendor(), "Linux");
    assert_eq!(OsProfileBuilder::new(ClientOs::Mac).build().os_vendor(), "Apple");
    assert_eq!(
      OsProfileBuilder::new(ClientOs::Windows).build().os_vendor(),
      "Microsoft"
    );
  }

  #[test]
  fn supports_macos_plist_csc_only_for_mac() {
    assert!(
      !OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .supports_macos_plist_csc()
    );
    assert!(OsProfileBuilder::new(ClientOs::Mac).build().supports_macos_plist_csc());
    assert!(
      !OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .supports_macos_plist_csc()
    );
  }

  #[test]
  fn supports_linux_process_csc_only_for_linux() {
    assert!(
      OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .supports_linux_process_csc()
    );
    assert!(
      !OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .supports_linux_process_csc()
    );
    assert!(
      !OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .supports_linux_process_csc()
    );
  }

  #[test]
  fn supports_windows_registry_csc_only_for_windows() {
    assert!(
      !OsProfileBuilder::new(ClientOs::Linux)
        .build()
        .supports_windows_registry_csc()
    );
    assert!(
      !OsProfileBuilder::new(ClientOs::Mac)
        .build()
        .supports_windows_registry_csc()
    );
    assert!(
      OsProfileBuilder::new(ClientOs::Windows)
        .build()
        .supports_windows_registry_csc()
    );
  }

  // ─── User Agent Tests ───────────────────────────────────────────────────────

  #[test]
  fn user_agent_derived_format_without_override() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).client_version("6.0.0").build();

    assert_eq!(
      profile.user_agent(),
      format!("PAN GlobalProtect/6.0.0 ({})", default_os_version(&ClientOs::Linux))
    );
  }

  #[test]
  fn user_agent_explicit_override_returns_custom_value() {
    let profile = OsProfileBuilder::new(ClientOs::Linux)
      .client_version("6.0.0")
      .user_agent("custom/1.0")
      .build();

    assert_eq!(profile.user_agent(), "custom/1.0");
  }

  #[test]
  fn user_agent_empty_override_falls_back_to_derived() {
    let profile = OsProfileBuilder::new(ClientOs::Linux)
      .client_version("6.0.0")
      .user_agent("")
      .build();

    assert_eq!(
      profile.user_agent(),
      format!("PAN GlobalProtect/6.0.0 ({})", default_os_version(&ClientOs::Linux))
    );
  }

  #[test]
  fn user_agent_whitespace_only_override_falls_back_to_derived() {
    let profile = OsProfileBuilder::new(ClientOs::Linux)
      .client_version("6.0.0")
      .user_agent("   ")
      .build();

    assert_eq!(
      profile.user_agent(),
      format!("PAN GlobalProtect/6.0.0 ({})", default_os_version(&ClientOs::Linux))
    );
  }

  #[test]
  fn user_agent_derived_is_non_empty_for_all_client_os_variants() {
    for client_os in [ClientOs::Linux, ClientOs::Windows, ClientOs::Mac] {
      let profile = OsProfileBuilder::new(client_os).build();
      assert!(
        !profile.user_agent().is_empty(),
        "user_agent() should be non-empty for {:?}",
        client_os
      );
    }
  }

  #[test]
  fn native_webview_user_agent_uses_native_default() {
    let profile = OsProfileBuilder::new(runtime_client_os()).build();
    let user_agent = profile.webview_user_agent();

    assert_eq!(
      user_agent,
      WebviewUserAgent::Native {
        prefix: profile.user_agent().to_string(),
        transform: target::webview_user_agent_transform(runtime_client_os())
      }
    );
  }

  #[test]
  fn projected_linux_webview_user_agent_uses_linux_default() {
    if runtime_client_os() == ClientOs::Linux {
      return;
    }

    let client_os = ClientOs::Linux;
    let profile = OsProfileBuilder::new(client_os).build();
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
    if runtime_client_os() == ClientOs::Mac {
      return;
    }

    let client_os = ClientOs::Mac;
    let profile = OsProfileBuilder::new(client_os).build();
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
    if runtime_client_os() == ClientOs::Windows {
      return;
    }

    let client_os = ClientOs::Windows;
    let profile = OsProfileBuilder::new(client_os).build();
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
    assert!(default_os_version(&ClientOs::Linux).starts_with("Linux "));
    assert!(default_os_version(&ClientOs::Mac).starts_with("Apple Mac OS X "));
    assert!(default_os_version(&ClientOs::Windows).starts_with("Microsoft "));
    assert_eq!(default_software_version(&ClientOs::Linux), "");
    assert!(!default_software_version(&ClientOs::Mac).is_empty());
    assert!(!default_software_version(&ClientOs::Windows).is_empty());
  }

  #[test]
  fn builder_fills_defaults_when_nothing_specified() {
    let profile = OsProfileBuilder::new(ClientOs::Linux).build();

    assert_eq!(profile.client_os(), ClientOs::Linux);
    assert!(!profile.computer().is_empty());
    assert!(profile.os_version().starts_with("Linux "));
    assert_eq!(profile.client_version(), GP_CLIENT_VERSION_LINUX);
    assert!(!profile.host_id().is_empty());
  }

  #[test]
  fn builder_delegates_default_identity_to_host_identity() {
    let expected = HostIdentity::collect();
    let profile = OsProfileBuilder::new(runtime_client_os())
      .host_identity(expected.clone())
      .build();

    assert_eq!(profile.host_identity(), &expected);
  }

  #[test]
  fn builder_host_id_override_uses_supplied_runtime_identity_seed() {
    let profile = OsProfileBuilder::new(runtime_client_os())
      .host_id_override("provided-host-id")
      .build();

    assert_eq!(profile.host_identity().host_id(), "provided-host-id");
  }

  #[test]
  fn builder_host_id_override_is_used_as_projected_identity_seed() {
    let client_os = match runtime_client_os() {
      ClientOs::Linux => ClientOs::Mac,
      ClientOs::Mac | ClientOs::Windows => ClientOs::Linux,
    };

    let first = OsProfileBuilder::new(client_os)
      .host_id_override("provided-host-id")
      .build();
    let second = OsProfileBuilder::new(client_os)
      .host_id_override("provided-host-id")
      .build();

    assert_eq!(first.host_identity().host_id(), "provided-host-id");
    assert_eq!(first.host_id(), second.host_id());
    assert_ne!(first.host_id(), "provided-host-id");
  }

  #[test]
  fn builder_uses_explicit_values_when_provided() {
    let identity = HostIdentity::new(
      "identity-computer".to_string(),
      "test-host-id".to_string(),
      "test-serial".to_string(),
      "aa:bb:cc:dd:ee:ff".to_string(),
    );
    let profile = OsProfileBuilder::new(ClientOs::Mac)
      .computer_name_override("MyMac")
      .client_version("6.2.0-100")
      .host_identity(identity.clone())
      .build();

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
      .build();

    // Should fall back to defaults, not use empty strings
    assert!(!profile.computer().is_empty());
    assert!(profile.os_version().starts_with("Microsoft "));
    assert_eq!(profile.client_version(), GP_CLIENT_VERSION_WINDOWS);
  }

  #[test]
  fn identity_accessors_delegate_to_host_identity() {
    let identity = HostIdentity::new(
      "test-computer".to_string(),
      "host-123".to_string(),
      "serial-456".to_string(),
      "11:22:33:44:55:66".to_string(),
    );
    let profile = OsProfileBuilder::new(runtime_client_os())
      .host_identity(identity)
      .build();

    assert!(!profile.host_id().is_empty());
    assert_eq!(profile.serialno(), "serial-456");
    assert!(!profile.mac_addr().is_empty());
  }

  #[test]
  fn local_hostname_override_changes_only_projected_computer() {
    let identity = HostIdentity::new(
      "real-computer".to_string(),
      "real-host-id".to_string(),
      "real-serial".to_string(),
      "11:22:33:44:55:66".to_string(),
    );

    let profile = OsProfileBuilder::new(runtime_client_os())
      .host_identity(identity)
      .computer_name_override("profile-computer")
      .build();

    assert_eq!(profile.computer(), "profile-computer");
    assert_eq!(profile.host_identity().computer(), "real-computer");
  }

  #[test]
  fn simulated_profile_identity_is_derived_from_real_host_id() {
    let client_os = non_native_client_os();
    let identity = HostIdentity::new(
      "real-computer".to_string(),
      "stable-real-host-id".to_string(),
      "real-serial".to_string(),
      "11:22:33:44:55:66".to_string(),
    );

    let first = OsProfileBuilder::new(client_os).host_identity(identity.clone()).build();
    let second = OsProfileBuilder::new(client_os).host_identity(identity).build();

    assert_eq!(first.host_identity().host_id(), "stable-real-host-id");
    assert_ne!(first.host_id(), "stable-real-host-id");
    assert_eq!(first.host_id(), second.host_id());
    assert_eq!(first.serialno(), second.serialno());
    assert_eq!(first.mac_addr(), second.mac_addr());
  }

  #[test]
  fn simulated_linux_serial_uses_vmware_style_uuid_bytes() {
    if runtime_client_os() == ClientOs::Linux {
      return;
    }

    let identity = HostIdentity::new(
      "real-computer".to_string(),
      "stable-real-host-id".to_string(),
      "real-serial".to_string(),
      "11:22:33:44:55:66".to_string(),
    );

    let profile = OsProfileBuilder::new(ClientOs::Linux).host_identity(identity).build();

    assert!(profile.serialno().starts_with("VMware-"));
    assert_eq!(
      profile.serialno().len(),
      "VMware-56 4d 78 5a 61 64 ac 19-9e a9 d3 6a 3b 9c 6c ef".len()
    );
  }

  #[test]
  fn profile_os_versions_are_not_host_identity_fields() {
    let identity = HostIdentity::new(
      "real-computer".to_string(),
      "stable-real-host-id".to_string(),
      "real-serial".to_string(),
      "11:22:33:44:55:66".to_string(),
    );
    let profile = OsProfileBuilder::new(ClientOs::Mac).host_identity(identity).build();
    let value = serde_json::to_value(profile.host_identity()).unwrap();

    assert!(value.get("osVersion").is_none());
    assert!(value.get("softwareVersion").is_none());
    assert!(profile.os_version().starts_with("Apple Mac OS X "));
    assert!(!profile.software_version().is_empty());
  }

  fn non_native_client_os() -> ClientOs {
    match runtime_client_os() {
      ClientOs::Linux => ClientOs::Mac,
      ClientOs::Mac | ClientOs::Windows => ClientOs::Linux,
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
          .build();

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
          .build();

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
          .build();

        // The result must be the derived format, not the whitespace string
        let expected = format!("PAN GlobalProtect/6.0.0 ({})", profile.os_version());
        prop_assert_eq!(profile.user_agent(), expected.as_str());
        prop_assert_ne!(profile.user_agent(), ws.as_str());
      }
    }
  }
}
