use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{auth::SamlAuthData, utils::base64::decode_to_string};

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PasswordCredential {
  username: String,
  password: String,
}

impl PasswordCredential {
  pub fn new(username: &str, password: &str) -> Self {
    Self {
      username: username.to_string(),
      password: password.to_string(),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn password(&self) -> &str {
    &self.password
  }
}

impl From<&CachedCredential> for PasswordCredential {
  fn from(value: &CachedCredential) -> Self {
    Self::new(value.username(), value.password().unwrap_or_default())
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PreloginCookieCredential {
  username: String,
  prelogin_cookie: String,
}

impl PreloginCookieCredential {
  pub fn new(username: &str, prelogin_cookie: &str) -> Self {
    Self {
      username: username.to_string(),
      prelogin_cookie: prelogin_cookie.to_string(),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn prelogin_cookie(&self) -> &str {
    &self.prelogin_cookie
  }
}

impl TryFrom<SamlAuthData> for PreloginCookieCredential {
  type Error = anyhow::Error;

  fn try_from(value: SamlAuthData) -> Result<Self, Self::Error> {
    let username = value.username().to_string();
    let prelogin_cookie = value
      .prelogin_cookie()
      .ok_or_else(|| anyhow::anyhow!("Missing prelogin cookie"))?
      .to_string();

    Ok(Self::new(&username, &prelogin_cookie))
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthCookieCredential {
  username: String,
  user_auth_cookie: String,
  prelogon_user_auth_cookie: String,
}

impl AuthCookieCredential {
  pub fn new(username: &str, user_auth_cookie: &str, prelogon_user_auth_cookie: &str) -> Self {
    Self {
      username: username.to_string(),
      user_auth_cookie: user_auth_cookie.to_string(),
      prelogon_user_auth_cookie: prelogon_user_auth_cookie.to_string(),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn user_auth_cookie(&self) -> &str {
    &self.user_auth_cookie
  }

  pub fn prelogon_user_auth_cookie(&self) -> &str {
    &self.prelogon_user_auth_cookie
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CachedCredential {
  username: String,
  password: Option<String>,
  auth_cookie: AuthCookieCredential,
}

impl CachedCredential {
  pub fn new(
    username: String,
    password: Option<String>,
    auth_cookie: AuthCookieCredential,
  ) -> Self {
    Self {
      username,
      password,
      auth_cookie,
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn password(&self) -> Option<&str> {
    self.password.as_deref()
  }

  pub fn auth_cookie(&self) -> &AuthCookieCredential {
    &self.auth_cookie
  }

  pub fn set_auth_cookie(&mut self, auth_cookie: AuthCookieCredential) {
    self.auth_cookie = auth_cookie;
  }
}

impl From<PasswordCredential> for CachedCredential {
  fn from(value: PasswordCredential) -> Self {
    Self::new(
      value.username().to_owned(),
      Some(value.password().to_owned()),
      AuthCookieCredential::new("", "", ""),
    )
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Credential {
  Password(PasswordCredential),
  PreloginCookie(PreloginCookieCredential),
  AuthCookie(AuthCookieCredential),
  CachedCredential(CachedCredential),
}

impl Credential {
  /// Create a credential from a globalprotectcallback:<base64 encoded string>
  pub fn parse_gpcallback(auth_data: &str) -> anyhow::Result<Self> {
    // Remove the surrounding quotes
    let auth_data = auth_data.trim_matches('"');
    let auth_data = auth_data.trim_start_matches("globalprotectcallback:");
    let auth_data = decode_to_string(auth_data)?;
    let auth_data = SamlAuthData::parse_html(&auth_data)?;

    Self::try_from(auth_data)
  }

  pub fn username(&self) -> &str {
    match self {
      Credential::Password(cred) => cred.username(),
      Credential::PreloginCookie(cred) => cred.username(),
      Credential::AuthCookie(cred) => cred.username(),
      Credential::CachedCredential(cred) => cred.username(),
    }
  }

  pub fn to_params(&self) -> HashMap<&str, &str> {
    let mut params = HashMap::new();
    params.insert("user", self.username());

    let (passwd, prelogin_cookie, portal_userauthcookie, portal_prelogonuserauthcookie) = match self
    {
      Credential::Password(cred) => (Some(cred.password()), None, None, None),
      Credential::PreloginCookie(cred) => (None, Some(cred.prelogin_cookie()), None, None),
      Credential::AuthCookie(cred) => (
        None,
        None,
        Some(cred.user_auth_cookie()),
        Some(cred.prelogon_user_auth_cookie()),
      ),
      Credential::CachedCredential(cred) => (
        cred.password(),
        None,
        Some(cred.auth_cookie.user_auth_cookie()),
        Some(cred.auth_cookie.prelogon_user_auth_cookie()),
      ),
    };

    params.insert("passwd", passwd.unwrap_or_default());
    params.insert("prelogin-cookie", prelogin_cookie.unwrap_or_default());
    params.insert(
      "portal-userauthcookie",
      portal_userauthcookie.unwrap_or_default(),
    );
    params.insert(
      "portal-prelogonuserauthcookie",
      portal_prelogonuserauthcookie.unwrap_or_default(),
    );

    params
  }
}

impl TryFrom<SamlAuthData> for Credential {
  type Error = anyhow::Error;

  fn try_from(value: SamlAuthData) -> Result<Self, Self::Error> {
    let prelogin_cookie = PreloginCookieCredential::try_from(value)?;

    Ok(Self::PreloginCookie(prelogin_cookie))
  }
}

impl From<PasswordCredential> for Credential {
  fn from(value: PasswordCredential) -> Self {
    Self::Password(value)
  }
}

impl From<&AuthCookieCredential> for Credential {
  fn from(value: &AuthCookieCredential) -> Self {
    Self::AuthCookie(value.clone())
  }
}

impl From<&CachedCredential> for Credential {
  fn from(value: &CachedCredential) -> Self {
    Self::CachedCredential(value.clone())
  }
}
