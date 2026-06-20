use std::{
  env,
  path::{Path, PathBuf},
};

use crate::constants::{GP_AUTH_BINARY, GP_CLIENT_BINARY, GP_GUI_BINARY, GP_GUI_HELPER_BINARY, GP_SERVICE_BINARY};

pub fn gpclient() -> PathBuf {
  resolve("GP_CLIENT_BINARY", "gpclient", GP_CLIENT_BINARY)
}

pub fn gpservice() -> PathBuf {
  resolve("GP_SERVICE_BINARY", "gpservice", GP_SERVICE_BINARY)
}

pub fn gpauth() -> PathBuf {
  resolve("GP_AUTH_BINARY", "gpauth", GP_AUTH_BINARY)
}

pub fn gpgui() -> PathBuf {
  resolve("GP_GUI_BINARY", "gpgui", GP_GUI_BINARY)
}

pub fn gpgui_helper() -> PathBuf {
  resolve("GP_GUI_HELPER_BINARY", "gpgui-helper", GP_GUI_HELPER_BINARY)
}

fn resolve(env_key: &str, binary_name: &str, default_path: &str) -> PathBuf {
  env::var_os(env_key)
    .filter(|value| !value.is_empty())
    .map(PathBuf::from)
    .or_else(|| sibling_binary(binary_name))
    .unwrap_or_else(|| PathBuf::from(default_path))
}

fn sibling_binary(binary_name: &str) -> Option<PathBuf> {
  let current_exe = env::current_exe().ok()?;
  let bin_dir = current_exe.parent()?;
  let binary = bin_dir.join(binary_name);

  is_file(&binary).then_some(binary)
}

fn is_file(path: &Path) -> bool {
  path.is_file()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_path_is_used_when_no_sibling_exists() {
    let path = resolve("GP_TEST_BINARY", "missing-gp-test-binary", "/usr/bin/gp-test");

    assert_eq!(path, PathBuf::from("/usr/bin/gp-test"));
  }
}
