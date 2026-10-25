use super::*;
/// Observed host facts supplied by the platform owner.
#[derive(Debug, Clone)]
pub struct HostSnapshot {
  pub runtime_os: ClientOs,
  pub identity: HostIdentity,
  pub os_version: String,
  pub software_version: String,
}
// ─── ProfileHostIdentity ────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProfileHostIdentity {
  pub(super) computer: String,
  pub(super) host_id: String,
  pub(super) serial_number: String,
  pub(super) mac_address: String,
}

impl ProfileHostIdentity {
  pub(super) fn for_client_os(
    runtime_os: ClientOs,
    client_os: ClientOs,
    host_identity: &HostIdentity,
    computer_override: Option<String>,
  ) -> Self {
    projection::project(runtime_os, client_os, host_identity, computer_override)
  }

  pub(super) fn computer(&self) -> &str {
    &self.computer
  }

  pub(super) fn host_id(&self) -> &str {
    &self.host_id
  }

  pub(super) fn serialno(&self) -> &str {
    &self.serial_number
  }

  pub(super) fn mac_addr(&self) -> &str {
    &self.mac_address
  }
}

// ─── OsProfile ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct OsProfile {
  runtime_os: ClientOs,
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
    self.runtime_os == self.client_os
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
  pub(super) client_os: ClientOs,
  pub(super) client_version: Option<String>,
  pub(super) os_version_override: Option<String>,
  pub(super) computer_name_override: Option<String>,
  pub(super) host_identity: Option<HostIdentity>,
  pub(super) user_agent: Option<String>,
}

impl OsProfileBuilder {
  pub fn new(client_os: ClientOs) -> Self {
    Self {
      client_os,
      client_version: None,
      os_version_override: None,
      computer_name_override: None,
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

  /// Preserve an explicitly negotiated operating system version.
  pub fn os_version(mut self, version: impl Into<String>) -> Self {
    let version = version.into();
    self.os_version_override = if version.trim().is_empty() { None } else { Some(version) };
    self
  }

  pub fn host_identity(mut self, identity: HostIdentity) -> Self {
    self.host_identity = Some(identity);
    self
  }

  pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
    let ua = ua.into();
    self.user_agent = if ua.trim().is_empty() { None } else { Some(ua) };
    self
  }

  /// Project supplied host facts without inspecting the machine running this code.
  pub fn build(self, snapshot: HostSnapshot) -> OsProfile {
    let (os_version, software_version) = if self.client_os == snapshot.runtime_os {
      (snapshot.os_version, snapshot.software_version)
    } else {
      (
        defaults::os_version(self.client_os),
        defaults::software_version(self.client_os),
      )
    };
    let identity = self.host_identity.clone().unwrap_or(snapshot.identity);
    self.finish(snapshot.runtime_os, identity, os_version, software_version)
  }

  fn finish(
    self,
    runtime_os: ClientOs,
    host_identity: HostIdentity,
    default_os_version: String,
    software_version: String,
  ) -> OsProfile {
    let client_version = self
      .client_version
      .filter(|version| !version.trim().is_empty())
      .unwrap_or_else(|| self.client_os.default_client_version().into());
    let profile_identity =
      ProfileHostIdentity::for_client_os(runtime_os, self.client_os, &host_identity, self.computer_name_override);
    let os_version = self.os_version_override.unwrap_or(default_os_version);
    let user_agent = self
      .user_agent
      .unwrap_or_else(|| format!("{GP_USER_AGENT}/{client_version} ({os_version})"));
    OsProfile {
      client_os: self.client_os,
      runtime_os,
      os_version,
      software_version,
      os_vendor: target::os_vendor(self.client_os),
      client_version,
      host_identity,
      profile_identity,
      user_agent,
    }
  }
}
