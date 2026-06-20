pub(crate) mod credential;
pub mod gateway_login;
pub mod gateway_prelogin;
pub mod portal_getconfig;
pub mod portal_prelogin;

/// Ordered request parameters split into body (form data) and query string.
///
/// Uses `Vec<(String, String)>` instead of `HashMap` to preserve parameter
/// ordering, as some servers may be sensitive to order.
#[derive(Debug, Clone, Default)]
pub struct RequestParams {
  pub body: Vec<(String, String)>,
  pub query: Vec<(String, String)>,
}
