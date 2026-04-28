use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionWarning {
  pub prior_secs: u32,
  pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
  pub lifetime_secs: Option<u32>,
  pub user_expires: Option<u32>,
  pub expires_in_human: Option<String>,
  pub lifetime_warning: Option<SessionWarning>,
  pub inactivity_warning: Option<SessionWarning>,
  pub admin_logout_message: Option<String>,
  pub allow_extend_session: bool,
}
