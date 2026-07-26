use std::process::{Command, Stdio};

use log::{debug, info, warn};

use crate::Browser;

const GOOGLE_CHROME_BROWSERS: &[&str] = &["google-chrome-stable", "google-chrome"];
const CHROMIUM_BROWSERS: &[&str] = &["chromium", "chromium-browser"];
const EDGE_BROWSERS: &[&str] = &["microsoft-edge-stable", "microsoft-edge"];
const EDGE_BETA_BROWSERS: &[&str] = &["microsoft-edge-beta"];
const EDGE_DEV_BROWSERS: &[&str] = &["microsoft-edge-dev"];
const FIREFOX_BROWSERS: &[&str] = &["firefox"];
const FIREFOX_ESR_BROWSERS: &[&str] = &["firefox-esr"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherSpec {
  Programs(&'static [&'static str]),
  Flatpak(&'static str),
}

#[derive(Debug, PartialEq, Eq)]
enum BrowserLauncher {
  Executable(String),
  Flatpak { executable: String, app_id: &'static str },
}

pub(super) fn open_url(url: &str, browser: Browser<'_>) -> anyhow::Result<()> {
  match browser {
    Browser::Auto => open_auto_browser(url),
    Browser::Default => open_default_browser(url),
    Browser::Chrome => {
      let browser = find_chrome_browser().unwrap_or_else(|| browser.as_str().to_string());
      open_with_browser(url, &browser)
    }
    Browser::Edge => {
      let browser = find_edge_browser().unwrap_or_else(|| browser.as_str().to_string());
      open_with_browser(url, &browser)
    }
    Browser::Firefox => {
      let browser = find_firefox_browser().unwrap_or_else(|| browser.as_str().to_string());
      open_with_browser(url, &browser)
    }
    Browser::Safari | Browser::Other(_) => open_with_browser(url, browser.as_str()),
  }
}

fn open_auto_browser(url: &str) -> anyhow::Result<()> {
  let default_launcher = find_default_browser_launcher();
  if let Some(launcher) = default_launcher.as_ref() {
    if let Err(err) = open_with_launcher(url, launcher) {
      warn!("Failed to launch recognized default browser: {err}");
    } else {
      return Ok(());
    }
  }

  if let Some(launcher) = find_auto_browser_launcher()
    && Some(&launcher) != default_launcher.as_ref()
  {
    if let Err(err) = open_with_launcher(url, &launcher) {
      warn!("Failed to launch an automatically detected browser: {err}");
    } else {
      return Ok(());
    }
  }

  warn!("Falling back to the system URL handler");
  open_default_browser(url)
}

fn open_default_browser(url: &str) -> anyhow::Result<()> {
  if webbrowser::open(url).is_ok() {
    return Ok(());
  }

  open::that_detached(url).map_err(Into::into)
}

fn open_with_browser(url: &str, browser: &str) -> anyhow::Result<()> {
  info!("Launching browser: {}", browser);
  open::with_detached(url, browser).map_err(Into::into)
}

fn open_with_launcher(url: &str, launcher: &BrowserLauncher) -> anyhow::Result<()> {
  match launcher {
    BrowserLauncher::Executable(executable) => open_with_browser(url, executable),
    BrowserLauncher::Flatpak { executable, app_id } => {
      info!("Launching Flatpak browser: {app_id}");
      Command::new(executable)
        .args(["run", app_id, url])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
      Ok(())
    }
  }
}

fn find_default_browser_launcher() -> Option<BrowserLauncher> {
  let desktop_id = default_browser_desktop_id()?;
  let Some(spec) = launcher_spec_for_desktop_id(&desktop_id) else {
    warn!("Default browser desktop ID is not recognized: {desktop_id}");
    return None;
  };

  let launcher = resolve_launcher(spec);
  if launcher.is_none() {
    warn!("Default browser is recognized but its launcher is unavailable: {desktop_id}");
  }
  launcher
}

fn default_browser_desktop_id() -> Option<String> {
  let output = match Command::new("xdg-settings")
    .args(["get", "default-web-browser"])
    .output()
  {
    Ok(output) => output,
    Err(err) => {
      debug!("Unable to query the default browser: {err}");
      return None;
    }
  };

  if !output.status.success() {
    debug!("xdg-settings could not determine the default browser");
    return None;
  }

  let desktop_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
  (!desktop_id.is_empty()).then_some(desktop_id)
}

fn launcher_spec_for_desktop_id(desktop_id: &str) -> Option<LauncherSpec> {
  match desktop_id {
    "google-chrome.desktop" | "google-chrome-stable.desktop" => Some(LauncherSpec::Programs(GOOGLE_CHROME_BROWSERS)),
    "chromium.desktop" | "chromium-browser.desktop" | "chromium_chromium.desktop" => {
      Some(LauncherSpec::Programs(CHROMIUM_BROWSERS))
    }
    "microsoft-edge.desktop" | "microsoft-edge-stable.desktop" => Some(LauncherSpec::Programs(EDGE_BROWSERS)),
    "microsoft-edge-beta.desktop" => Some(LauncherSpec::Programs(EDGE_BETA_BROWSERS)),
    "microsoft-edge-dev.desktop" => Some(LauncherSpec::Programs(EDGE_DEV_BROWSERS)),
    "firefox.desktop" | "firefox_firefox.desktop" => Some(LauncherSpec::Programs(FIREFOX_BROWSERS)),
    "firefox-esr.desktop" => Some(LauncherSpec::Programs(FIREFOX_ESR_BROWSERS)),
    "com.google.Chrome.desktop" => Some(LauncherSpec::Flatpak("com.google.Chrome")),
    "org.chromium.Chromium.desktop" => Some(LauncherSpec::Flatpak("org.chromium.Chromium")),
    "com.microsoft.Edge.desktop" => Some(LauncherSpec::Flatpak("com.microsoft.Edge")),
    "org.mozilla.firefox.desktop" => Some(LauncherSpec::Flatpak("org.mozilla.firefox")),
    _ => None,
  }
}

fn resolve_launcher(spec: LauncherSpec) -> Option<BrowserLauncher> {
  match spec {
    LauncherSpec::Programs(programs) => find_programs(programs).map(BrowserLauncher::Executable),
    LauncherSpec::Flatpak(app_id) => {
      find_program("flatpak").map(|executable| BrowserLauncher::Flatpak { executable, app_id })
    }
  }
}

fn find_auto_browser_launcher() -> Option<BrowserLauncher> {
  find_auto_browser().map(BrowserLauncher::Executable)
}

fn find_auto_browser() -> Option<String> {
  find_chrome_browser()
    .or_else(find_edge_browser)
    .or_else(find_firefox_browser)
}

fn find_chrome_browser() -> Option<String> {
  find_programs(GOOGLE_CHROME_BROWSERS).or_else(|| find_programs(CHROMIUM_BROWSERS))
}

fn find_edge_browser() -> Option<String> {
  find_programs(EDGE_BROWSERS)
    .or_else(|| find_programs(EDGE_BETA_BROWSERS))
    .or_else(|| find_programs(EDGE_DEV_BROWSERS))
}

fn find_firefox_browser() -> Option<String> {
  find_programs(FIREFOX_BROWSERS).or_else(|| find_programs(FIREFOX_ESR_BROWSERS))
}

fn find_programs(programs: &[&str]) -> Option<String> {
  programs.iter().find_map(|program| find_program(program))
}

fn find_program(name: &str) -> Option<String> {
  which::which(name).ok().map(|path| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn auto_candidates_try_chrome_then_edge_then_firefox() {
    let candidates = GOOGLE_CHROME_BROWSERS
      .iter()
      .chain(CHROMIUM_BROWSERS)
      .chain(EDGE_BROWSERS)
      .chain(EDGE_BETA_BROWSERS)
      .chain(EDGE_DEV_BROWSERS)
      .chain(FIREFOX_BROWSERS)
      .chain(FIREFOX_ESR_BROWSERS)
      .copied()
      .collect::<Vec<_>>();

    assert_eq!(
      candidates,
      vec![
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
        "microsoft-edge-stable",
        "microsoft-edge",
        "microsoft-edge-beta",
        "microsoft-edge-dev",
        "firefox",
        "firefox-esr",
      ]
    );
  }

  #[test]
  fn known_desktop_ids_map_to_expected_launchers() {
    let cases = [
      ("google-chrome.desktop", LauncherSpec::Programs(GOOGLE_CHROME_BROWSERS)),
      ("chromium.desktop", LauncherSpec::Programs(CHROMIUM_BROWSERS)),
      ("chromium-browser.desktop", LauncherSpec::Programs(CHROMIUM_BROWSERS)),
      ("chromium_chromium.desktop", LauncherSpec::Programs(CHROMIUM_BROWSERS)),
      ("microsoft-edge.desktop", LauncherSpec::Programs(EDGE_BROWSERS)),
      (
        "microsoft-edge-beta.desktop",
        LauncherSpec::Programs(EDGE_BETA_BROWSERS),
      ),
      ("microsoft-edge-dev.desktop", LauncherSpec::Programs(EDGE_DEV_BROWSERS)),
      ("firefox.desktop", LauncherSpec::Programs(FIREFOX_BROWSERS)),
      ("firefox-esr.desktop", LauncherSpec::Programs(FIREFOX_ESR_BROWSERS)),
      ("firefox_firefox.desktop", LauncherSpec::Programs(FIREFOX_BROWSERS)),
      ("com.google.Chrome.desktop", LauncherSpec::Flatpak("com.google.Chrome")),
      (
        "org.chromium.Chromium.desktop",
        LauncherSpec::Flatpak("org.chromium.Chromium"),
      ),
      (
        "com.microsoft.Edge.desktop",
        LauncherSpec::Flatpak("com.microsoft.Edge"),
      ),
      (
        "org.mozilla.firefox.desktop",
        LauncherSpec::Flatpak("org.mozilla.firefox"),
      ),
    ];

    for (desktop_id, expected) in cases {
      assert_eq!(launcher_spec_for_desktop_id(desktop_id), Some(expected));
    }
  }

  #[test]
  fn unknown_desktop_id_is_not_used() {
    assert_eq!(launcher_spec_for_desktop_id("code.desktop"), None);
  }
}
