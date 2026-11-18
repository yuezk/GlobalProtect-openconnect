//! Resolve system collectors from Windows' system directory, never the working directory.
use super::command_runner::run_controlled;
use crate::collection::CollectionControl;
use std::{
  io,
  path::{Path, PathBuf},
  process::{Command, Output},
};

pub struct CollectorCommands<'a> {
  control: &'a dyn CollectionControl,
}
impl<'a> CollectorCommands<'a> {
  pub fn new(control: &'a dyn CollectionControl) -> Self {
    Self { control }
  }
  pub fn resolve(&self, name: &str) -> Option<PathBuf> {
    let system = std::env::var_os("SystemRoot")?;
    let path = PathBuf::from(system).join("System32").join(name).with_extension("exe");
    path.is_file().then_some(path)
  }
  pub fn run(&self, name: &str, args: &[&str]) -> io::Result<Option<Output>> {
    self.control.check()?;
    match self.resolve(name) {
      Some(path) => self.run_path(&path, args),
      None => Ok(None),
    }
  }
  pub fn run_executable(&self, path: &Path, args: &[&str]) -> io::Result<Option<Output>> {
    self.run_path(path, args)
  }
  pub fn run_path(&self, path: &Path, args: &[&str]) -> io::Result<Option<Output>> {
    self.control.check()?;
    let mut command = Command::new(path);
    command.args(args);
    match run_controlled(command, self.control, 256 * 1024) {
      Ok(output) => Ok(Some(output)),
      Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied) => Ok(None),
      Err(error) => Err(error),
    }
  }
}
