use std::{env::temp_dir, io::Write};

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
      let mut file = std::fs::File::create(&html_file)?;

      file.write_all(self.auth_request.as_bytes())?;

      open::that_detached(html_file)?;
    }

    Ok(())
  }
}

impl Drop for BrowserAuthenticator<'_> {
  fn drop(&mut self) {
    // Cleanup the temporary file
    let html_file = temp_dir().join("gpauth.html");
    let _ = std::fs::remove_file(html_file);
  }
}
