use log::info;

use crate::Browser;

const SAFARI_BUNDLE_ID: &str = "com.apple.Safari";
const CHROME_BUNDLE_ID: &str = "com.google.Chrome";
const EDGE_BUNDLE_ID: &str = "com.microsoft.edgemac";
const FIREFOX_BUNDLE_ID: &str = "org.mozilla.firefox";

pub(super) fn open_url(url: &str, browser: Browser<'_>) -> anyhow::Result<()> {
  match browser {
    Browser::Auto | Browser::Default => open_default_browser(url),
    Browser::Safari => open_browser_bundle(url, SAFARI_BUNDLE_ID),
    Browser::Chrome => open_browser_bundle(url, CHROME_BUNDLE_ID),
    Browser::Edge => open_browser_bundle(url, EDGE_BUNDLE_ID),
    Browser::Firefox => open_browser_bundle(url, FIREFOX_BUNDLE_ID),
    Browser::Other(application) => open_browser_application(url, application),
  }
}

fn open_default_browser(url: &str) -> anyhow::Result<()> {
  if webbrowser::open(url).is_ok() {
    return Ok(());
  }

  open::that_detached(url).map_err(Into::into)
}

fn open_browser_bundle(url: &str, bundle_id: &str) -> anyhow::Result<()> {
  info!("Launching browser bundle: {}", bundle_id);
  let status = std::process::Command::new("/usr/bin/open")
    .args(["-b", bundle_id, url])
    .status()?;

  if !status.success() {
    anyhow::bail!("Failed to launch browser bundle {bundle_id}: {status}");
  }

  Ok(())
}

fn open_browser_application(url: &str, application: &str) -> anyhow::Result<()> {
  info!("Launching browser application: {}", application);
  open::with_detached(url, application).map_err(Into::into)
}
