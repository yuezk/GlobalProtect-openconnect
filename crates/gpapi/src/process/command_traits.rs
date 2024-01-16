use anyhow::bail;
use std::{env, ffi::OsStr};
use tokio::process::Command;
use users::{os::unix::UserExt, User};

pub trait CommandExt {
  fn new_pkexec<S: AsRef<OsStr>>(program: S) -> Command;
  fn into_non_root(self) -> anyhow::Result<Command>;
}

impl CommandExt for Command {
  fn new_pkexec<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new("pkexec");
    cmd
      .arg("--disable-internal-agent")
      .arg("--user")
      .arg("root")
      .arg(program);

    cmd
  }

  fn into_non_root(mut self) -> anyhow::Result<Command> {
    let user =
      get_non_root_user().map_err(|_| anyhow::anyhow!("{:?} cannot be run as root", self))?;

    self
      .env("HOME", user.home_dir())
      .env("USER", user.name())
      .env("LOGNAME", user.name())
      .env("USERNAME", user.name())
      .uid(user.uid())
      .gid(user.primary_group_id());

    Ok(self)
  }
}

fn get_non_root_user() -> anyhow::Result<User> {
  let current_user = whoami::username();

  let user = if current_user == "root" {
    get_real_user()?
  } else {
    users::get_user_by_name(&current_user)
      .ok_or_else(|| anyhow::anyhow!("User ({}) not found", current_user))?
  };

  if user.uid() == 0 {
    bail!("Non-root user not found")
  }

  Ok(user)
}

fn get_real_user() -> anyhow::Result<User> {
  // Read the UID from SUDO_UID or PKEXEC_UID environment variable if available.
  let uid = match env::var("SUDO_UID") {
    Ok(uid) => uid.parse::<u32>()?,
    _ => env::var("PKEXEC_UID")?.parse::<u32>()?,
  };

  users::get_user_by_uid(uid).ok_or_else(|| anyhow::anyhow!("User not found"))
}
