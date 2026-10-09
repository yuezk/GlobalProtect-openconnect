use anyhow::bail;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::auth::{AuthenticationCancelled, SamlAuthData, SamlAuthResult};

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
  portal_userauthcookie: Option<String>,
  token: Option<String>,
}

impl PreloginCredential {
  pub fn new(
    username: &str,
    prelogin_cookie: Option<&str>,
    portal_userauthcookie: Option<&str>,
    token: Option<&str>,
  ) -> Self {
    Self {
      username: username.to_string(),
      prelogin_cookie: prelogin_cookie.map(|s| s.to_string()),
      portal_userauthcookie: portal_userauthcookie.map(|s| s.to_string()),
      token: token.map(|s| s.to_string()),
    }
  }

  pub fn username(&self) -> &str {
    &self.username
  }

  pub fn prelogin_cookie(&self) -> Option<&str> {
    self.prelogin_cookie.as_deref()
  }

  pub fn portal_userauthcookie(&self) -> Option<&str> {
    self.portal_userauthcookie.as_deref()
  }

  pub fn token(&self) -> Option<&str> {
    self.token.as_deref()
  }
}

impl From<SamlAuthData> for PreloginCredential {
  fn from(value: SamlAuthData) -> Self {
    let username = value.username().to_string();
    let prelogin_cookie = value.prelogin_cookie();
    let portal_userauthcookie = value.portal_userauthcookie();
    let token = value.token();

    Self::new(&username, prelogin_cookie, portal_userauthcookie, token)
  }
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct AuthCookieCredential {
  username: String,
  user_auth_cookie: String,
  prelogon_user_auth_cookie: String,
  #[serde(skip)]
  password: Option<String>,
}

impl AuthCookieCredential {
  pub fn new(username: &str, user_auth_cookie: &str, prelogon_user_auth_cookie: &str) -> Self {
    Self {
      username: username.to_string(),
      user_auth_cookie: user_auth_cookie.to_string(),
      prelogon_user_auth_cookie: prelogon_user_auth_cookie.to_string(),
      password: None,
    }
  }

  pub fn with_password(mut self, password: impl Into<String>) -> Self {
    let password = password.into();
    self.password = (!password.trim().is_empty()).then_some(password);
    self
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

  pub fn password(&self) -> Option<&str> {
    self.password.as_deref()
  }

  pub fn can_authenticate_gateway(&self) -> bool {
    is_gateway_auth_cookie(&self.user_auth_cookie) || is_gateway_auth_cookie(&self.prelogon_user_auth_cookie)
  }
}

fn is_gateway_auth_cookie(value: &str) -> bool {
  let value = value.trim();
  !value.is_empty() && value != "empty"
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CachedCredential {
  username: String,
  password: Option<String>,
  auth_cookie: Option<AuthCookieCredential>,
}

impl CachedCredential {
  pub fn new(username: String, password: Option<String>, auth_cookie: Option<AuthCookieCredential>) -> Self {
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

  pub fn auth_cookie(&self) -> Option<&AuthCookieCredential> {
    self.auth_cookie.as_ref()
  }

  pub fn can_authenticate_gateway(&self) -> bool {
    self
      .auth_cookie
      .as_ref()
      .is_some_and(AuthCookieCredential::can_authenticate_gateway)
  }

  pub fn set_auth_cookie(&mut self, auth_cookie: AuthCookieCredential) {
    self.auth_cookie = Some(auth_cookie);
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
    Self::new(value.username().to_owned(), Some(value.password().to_owned()), None)
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

  pub fn password(&self) -> Option<&str> {
    match self {
      Credential::Password(cred) => Some(cred.password()),
      Credential::Cached(cred) => cred.password(),
      Credential::Prelogin(_) | Credential::AuthCookie(_) => None,
    }
  }

  pub fn can_authenticate_gateway(&self) -> bool {
    match self {
      Credential::AuthCookie(cred) => cred.can_authenticate_gateway(),
      Credential::Cached(cred) => cred.can_authenticate_gateway(),
      Credential::Password(_) | Credential::Prelogin(_) => true,
    }
  }
}

impl From<SamlAuthData> for Credential {
  fn from(value: SamlAuthData) -> Self {
    let cred = PreloginCredential::from(value);

    Self::Prelogin(cred)
  }
}

impl TryFrom<SamlAuthResult> for Credential {
  type Error = anyhow::Error;

  fn try_from(value: SamlAuthResult) -> anyhow::Result<Self> {
    match value {
      SamlAuthResult::Success(auth_data) => Ok(Self::from(auth_data)),
      SamlAuthResult::Cancelled => bail!(AuthenticationCancelled),
      SamlAuthResult::Failure(err) => bail!(err),
    }
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn prelogin_credential_keeps_saml_cookies() {
    let auth_data = SamlAuthData::new(
      Some("alice@example.com".to_string()),
      Some("prelogin-cookie".to_string()),
      Some("portal-userauthcookie".to_string()),
    )
    .unwrap();

    let cred = Credential::from(auth_data);
    let Credential::Prelogin(prelogin) = cred else {
      panic!("expected prelogin credential");
    };

    assert_eq!(prelogin.username(), "alice@example.com");
    assert_eq!(prelogin.prelogin_cookie(), Some("prelogin-cookie"));
    assert_eq!(prelogin.portal_userauthcookie(), Some("portal-userauthcookie"));
    assert_eq!(prelogin.token(), None);
  }

  #[test]
  fn prelogin_credential_keeps_cas_token_without_cookies() {
    let auth_data =
      SamlAuthData::from_gpcallback("globalprotectcallback:cas-as=1&un=alice@example.com&token=cas-token").unwrap();

    let cred = Credential::from(auth_data);
    let Credential::Prelogin(prelogin) = cred else {
      panic!("expected prelogin credential");
    };

    assert_eq!(prelogin.username(), "alice@example.com");
    assert_eq!(prelogin.prelogin_cookie(), None);
    assert_eq!(prelogin.portal_userauthcookie(), None);
    assert_eq!(prelogin.token(), Some("cas-token"));
  }

  #[test]
  fn auth_cookie_runtime_password_is_not_serialized() {
    let value = serde_json::to_value(
      AuthCookieCredential::new("alice", "user-cookie", "prelogon-cookie").with_password("secret"),
    )
    .unwrap();

    assert_eq!(value.get("password"), None);
  }

  #[test]
  fn auth_cookie_with_empty_sentinels_cannot_authenticate_gateway() {
    let cred = AuthCookieCredential::new("alice", "empty", "empty");

    assert!(!cred.can_authenticate_gateway());
  }

  #[test]
  fn auth_cookie_with_blank_values_cannot_authenticate_gateway() {
    let cred = AuthCookieCredential::new("alice", "", " ");

    assert!(!cred.can_authenticate_gateway());
  }

  #[test]
  fn auth_cookie_with_real_cookie_can_authenticate_gateway() {
    let cred = AuthCookieCredential::new("alice", "empty", "portal-prelogon-cookie");

    assert!(cred.can_authenticate_gateway());
  }

  #[test]
  fn cached_credential_without_real_auth_cookie_cannot_authenticate_gateway() {
    let cred = CachedCredential::new(
      "alice".to_string(),
      None,
      Some(AuthCookieCredential::new("alice", "empty", "empty")),
    );

    assert!(!cred.can_authenticate_gateway());
    assert!(!Credential::from(&cred).can_authenticate_gateway());
  }
}
