use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Serialize, Deserialize, Clone, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionWarning {
  pub prior_secs: u32,
  pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, Type, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
  pub lifetime_secs: Option<u32>,
  pub user_expires: Option<u32>,
  pub lifetime_warning: Option<SessionWarning>,
  pub inactivity_warning: Option<SessionWarning>,
  pub admin_logout_message: Option<String>,
  pub allow_extend_session: bool,
}
