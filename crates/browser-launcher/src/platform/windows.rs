//! Windows browsers are resolved through their registered App Paths.
use crate::Browser;
use anyhow::{Context, bail};
use std::path::PathBuf;
use winreg::{
  RegKey,
  enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
};

pub(super) fn open_url(url: &str, browser: Browser<'_>) -> anyhow::Result<()> {
  let executable = match browser {
    Browser::Auto | Browser::Default => return webbrowser::open(url).context("Could not open the browser"),
    Browser::Chrome => registered_browser("chrome.exe").context("Chrome is not installed")?,
    Browser::Edge => registered_browser("msedge.exe").context("Edge is not installed")?,
    Browser::Firefox => registered_browser("firefox.exe").context("Firefox is not installed")?,
    Browser::Safari => bail!("Safari is not available on Windows"),
    Browser::Other(program) => PathBuf::from(program),
  };
  std::process::Command::new(executable)
    .arg(url)
    .spawn()
    .context("Could not open the configured browser")?;
  Ok(())
}

fn registered_browser(name: &str) -> Option<PathBuf> {
  [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().find_map(|hive| {
    let key = RegKey::predef(hive)
      .open_subkey(format!(r"Software\Microsoft\Windows\CurrentVersion\App Paths\{name}"))
      .ok()?;
    let path: String = key.get_value("").ok()?;
    let path = PathBuf::from(path.trim_matches('"'));
    path.is_file().then_some(path)
  })
}
