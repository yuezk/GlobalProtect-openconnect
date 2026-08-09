use std::process::{Command, Stdio};

use anyhow::Context;
use log::{debug, info, warn};

use crate::Browser;

const GOOGLE_CHROME_BROWSERS: &[&str] = &["google-chrome-stable", "google-chrome"];
const CHROMIUM_BROWSERS: &[&str] = &["chromium", "chromium-browser"];
const EDGE_BROWSERS: &[&str] = &["microsoft-edge-stable", "microsoft-edge"];
const EDGE_BETA_BROWSERS: &[&str] = &["microsoft-edge-beta"];
const EDGE_DEV_BROWSERS: &[&str] = &["microsoft-edge-dev"];
const FIREFOX_BROWSERS: &[&str] = &["firefox"];
const FIREFOX_ESR_BROWSERS: &[&str] = &["firefox-esr"];
const BRAVE_BROWSERS: &[&str] = &["brave-browser-stable"];
const VIVALDI_BROWSERS: &[&str] = &["vivaldi-stable"];
const FALKON_BROWSERS: &[&str] = &["falkon"];
const EPIPHANY_BROWSERS: &[&str] = &["epiphany"];
const OPERA_BROWSERS: &[&str] = &["opera"];
const LIBREWOLF_BROWSERS: &[&str] = &["librewolf"];
const WATERFOX_BROWSERS: &[&str] = &["waterfox"];
const QUTEBROWSER_BROWSERS: &[&str] = &["qutebrowser"];

const AUTO_BROWSER_PROGRAMS: &[&[&str]] = &[
  GOOGLE_CHROME_BROWSERS,
  CHROMIUM_BROWSERS,
  EDGE_BROWSERS,
  EDGE_BETA_BROWSERS,
  EDGE_DEV_BROWSERS,
  FIREFOX_BROWSERS,
  FIREFOX_ESR_BROWSERS,
  BRAVE_BROWSERS,
  VIVALDI_BROWSERS,
  FALKON_BROWSERS,
  EPIPHANY_BROWSERS,
  OPERA_BROWSERS,
  LIBREWOLF_BROWSERS,
  WATERFOX_BROWSERS,
  QUTEBROWSER_BROWSERS,
];

const AUTO_BROWSER_FLATPAK_IDS: &[&str] = &[
  "com.google.Chrome",
  "org.chromium.Chromium",
  "com.microsoft.Edge",
  "org.mozilla.firefox",
  "com.brave.Browser",
  "com.vivaldi.Vivaldi",
  "org.kde.falkon",
  "org.gnome.Epiphany",
  "com.opera.Opera",
  "io.gitlab.librewolf-community",
  "net.waterfox.waterfox",
  "org.qutebrowser.qutebrowser",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LauncherSpec {
  Programs(&'static [&'static str]),
  Flatpak(&'static str),
  ProgramsOrFlatpak {
    programs: &'static [&'static str],
    app_id: &'static str,
  },
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
    Browser::Chrome => open_resolved_browser(url, find_chrome_browser(), "Chrome is not available."),
    Browser::Edge => open_resolved_browser(url, find_edge_browser(), "Edge is not available."),
    Browser::Firefox => open_resolved_browser(url, find_firefox_browser(), "Firefox is not available."),
    Browser::Safari => open_resolved_browser(url, find_program(browser.as_str()), "Safari is not available."),
    Browser::Other(browser) => open_resolved_browser(
      url,
      find_program(browser.trim()),
      "Configured browser is not available.",
    ),
  }
}

fn open_resolved_browser(
  url: &str,
  executable: Option<String>,
  unavailable_message: &'static str,
) -> anyhow::Result<()> {
  let executable = executable.context(unavailable_message)?;
  open_with_browser(url, &executable)
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
    "org.mozilla.firefox.desktop" => Some(LauncherSpec::ProgramsOrFlatpak {
      programs: FIREFOX_BROWSERS,
      app_id: "org.mozilla.firefox",
    }),
    "com.brave.Browser.desktop" => Some(LauncherSpec::Flatpak("com.brave.Browser")),
    "vivaldi-stable.desktop" => Some(LauncherSpec::Programs(VIVALDI_BROWSERS)),
    "com.vivaldi.Vivaldi.desktop" => Some(LauncherSpec::Flatpak("com.vivaldi.Vivaldi")),
    "org.kde.falkon.desktop" => Some(LauncherSpec::ProgramsOrFlatpak {
      programs: FALKON_BROWSERS,
      app_id: "org.kde.falkon",
    }),
    "org.gnome.Epiphany.desktop" => Some(LauncherSpec::ProgramsOrFlatpak {
      programs: EPIPHANY_BROWSERS,
      app_id: "org.gnome.Epiphany",
    }),
    "com.opera.Opera.desktop" => Some(LauncherSpec::Flatpak("com.opera.Opera")),
    "io.gitlab.librewolf-community.desktop" => Some(LauncherSpec::Flatpak("io.gitlab.librewolf-community")),
    "net.waterfox.waterfox.desktop" => Some(LauncherSpec::Flatpak("net.waterfox.waterfox")),
    "org.qutebrowser.qutebrowser.desktop" => Some(LauncherSpec::ProgramsOrFlatpak {
      programs: QUTEBROWSER_BROWSERS,
      app_id: "org.qutebrowser.qutebrowser",
    }),
    _ => None,
  }
}

fn resolve_launcher(spec: LauncherSpec) -> Option<BrowserLauncher> {
  match spec {
    LauncherSpec::Programs(programs) => find_programs(programs).map(BrowserLauncher::Executable),
    LauncherSpec::Flatpak(app_id) => find_flatpak_browser(app_id),
    LauncherSpec::ProgramsOrFlatpak { programs, app_id } => find_programs(programs)
      .map(BrowserLauncher::Executable)
      .or_else(|| find_flatpak_browser(app_id)),
  }
}

fn find_flatpak_browser(app_id: &'static str) -> Option<BrowserLauncher> {
  let executable = find_program("flatpak")?;
  let installed = Command::new(&executable)
    .args(["info", app_id])
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .is_ok_and(|status| status.success());

  installed.then_some(BrowserLauncher::Flatpak { executable, app_id })
}

fn find_auto_browser_launcher() -> Option<BrowserLauncher> {
  find_auto_browser()
    .map(BrowserLauncher::Executable)
    .or_else(find_auto_flatpak_browser)
}

fn find_auto_browser() -> Option<String> {
  AUTO_BROWSER_PROGRAMS
    .iter()
    .find_map(|programs| find_programs(programs))
}

fn find_auto_flatpak_browser() -> Option<BrowserLauncher> {
  let executable = find_program("flatpak")?;
  let output = Command::new(&executable)
    .args(["list", "--app", "--columns=application"])
    .stdin(Stdio::null())
    .stderr(Stdio::null())
    .output()
    .ok()?;

  if !output.status.success() {
    return None;
  }

  let installed_apps = String::from_utf8_lossy(&output.stdout);
  let app_id = find_auto_flatpak_app_id(&installed_apps)?;

  Some(BrowserLauncher::Flatpak { executable, app_id })
}

fn find_auto_flatpak_app_id(installed_apps: &str) -> Option<&'static str> {
  AUTO_BROWSER_FLATPAK_IDS
    .iter()
    .copied()
    .find(|app_id| installed_apps.lines().any(|installed| installed.trim() == *app_id))
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
  fn auto_candidates_use_the_expected_order() {
    let candidates = AUTO_BROWSER_PROGRAMS
      .iter()
      .flat_map(|programs| programs.iter())
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
        "brave-browser-stable",
        "vivaldi-stable",
        "falkon",
        "epiphany",
        "opera",
        "librewolf",
        "waterfox",
        "qutebrowser",
      ]
    );

    assert_eq!(
      AUTO_BROWSER_FLATPAK_IDS,
      [
        "com.google.Chrome",
        "org.chromium.Chromium",
        "com.microsoft.Edge",
        "org.mozilla.firefox",
        "com.brave.Browser",
        "com.vivaldi.Vivaldi",
        "org.kde.falkon",
        "org.gnome.Epiphany",
        "com.opera.Opera",
        "io.gitlab.librewolf-community",
        "net.waterfox.waterfox",
        "org.qutebrowser.qutebrowser",
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
        LauncherSpec::ProgramsOrFlatpak {
          programs: FIREFOX_BROWSERS,
          app_id: "org.mozilla.firefox",
        },
      ),
      ("com.brave.Browser.desktop", LauncherSpec::Flatpak("com.brave.Browser")),
      ("vivaldi-stable.desktop", LauncherSpec::Programs(VIVALDI_BROWSERS)),
      (
        "com.vivaldi.Vivaldi.desktop",
        LauncherSpec::Flatpak("com.vivaldi.Vivaldi"),
      ),
      (
        "org.kde.falkon.desktop",
        LauncherSpec::ProgramsOrFlatpak {
          programs: FALKON_BROWSERS,
          app_id: "org.kde.falkon",
        },
      ),
      (
        "org.gnome.Epiphany.desktop",
        LauncherSpec::ProgramsOrFlatpak {
          programs: EPIPHANY_BROWSERS,
          app_id: "org.gnome.Epiphany",
        },
      ),
      ("com.opera.Opera.desktop", LauncherSpec::Flatpak("com.opera.Opera")),
      (
        "io.gitlab.librewolf-community.desktop",
        LauncherSpec::Flatpak("io.gitlab.librewolf-community"),
      ),
      (
        "net.waterfox.waterfox.desktop",
        LauncherSpec::Flatpak("net.waterfox.waterfox"),
      ),
      (
        "org.qutebrowser.qutebrowser.desktop",
        LauncherSpec::ProgramsOrFlatpak {
          programs: QUTEBROWSER_BROWSERS,
          app_id: "org.qutebrowser.qutebrowser",
        },
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

  #[test]
  fn auto_flatpak_candidates_use_priority_and_exact_ids() {
    assert_eq!(
      find_auto_flatpak_app_id("org.kde.falkon\ncom.brave.Browser\n"),
      Some("com.brave.Browser")
    );
    assert_eq!(find_auto_flatpak_app_id("com.brave.Browser.beta\n"), None);
  }

  #[test]
  fn unavailable_custom_browser_returns_an_error() {
    let err = open_url(
      "https://example.com",
      Browser::Other("gpgui-browser-that-does-not-exist"),
    )
    .unwrap_err();

    assert_eq!(err.to_string(), "Configured browser is not available.");
  }
}
