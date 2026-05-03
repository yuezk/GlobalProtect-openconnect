use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, TimeZone};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::gp_params::ClientOs;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct SessionWarning {
  pub prior_secs: u32,
  pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SessionRequestArgs {
  cookie: String,
  user_agent: Option<String>,
  os: Option<ClientOs>,
  os_version: Option<String>,
  client_version: Option<String>,
  certificate: Option<String>,
  sslkey: Option<String>,
  key_password: Option<String>,
  disable_ipv6: bool,
}

impl SessionRequestArgs {
  pub fn new(cookie: String) -> Self {
    Self {
      cookie,
      user_agent: None,
      os: None,
      os_version: None,
      client_version: None,
      certificate: None,
      sslkey: None,
      key_password: None,
      disable_ipv6: false,
    }
  }

  pub fn with_user_agent<T: Into<Option<String>>>(mut self, user_agent: T) -> Self {
    self.user_agent = user_agent.into();
    self
  }

  pub fn with_os<T: Into<Option<ClientOs>>>(mut self, os: T) -> Self {
    self.os = os.into();
    self
  }

  pub fn with_os_version<T: Into<Option<String>>>(mut self, os_version: T) -> Self {
    self.os_version = os_version.into();
    self
  }

  pub fn with_client_version<T: Into<Option<String>>>(mut self, client_version: T) -> Self {
    self.client_version = client_version.into();
    self
  }

  pub fn with_certificate<T: Into<Option<String>>>(mut self, certificate: T) -> Self {
    self.certificate = certificate.into();
    self
  }

  pub fn with_sslkey<T: Into<Option<String>>>(mut self, sslkey: T) -> Self {
    self.sslkey = sslkey.into();
    self
  }

  pub fn with_key_password<T: Into<Option<String>>>(mut self, key_password: T) -> Self {
    self.key_password = key_password.into();
    self
  }

  pub fn with_disable_ipv6(mut self, disable_ipv6: bool) -> Self {
    self.disable_ipv6 = disable_ipv6;
    self
  }

  pub fn cookie(&self) -> &str {
    &self.cookie
  }

  pub fn user_agent(&self) -> Option<String> {
    self.user_agent.clone()
  }

  pub fn os(&self) -> Option<ClientOs> {
    self.os.clone()
  }

  pub fn os_version(&self) -> Option<String> {
    self.os_version.clone()
  }

  pub fn client_version(&self) -> Option<String> {
    self.client_version.clone()
  }

  pub fn certificate(&self) -> Option<String> {
    self.certificate.clone()
  }

  pub fn sslkey(&self) -> Option<String> {
    self.sslkey.clone()
  }

  pub fn key_password(&self) -> Option<String> {
    self.key_password.clone()
  }

  pub fn disable_ipv6(&self) -> bool {
    self.disable_ipv6
  }
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

impl SessionInfo {
  pub fn with_computed_human_times(mut self) -> Self {
    self.expires_in_human = build_human_readable_expiry(self.user_expires, self.lifetime_secs);
    self
  }

  pub fn from_vpn_session_fields(
    lifetime_secs: Option<u32>,
    user_expires: Option<u32>,
    lifetime_warning: Option<SessionWarning>,
    allow_extend_session: bool,
  ) -> Self {
    Self {
      lifetime_secs,
      user_expires,
      expires_in_human: None,
      lifetime_warning,
      inactivity_warning: None,
      admin_logout_message: None,
      allow_extend_session,
    }
    .with_computed_human_times()
  }

  pub fn rescheduled_after_extension(&self) -> Option<Self> {
    let lifetime_secs = self.lifetime_secs?;
    let mut session_info = self.clone();
    session_info.user_expires = Some(unix_timestamp().saturating_add(lifetime_secs));
    Some(session_info.with_computed_human_times())
  }

  pub fn log_summary(&self) -> String {
    let lifetime_secs = self
      .lifetime_secs
      .map(format_secs_with_duration)
      .unwrap_or_else(|| "none".to_string());
    let user_expires = self
      .user_expires
      .map(format_epoch_with_local_time)
      .unwrap_or_else(|| "none".to_string());
    let lifetime_warning_prior = self
      .lifetime_warning
      .as_ref()
      .map(|warning| format_secs_with_duration(warning.prior_secs))
      .unwrap_or_else(|| "none".to_string());

    format!(
      "lifetime_secs={lifetime_secs}, user_expires={user_expires}, lifetime_warning_prior={lifetime_warning_prior}, allow_extend_session={}",
      self.allow_extend_session
    )
  }
}

pub fn format_duration_secs(total_secs: u32) -> String {
  humantime::format_duration(Duration::from_secs(total_secs as u64)).to_string()
}

fn format_secs_with_duration(secs: u32) -> String {
  format!("{secs} ({})", format_duration_secs(secs))
}

fn format_epoch_with_local_time(epoch: u32) -> String {
  let local_time = Local
    .timestamp_opt(epoch as i64, 0)
    .single()
    .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
    .unwrap_or_else(|| "invalid time".to_string());
  let now = unix_timestamp();
  let relative = if epoch >= now {
    format!("in {}", format_duration_secs(epoch.saturating_sub(now)))
  } else {
    format!("{} ago", format_duration_secs(now.saturating_sub(epoch)))
  };

  format!("{epoch} ({local_time}, {relative})")
}

fn build_human_readable_expiry(user_expires: Option<u32>, lifetime_secs: Option<u32>) -> Option<String> {
  if let Some(user_expires) = user_expires {
    let now = unix_timestamp();
    return Some(format_duration_secs(user_expires.saturating_sub(now)));
  }

  lifetime_secs.map(format_duration_secs)
}

fn unix_timestamp() -> u32 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_secs() as u32
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn formats_duration_secs() {
    assert_eq!(format_duration_secs(43_200), "12h");
    assert_eq!(format_duration_secs(1_830), "30m 30s");
  }

  #[test]
  fn logs_session_info_with_human_readable_times() {
    let info = SessionInfo {
      lifetime_secs: Some(43_200),
      user_expires: Some(unix_timestamp().saturating_add(43_200)),
      lifetime_warning: Some(SessionWarning {
        prior_secs: 1_800,
        message: "Session expires soon".to_string(),
      }),
      allow_extend_session: true,
      ..Default::default()
    };

    let summary = info.log_summary();

    assert!(summary.contains("lifetime_secs=43200 (12h)"));
    assert!(summary.contains("user_expires="));
    assert!(summary.contains(", in "));
    assert!(summary.contains("lifetime_warning_prior=1800 (30m)"));
    assert!(summary.contains("allow_extend_session=true"));
  }

  #[test]
  fn computes_expires_in_human_from_lifetime_or_epoch() {
    let from_lifetime = SessionInfo {
      lifetime_secs: Some(43_200),
      ..Default::default()
    }
    .with_computed_human_times();
    assert_eq!(from_lifetime.expires_in_human.as_deref(), Some("12h"));

    let from_epoch = SessionInfo {
      user_expires: Some(unix_timestamp().saturating_add(1_830)),
      ..Default::default()
    }
    .with_computed_human_times();
    assert!(
      from_epoch
        .expires_in_human
        .as_deref()
        .is_some_and(|value| value.starts_with("30m"))
    );
  }

  #[test]
  fn reschedules_after_extension_from_lifetime() {
    let now = unix_timestamp();
    let info = SessionInfo {
      lifetime_secs: Some(7_200),
      user_expires: Some(now.saturating_add(1_830)),
      ..Default::default()
    };

    let next = info.rescheduled_after_extension().unwrap();

    assert!(
      next
        .user_expires
        .is_some_and(|expires| expires >= now.saturating_add(7_200))
    );
    assert!(next.expires_in_human.is_some());
  }

  #[test]
  fn reschedule_after_extension_requires_lifetime() {
    assert!(SessionInfo::default().rescheduled_after_extension().is_none());
  }
}
