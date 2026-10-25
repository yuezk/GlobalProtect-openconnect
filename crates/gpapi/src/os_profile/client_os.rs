use super::*;
// ─── ClientOs ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Type)]
pub enum ClientOs {
  Linux,
  Windows,
  Mac,
  Ios,
}

impl From<&str> for ClientOs {
  fn from(os: &str) -> Self {
    match os {
      "Linux" => ClientOs::Linux,
      "Windows" => ClientOs::Windows,
      "Mac" => ClientOs::Mac,
      "iOS" => ClientOs::Ios,
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
      ClientOs::Ios => "iOS",
    }
  }

  pub fn to_openconnect_os(&self) -> &str {
    match self {
      ClientOs::Linux => "linux",
      ClientOs::Windows => "win",
      ClientOs::Mac => "mac-intel",
      ClientOs::Ios => "apple-ios",
    }
  }

  pub fn default_client_version(&self) -> &'static str {
    match self {
      ClientOs::Linux => GP_CLIENT_VERSION_LINUX,
      ClientOs::Windows => GP_CLIENT_VERSION_WINDOWS,
      ClientOs::Mac => GP_CLIENT_VERSION_MACOS,
      ClientOs::Ios => "6.3.5-28",
    }
  }

  pub(crate) fn default_csc_support(&self) -> bool {
    match self {
      ClientOs::Linux | ClientOs::Ios => false,
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

pub(super) mod target {
  use super::*;

  pub(in crate::os_profile) fn prelogin_param_location(client_os: ClientOs) -> PreloginParamLocation {
    match client_os {
      ClientOs::Linux | ClientOs::Mac | ClientOs::Ios => PreloginParamLocation::Body,
      ClientOs::Windows => PreloginParamLocation::Query,
    }
  }

  pub(in crate::os_profile) fn saml_password(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => "SAMLPASS",
      ClientOs::Mac | ClientOs::Windows | ClientOs::Ios => "",
    }
  }

  pub(in crate::os_profile) fn external_browser_os_value(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => "4",
      ClientOs::Mac => "3",
      ClientOs::Windows => "2",
      ClientOs::Ios => "0",
    }
  }

  pub(in crate::os_profile) fn supports_macos_plist_csc(client_os: ClientOs) -> bool {
    matches!(client_os, ClientOs::Mac)
  }

  pub(in crate::os_profile) fn supports_linux_process_csc(client_os: ClientOs) -> bool {
    matches!(client_os, ClientOs::Linux)
  }

  pub(in crate::os_profile) fn supports_windows_registry_csc(client_os: ClientOs) -> bool {
    matches!(client_os, ClientOs::Windows)
  }

  pub(in crate::os_profile) fn kerberos_support_in_query(client_os: ClientOs) -> bool {
    match client_os {
      ClientOs::Linux | ClientOs::Ios => false,
      ClientOs::Mac | ClientOs::Windows => true,
    }
  }

  pub(in crate::os_profile) fn os_vendor(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => "Linux",
      ClientOs::Mac | ClientOs::Ios => "Apple",
      ClientOs::Windows => "Microsoft",
    }
  }

  pub(in crate::os_profile) fn webview_user_agent_default(client_os: ClientOs) -> &'static str {
    match client_os {
      ClientOs::Linux => DEFAULT_LINUX_WEBVIEW_USER_AGENT,
      ClientOs::Mac | ClientOs::Ios => DEFAULT_MACOS_WEBVIEW_USER_AGENT,
      ClientOs::Windows => DEFAULT_WINDOWS_WEBVIEW_USER_AGENT,
    }
  }

  pub(in crate::os_profile) fn webview_user_agent_transform(client_os: ClientOs) -> WebviewUserAgentTransform {
    match client_os {
      ClientOs::Linux => WebviewUserAgentTransform::LinuxPanGpuiVersion,
      ClientOs::Mac | ClientOs::Windows | ClientOs::Ios => WebviewUserAgentTransform::None,
    }
  }
}
