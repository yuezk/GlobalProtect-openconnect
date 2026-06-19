use crate::{credential::Credential, gp_params::GpParams};

const EMPTY_PORTAL_COOKIE: &str = "empty";

fn portal_cookie_param(value: Option<&str>) -> String {
  match value {
    Some(value) if !value.is_empty() => value.to_string(),
    _ => EMPTY_PORTAL_COOKIE.to_string(),
  }
}

pub(crate) fn build(cred: &Credential, gp_params: &GpParams) -> Vec<(String, String)> {
  let mut params = Vec::new();
  params.push(("user".to_string(), cred.username().to_string()));

  let (passwd, prelogin_cookie, portal_userauthcookie, portal_prelogonuserauthcookie, token) = match cred {
    Credential::Password(cred) => (Some(cred.password()), None, None, None, None),
    Credential::Prelogin(cred) => (
      Some(gp_params.os_profile().saml_password()),
      cred.prelogin_cookie(),
      cred.portal_userauthcookie(),
      None,
      cred.token(),
    ),
    Credential::AuthCookie(cred) => (
      Some(
        cred
          .password()
          .unwrap_or_else(|| gp_params.os_profile().saml_password()),
      ),
      None,
      Some(cred.user_auth_cookie()),
      Some(cred.prelogon_user_auth_cookie()),
      None,
    ),
    Credential::Cached(cred) => (
      cred.password(),
      None,
      cred.auth_cookie().map(|cred| cred.user_auth_cookie()),
      cred.auth_cookie().map(|cred| cred.prelogon_user_auth_cookie()),
      None,
    ),
  };

  params.push(("passwd".to_string(), passwd.unwrap_or_default().to_string()));
  params.push((
    "prelogin-cookie".to_string(),
    prelogin_cookie.unwrap_or_default().to_string(),
  ));
  params.push((
    "portal-userauthcookie".to_string(),
    portal_cookie_param(portal_userauthcookie),
  ));
  params.push((
    "portal-prelogonuserauthcookie".to_string(),
    portal_cookie_param(portal_prelogonuserauthcookie),
  ));

  params.push(("token".to_string(), token.unwrap_or_default().to_string()));

  params
}
