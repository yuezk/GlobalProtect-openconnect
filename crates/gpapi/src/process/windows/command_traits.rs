use tokio::process::Command;

/// Windows authentication runs in the interactive user's GUI process.
pub trait CommandExt {
  fn into_non_root(self) -> anyhow::Result<Command>;
}

impl CommandExt for Command {
  fn into_non_root(self) -> anyhow::Result<Command> {
    Ok(self)
  }
}
