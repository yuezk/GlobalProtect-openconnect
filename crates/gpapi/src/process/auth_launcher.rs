use std::process::Stdio;

use tokio::process::Command;

use crate::{auth::SamlAuthResult, credential::Credential, GP_AUTH_BINARY};

use super::command_traits::CommandExt;

pub struct SamlAuthLauncher<'a> {
  server: &'a str,
  user_agent: Option<&'a str>,
  saml_request: Option<&'a str>,
  hidpi: bool,
  fix_openssl: bool,
  clean: bool,
}

impl<'a> SamlAuthLauncher<'a> {
  pub fn new(server: &'a str) -> Self {
    Self {
      server,
      user_agent: None,
      saml_request: None,
      hidpi: false,
      fix_openssl: false,
      clean: false,
    }
  }

  pub fn user_agent(mut self, user_agent: &'a str) -> Self {
    self.user_agent = Some(user_agent);
    self
  }

  pub fn saml_request(mut self, saml_request: &'a str) -> Self {
    self.saml_request = Some(saml_request);
    self
  }

  pub fn hidpi(mut self, hidpi: bool) -> Self {
    self.hidpi = hidpi;
    self
  }

  pub fn fix_openssl(mut self, fix_openssl: bool) -> Self {
    self.fix_openssl = fix_openssl;
    self
  }

  pub fn clean(mut self, clean: bool) -> Self {
    self.clean = clean;
    self
  }

  /// Launch the authenticator binary as the current user or SUDO_USER if available.
  pub async fn launch(self) -> anyhow::Result<Credential> {
    let mut auth_cmd = Command::new(GP_AUTH_BINARY);
    auth_cmd.arg(self.server);

    if let Some(user_agent) = self.user_agent {
      auth_cmd.arg("--user-agent").arg(user_agent);
    }

    if let Some(saml_request) = self.saml_request {
      auth_cmd.arg("--saml-request").arg(saml_request);
    }

    if self.fix_openssl {
      auth_cmd.arg("--fix-openssl");
    }

    if self.hidpi {
      auth_cmd.arg("--hidpi");
    }

    if self.clean {
      auth_cmd.arg("--clean");
    }

    let mut non_root_cmd = auth_cmd.into_non_root()?;
    let output = non_root_cmd
      .kill_on_drop(true)
      .stdout(Stdio::piped())
      .spawn()?
      .wait_with_output()
      .await?;

    let auth_result: SamlAuthResult = serde_json::from_slice(&output.stdout)
      .map_err(|_| anyhow::anyhow!("Failed to parse auth data"))?;

    match auth_result {
      SamlAuthResult::Success(auth_data) => Credential::try_from(auth_data),
      SamlAuthResult::Failure(msg) => Err(anyhow::anyhow!(msg)),
    }
  }
}
