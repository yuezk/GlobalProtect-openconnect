#[cfg(all(feature = "launcher", target_os = "macos"))]
#[path = "platform/macos.rs"]
mod platform;

#[cfg(all(feature = "launcher", not(target_os = "macos")))]
#[path = "platform/generic.rs"]
mod platform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser<'a> {
  Auto,
  Default,
  Chrome,
  Edge,
  Firefox,
  Safari,
  Other(&'a str),
}

impl<'a> Browser<'a> {
  pub fn from_config(value: Option<&'a str>) -> Self {
    let Some(value) = value.filter(|value| !value.trim().is_empty()) else {
      return Browser::Auto;
    };

    Self::from_name(value)
  }

  pub fn from_name(value: &'a str) -> Self {
    match value.trim().to_lowercase().as_str() {
      "auto" => Browser::Auto,
      "default" => Browser::Default,
      "chrome" => Browser::Chrome,
      "edge" => Browser::Edge,
      "firefox" => Browser::Firefox,
      "safari" => Browser::Safari,
      _ => Browser::Other(value),
    }
  }

  #[cfg(all(feature = "launcher", not(target_os = "macos")))]
  pub(crate) fn as_str(&self) -> &str {
    match self {
      Browser::Auto => "auto",
      Browser::Default => "default",
      Browser::Chrome => "chrome",
      Browser::Edge => "edge",
      Browser::Firefox => "firefox",
      Browser::Safari => "safari",
      Browser::Other(browser) => browser,
    }
  }
}

#[cfg(feature = "launcher")]
pub fn open_url(url: &str, browser: Browser<'_>) -> anyhow::Result<()> {
  platform::open_url(url, browser)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn empty_config_uses_auto() {
    assert_eq!(Browser::from_config(None), Browser::Auto);
    assert_eq!(Browser::from_config(Some("")), Browser::Auto);
    assert_eq!(Browser::from_config(Some("  ")), Browser::Auto);
  }

  #[test]
  fn known_browser_names_are_case_insensitive() {
    assert_eq!(Browser::from_name("AUTO"), Browser::Auto);
    assert_eq!(Browser::from_name("Default"), Browser::Default);
    assert_eq!(Browser::from_name("Chrome"), Browser::Chrome);
    assert_eq!(Browser::from_name("EDGE"), Browser::Edge);
    assert_eq!(Browser::from_name("Firefox"), Browser::Firefox);
    assert_eq!(Browser::from_name("Safari"), Browser::Safari);
  }

  #[test]
  fn custom_browser_names_are_preserved() {
    assert_eq!(Browser::from_name("/opt/browser"), Browser::Other("/opt/browser"));
    assert_eq!(
      Browser::from_config(Some(" custom-browser ")),
      Browser::Other(" custom-browser ")
    );
  }
}
