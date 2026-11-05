use super::ClientOs;
pub(super) const DEFAULT_MACOS_VERSION: &str = "13.4.0";
pub(super) const DEFAULT_LINUX_DISTRO: &str = "Ubuntu 24.04.3 LTS";
pub(super) const DEFAULT_WINDOWS_DISTRO: &str = "Windows 11 Pro";
pub(super) const DEFAULT_WINDOWS_VERSION: &str = "10.0.22631.0";
pub(super) const DEFAULT_LINUX_WEBVIEW_USER_AGENT: &str =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/602.1 (KHTML, like Gecko) PanGPUI Version/10.0 Safari/602.1";
pub(super) const DEFAULT_MACOS_WEBVIEW_USER_AGENT: &str =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
pub(super) const DEFAULT_WINDOWS_WEBVIEW_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/144.0.0.0 Safari/537.36 Edg/144.0.0.0";

pub(crate) fn os_version(os: ClientOs) -> String {
  match os {
    ClientOs::Linux => format!("Linux {DEFAULT_LINUX_DISTRO}"),
    ClientOs::Mac => format!("Apple Mac OS X {DEFAULT_MACOS_VERSION}"),
    ClientOs::Windows => format!("Microsoft {DEFAULT_WINDOWS_DISTRO} , 64-bit"),
    ClientOs::Ios => "Apple iOS".into(),
  }
}
pub(crate) fn software_version(os: ClientOs) -> String {
  match os {
    ClientOs::Mac => DEFAULT_MACOS_VERSION.into(),
    ClientOs::Windows => DEFAULT_WINDOWS_VERSION.into(),
    ClientOs::Linux | ClientOs::Ios => String::new(),
  }
}
