use std::{borrow::Cow, env::temp_dir, fs, io::Write, os::unix::fs::PermissionsExt};

use anyhow::bail;
use log::{info, warn};

pub struct BrowserAuthenticator<'a> {
  auth_request: &'a str,
  browser: Option<&'a str>,
}

impl BrowserAuthenticator<'_> {
  pub fn new(auth_request: &str) -> BrowserAuthenticator {
    BrowserAuthenticator {
      auth_request,
      browser: None,
    }
  }

  pub fn new_with_browser<'a>(auth_request: &'a str, browser: &'a str) -> BrowserAuthenticator<'a> {
    let browser = browser.trim();
    BrowserAuthenticator {
      auth_request,
      browser: if browser.is_empty() || browser == "default" {
        None
      } else {
        Some(browser)
      },
    }
  }

  pub fn authenticate(&self) -> anyhow::Result<()> {
    let path = if self.auth_request.starts_with("http") {
      Cow::Borrowed(self.auth_request)
    } else {
      let html_file = temp_dir().join("gpauth.html");

      // Remove the file and error if permission denied
      if let Err(err) = fs::remove_file(&html_file) {
        if err.kind() != std::io::ErrorKind::NotFound {
          warn!("Failed to remove the temporary file: {}", err);
          bail!("Please remove the file manually: {:?}", html_file);
        }
      }

      let mut file = fs::File::create(&html_file)?;

      file.set_permissions(fs::Permissions::from_mode(0o600))?;
      file.write_all(self.auth_request.as_bytes())?;

      Cow::Owned(html_file.to_string_lossy().to_string())
    };

    if let Some(browser) = self.browser {
      let app = find_browser_path(browser);

      info!("Launching browser: {}", app);
      open::with_detached(path.as_ref(), app)?;
    } else {
      info!("Launching the default browser...");
      webbrowser::open(path.as_ref())?;
    }

    Ok(())
  }
}

fn find_browser_path(browser: &str) -> String {
  if browser == "chrome" {
    which::which("google-chrome-stable")
      .or_else(|_| which::which("google-chrome"))
      .or_else(|_| which::which("chromium"))
      .map(|path| path.to_string_lossy().to_string())
      .unwrap_or_else(|_| browser.to_string())
  } else {
    browser.into()
  }
}
