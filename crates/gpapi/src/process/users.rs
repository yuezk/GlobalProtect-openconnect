use std::env;

use anyhow::bail;
use uzers::User;

#[derive(Debug, PartialEq, Eq)]
enum RealUserLookup {
  Uid(u32),
  Name(String),
}

pub fn get_user_by_name(username: &str) -> anyhow::Result<User> {
  uzers::get_user_by_name(username).ok_or_else(|| anyhow::anyhow!("User ({}) not found", username))
}

pub fn get_non_root_user() -> anyhow::Result<User> {
  let current_user = current_username()?;

  let user = if current_user == "root" {
    get_real_user()?
  } else {
    get_user_by_name(&current_user)?
  };

  if user.uid() == 0 {
    bail!("Non-root user not found")
  }

  Ok(user)
}

pub fn get_current_user() -> anyhow::Result<User> {
  let current_user = current_username()?;
  get_user_by_name(&current_user)
}

fn current_username() -> anyhow::Result<String> {
  whoami::username().map_err(|err| anyhow::anyhow!("Failed to resolve current user: {err}"))
}

fn get_real_user() -> anyhow::Result<User> {
  match real_user_lookup(|key| env::var(key).ok())? {
    RealUserLookup::Uid(uid) => uzers::get_user_by_uid(uid).ok_or_else(|| anyhow::anyhow!("User not found")),
    RealUserLookup::Name(name) => get_user_by_name(&name),
  }
}

fn real_user_lookup(env_var: impl Fn(&str) -> Option<String>) -> anyhow::Result<RealUserLookup> {
  if let Some(uid) = env_var("SUDO_UID") {
    return Ok(RealUserLookup::Uid(uid.parse::<u32>()?));
  }

  if let Some(uid) = env_var("PKEXEC_UID") {
    return Ok(RealUserLookup::Uid(uid.parse::<u32>()?));
  }

  if let Some(user) = env_var("DOAS_USER").filter(|user| !user.trim().is_empty()) {
    return Ok(RealUserLookup::Name(user));
  }

  bail!("User not found")
}

#[cfg(test)]
mod tests {
  use super::*;

  fn lookup(vars: &[(&str, &str)]) -> anyhow::Result<RealUserLookup> {
    real_user_lookup(|key| {
      vars
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value.to_string())
    })
  }

  #[test]
  fn real_user_lookup_uses_sudo_uid() {
    assert_eq!(lookup(&[("SUDO_UID", "1000")]).unwrap(), RealUserLookup::Uid(1000));
  }

  #[test]
  fn real_user_lookup_uses_pkexec_uid() {
    assert_eq!(lookup(&[("PKEXEC_UID", "1001")]).unwrap(), RealUserLookup::Uid(1001));
  }

  #[test]
  fn real_user_lookup_uses_doas_user() {
    assert_eq!(
      lookup(&[("DOAS_USER", "alice")]).unwrap(),
      RealUserLookup::Name("alice".to_string())
    );
  }

  #[test]
  fn real_user_lookup_prefers_uid_over_doas_user() {
    assert_eq!(
      lookup(&[("SUDO_UID", "1000"), ("PKEXEC_UID", "1001"), ("DOAS_USER", "alice")]).unwrap(),
      RealUserLookup::Uid(1000)
    );
    assert_eq!(
      lookup(&[("PKEXEC_UID", "1001"), ("DOAS_USER", "alice")]).unwrap(),
      RealUserLookup::Uid(1001)
    );
  }

  #[test]
  fn real_user_lookup_ignores_blank_doas_user() {
    assert!(lookup(&[("DOAS_USER", " ")]).is_err());
  }

  #[test]
  fn real_user_lookup_errors_without_root_origin_env() {
    assert!(lookup(&[]).is_err());
  }
}
