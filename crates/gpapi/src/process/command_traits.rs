use std::ffi::OsStr;
use tokio::process::Command;
use uzers::os::unix::UserExt;

use super::users::get_non_root_user;

pub trait CommandExt {
  fn new_pkexec<S: AsRef<OsStr>>(program: S) -> Command;
  fn into_non_root(self) -> anyhow::Result<Command>;
}

impl CommandExt for Command {
  fn new_pkexec<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new("pkexec");
    cmd.arg("--user").arg("root").arg(program);

    cmd
  }

  fn into_non_root(mut self) -> anyhow::Result<Command> {
    let user = get_non_root_user().map_err(|_| anyhow::anyhow!("{:?} cannot be run as root", self))?;

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
