use std::process::Stdio;

use anyhow::bail;
use tokio::process::Command;

pub struct HipLauncher<'a> {
  program: &'a str,
  cookie: Option<&'a str>,
  client_ip: Option<&'a str>,
  md5: Option<&'a str>,
  client_os: Option<&'a str>,
  client_version: Option<&'a str>,
}

impl<'a> HipLauncher<'a> {
  pub fn new(program: &'a str) -> Self {
    Self {
      program,
      cookie: None,
      client_ip: None,
      md5: None,
      client_os: None,
      client_version: None,
    }
  }

  pub fn cookie(mut self, cookie: &'a str) -> Self {
    self.cookie = Some(cookie);
    self
  }

  pub fn client_ip(mut self, client_ip: &'a str) -> Self {
    self.client_ip = Some(client_ip);
    self
  }

  pub fn md5(mut self, md5: &'a str) -> Self {
    self.md5 = Some(md5);
    self
  }

  pub fn client_os(mut self, client_os: &'a str) -> Self {
    self.client_os = Some(client_os);
    self
  }

  pub fn client_version(mut self, client_version: Option<&'a str>) -> Self {
    self.client_version = client_version;
    self
  }

  pub async fn launch(&self) -> anyhow::Result<String> {
    let mut cmd = Command::new(self.program);

    if let Some(cookie) = self.cookie {
      cmd.arg("--cookie").arg(cookie);
    }

    if let Some(client_ip) = self.client_ip {
      cmd.arg("--client-ip").arg(client_ip);
    }

    if let Some(md5) = self.md5 {
      cmd.arg("--md5").arg(md5);
    }

    if let Some(client_os) = self.client_os {
      cmd.arg("--client-os").arg(client_os);
    }

    if let Some(client_version) = self.client_version {
      cmd.env("APP_VERSION", client_version);
    }

    let output = cmd
      .kill_on_drop(true)
      .stdout(Stdio::piped())
      .spawn()?
      .wait_with_output()
      .await?;

    if let Some(exit_status) = output.status.code() {
      if exit_status != 0 {
        bail!("HIP report generation failed with exit code {}", exit_status);
      }

      let report = String::from_utf8(output.stdout)?;

      Ok(report)
    } else {
      bail!("HIP report generation failed");
    }
  }
}
