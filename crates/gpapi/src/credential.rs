use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::auth::SamlAuthData;

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
pub struct PreloginCredential {
  username: String,
  prelogin_cookie: Option<String>,
  token: Option<String>,
}

impl PreloginCredential {
  pub fn new(username: &str, prelogin_cookie: Option<&str>, token: Option<&str>) -> Self {
    Self {
      username: username.to_string(),
      prelogin_cookie: prelogin_cookie.map(|s| s.to_string()),
      token: token.map(|s| s.to_string()),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn prelogin_cookie(&self) -> Option<&str> {
    self.prelogin_cookie.as_deref()
  }

  pub fn token(&self) -> Option<&str> {
    self.token.as_deref()
  }
}

impl From<SamlAuthData> for PreloginCredential {
  fn from(value: SamlAuthData) -> Self {
    let username = value.username().to_string();
    let prelogin_cookie = value.prelogin_cookie();
    let token = value.token();

    Self::new(&username, prelogin_cookie, token)
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
  pub fn new(username: String, password: Option<String>, auth_cookie: AuthCookieCredential) -> Self {
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

  pub fn set_username(&mut self, username: String) {
    self.username = username;
  }

  pub fn set_password(&mut self, password: Option<String>) {
    self.password = password.map(|s| s.to_string());
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
  Prelogin(PreloginCredential),
  AuthCookie(AuthCookieCredential),
  Cached(CachedCredential),
}

impl Credential {
  /// Create a credential from a globalprotectcallback:<base64 encoded string>,
  /// or globalprotectcallback:cas-as=1&un=user@xyz.com&token=very_long_string
  pub fn from_gpcallback(auth_data: &str) -> anyhow::Result<Self> {
    let auth_data = SamlAuthData::from_gpcallback(auth_data)?;

    Ok(Self::from(auth_data))
  }

  pub fn username(&self) -> &str {
    match self {
      Credential::Password(cred) => cred.username(),
      Credential::Prelogin(cred) => cred.username(),
      Credential::AuthCookie(cred) => cred.username(),
      Credential::Cached(cred) => cred.username(),
    }
  }

  pub fn to_params(&self) -> HashMap<&str, &str> {
    let mut params = HashMap::new();
    params.insert("user", self.username());

    let (passwd, prelogin_cookie, portal_userauthcookie, portal_prelogonuserauthcookie, token) = match self {
      Credential::Password(cred) => (Some(cred.password()), None, None, None, None),
      Credential::Prelogin(cred) => (None, cred.prelogin_cookie(), None, None, cred.token()),
      Credential::AuthCookie(cred) => (
        None,
        None,
        Some(cred.user_auth_cookie()),
        Some(cred.prelogon_user_auth_cookie()),
        None,
      ),
      Credential::Cached(cred) => (
        cred.password(),
        None,
        Some(cred.auth_cookie.user_auth_cookie()),
        Some(cred.auth_cookie.prelogon_user_auth_cookie()),
        None,
      ),
    };

    params.insert("passwd", passwd.unwrap_or_default());
    params.insert("prelogin-cookie", prelogin_cookie.unwrap_or_default());
    params.insert("portal-userauthcookie", portal_userauthcookie.unwrap_or_default());
    params.insert(
      "portal-prelogonuserauthcookie",
      portal_prelogonuserauthcookie.unwrap_or_default(),
    );

    if let Some(token) = token {
      params.insert("token", token);
    }

    params
  }
}

impl From<SamlAuthData> for Credential {
  fn from(value: SamlAuthData) -> Self {
    let cred = PreloginCredential::from(value);

    Self::Prelogin(cred)
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
    Self::Cached(value.clone())
  }
}
