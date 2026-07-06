#[cfg(feature = "launcher")]
use log::info;

#[cfg(any(feature = "launcher", test))]
const CHROME_BROWSERS: &[&str] = &["google-chrome-stable", "google-chrome", "chromium"];
#[cfg(any(feature = "launcher", test))]
const EDGE_BROWSERS: &[&str] = &[
  "microsoft-edge-stable",
  "microsoft-edge",
  "microsoft-edge-beta",
  "microsoft-edge-dev",
];
#[cfg(any(feature = "launcher", test))]
const FIREFOX_BROWSER: &str = "firefox";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Browser<'a> {
  Auto,
  Default,
  Chrome,
  Edge,
  Firefox,
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
      _ => Browser::Other(value),
    }
  }

  #[cfg(feature = "launcher")]
  fn as_str(&self) -> &str {
    match self {
      Browser::Auto => "auto",
      Browser::Default => "default",
      Browser::Chrome => "chrome",
      Browser::Edge => "edge",
      Browser::Firefox => FIREFOX_BROWSER,
      Browser::Other(browser) => browser,
    }
  }
}

#[cfg(feature = "launcher")]
pub fn open_url(url: &str, browser: Browser<'_>) -> anyhow::Result<()> {
  match browser {
    Browser::Auto => {
      if let Some(browser) = find_auto_browser() {
        open_with_browser(url, &browser).or_else(|_| open_default_browser(url))
      } else {
        open_default_browser(url)
      }
    }
    Browser::Default => open_default_browser(url),
    Browser::Chrome => {
      let browser = find_chrome_browser().unwrap_or_else(|| browser.as_str().to_string());
      open_with_browser(url, &browser)
    }
    Browser::Edge => {
      let browser = find_edge_browser().unwrap_or_else(|| browser.as_str().to_string());
      open_with_browser(url, &browser)
    }
    Browser::Firefox | Browser::Other(_) => open_with_browser(url, browser.as_str()),
  }
}

#[cfg(feature = "launcher")]
fn open_default_browser(url: &str) -> anyhow::Result<()> {
  if webbrowser::open(url).is_ok() {
    return Ok(());
  }

  open::that_detached(url).map_err(Into::into)
}

#[cfg(feature = "launcher")]
fn open_with_browser(url: &str, browser: &str) -> anyhow::Result<()> {
  info!("Launching browser: {}", browser);
  open::with_detached(url, browser).map_err(Into::into)
}

#[cfg(feature = "launcher")]
fn find_auto_browser() -> Option<String> {
  find_chrome_browser()
    .or_else(find_edge_browser)
    .or_else(|| find_program(FIREFOX_BROWSER))
}

#[cfg(feature = "launcher")]
fn find_chrome_browser() -> Option<String> {
  CHROME_BROWSERS.iter().find_map(|browser| find_program(browser))
}

#[cfg(feature = "launcher")]
fn find_edge_browser() -> Option<String> {
  EDGE_BROWSERS.iter().find_map(|browser| find_program(browser))
}

#[cfg(feature = "launcher")]
fn find_program(name: &str) -> Option<String> {
  which::which(name).ok().map(|path| path.to_string_lossy().to_string())
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
  }

  #[test]
  fn custom_browser_names_are_preserved() {
    assert_eq!(Browser::from_name("/opt/browser"), Browser::Other("/opt/browser"));
    assert_eq!(
      Browser::from_config(Some(" custom-browser ")),
      Browser::Other(" custom-browser ")
    );
  }

  #[test]
  fn auto_candidates_try_chrome_then_edge_then_firefox() {
    let candidates = CHROME_BROWSERS
      .iter()
      .chain(EDGE_BROWSERS)
      .chain([FIREFOX_BROWSER].iter())
      .copied()
      .collect::<Vec<_>>();

    assert_eq!(
      candidates,
      vec![
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "microsoft-edge-stable",
        "microsoft-edge",
        "microsoft-edge-beta",
        "microsoft-edge-dev",
        "firefox",
      ]
    );
  }
}
