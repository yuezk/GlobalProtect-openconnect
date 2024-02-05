use std::env;

use anyhow::bail;
use uzers::User;

pub fn get_user_by_name(username: &str) -> anyhow::Result<User> {
  uzers::get_user_by_name(username).ok_or_else(|| anyhow::anyhow!("User ({}) not found", username))
}

pub fn get_non_root_user() -> anyhow::Result<User> {
  let current_user = whoami::username();

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
  let current_user = whoami::username();
  get_user_by_name(&current_user)
}

fn get_real_user() -> anyhow::Result<User> {
  // Read the UID from SUDO_UID or PKEXEC_UID environment variable if available.
  let uid = match env::var("SUDO_UID") {
    Ok(uid) => uid.parse::<u32>()?,
    _ => env::var("PKEXEC_UID")?.parse::<u32>()?,
  };

  uzers::get_user_by_uid(uid).ok_or_else(|| anyhow::anyhow!("User not found"))
}
