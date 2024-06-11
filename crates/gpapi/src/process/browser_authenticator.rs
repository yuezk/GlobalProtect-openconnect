use std::{env::temp_dir, fs, io::Write, os::unix::fs::PermissionsExt};

use anyhow::bail;
use log::warn;

pub struct BrowserAuthenticator<'a> {
  auth_request: &'a str,
}

impl BrowserAuthenticator<'_> {
  pub fn new(auth_request: &str) -> BrowserAuthenticator {
    BrowserAuthenticator { auth_request }
  }

  pub fn authenticate(&self) -> anyhow::Result<()> {
    if self.auth_request.starts_with("http") {
      open::that_detached(self.auth_request)?;
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

      open::that_detached(html_file)?;
    }

    Ok(())
  }
}
