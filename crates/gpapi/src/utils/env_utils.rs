use std::collections::HashMap;
use std::env;
use std::io::Write;
use std::path::Path;

use log::info;
use tempfile::NamedTempFile;

pub fn persist_env_vars(extra: Option<HashMap<String, String>>) -> anyhow::Result<NamedTempFile> {
  let mut env_file = NamedTempFile::new()?;
  let content = env::vars()
    .map(|(key, value)| format!("{}={}", key, value))
    .chain(
      extra
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| format!("{}={}", key, value)),
    )
    .collect::<Vec<String>>()
    .join("\n");

  writeln!(env_file, "{}", content)?;

  Ok(env_file)
}

pub fn load_env_vars<T: AsRef<Path>>(env_file: T) -> anyhow::Result<HashMap<String, String>> {
  let content = std::fs::read_to_string(env_file)?;
  let mut env_vars: HashMap<String, String> = HashMap::new();

  for line in content.lines() {
    if let Some((key, value)) = line.split_once('=') {
      env_vars.insert(key.to_string(), value.to_string());
    }
  }

  Ok(env_vars)
}

pub fn patch_gui_runtime_env(hidpi: bool) {
  // This is to avoid blank screen on some systems
  unsafe { std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1") };

  if hidpi {
    info!("Setting GDK_SCALE=2 and GDK_DPI_SCALE=0.5");
    unsafe {
      std::env::set_var("GDK_SCALE", "2");
      std::env::set_var("GDK_DPI_SCALE", "0.5");
    };
  }
}
