use std::sync::Arc;

use super::auth_messenger::AuthMessenger;

pub struct AuthRequest<'a>(&'a str);

impl<'a> AuthRequest<'a> {
  pub fn new(auth_request: &'a str) -> Self {
    Self(auth_request)
  }

  pub fn is_url(&self) -> bool {
    self.0.starts_with("http")
  }

  pub fn as_str(&self) -> &str {
    self.0
  }
}

pub struct AuthSettings<'a> {
  pub auth_request: AuthRequest<'a>,
  pub auth_messenger: Arc<AuthMessenger>,
  pub ignore_tls_errors: bool,
}
