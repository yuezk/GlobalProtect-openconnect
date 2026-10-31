//! Desktop GUI selection shared by launchers, installers and settings.
use serde::{Deserialize, Serialize};
use specta::Type;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum GuiVariant {
  #[default]
  Tauri,
  Nsh,
}

impl GuiVariant {
  pub fn binary_name(self) -> &'static str {
    match self {
      Self::Tauri => "gpgui",
      Self::Nsh => "gpgui-nsh",
    }
  }

  pub fn update_target(self) -> PathBuf {
    let target = common::binary_paths::gpgui_update_target();
    match self {
      Self::Tauri => target,
      Self::Nsh => target.with_file_name(self.binary_name()),
    }
  }

  pub fn primary(self) -> PathBuf {
    match self {
      Self::Tauri => common::binary_paths::gpgui(),
      Self::Nsh => common::binary_paths::gpgui().with_file_name(self.binary_name()),
    }
  }
}

#[cfg(target_os = "linux")]
fn preference_path() -> anyhow::Result<PathBuf> {
  let dirs = directories::ProjectDirs::from("com.yuezk", "GlobalProtect-openconnect", "gpclient")
    .ok_or_else(|| anyhow::anyhow!("Failed to locate GUI preference directory"))?;
  Ok(dirs.config_dir().join("gui"))
}

pub fn preferred() -> anyhow::Result<GuiVariant> {
  #[cfg(target_os = "linux")]
  {
    match std::fs::read_to_string(preference_path()?) {
      Ok(value) => match value.trim() {
        "gpgui" => Ok(GuiVariant::Tauri),
        "gpgui-nsh" => Ok(GuiVariant::Nsh),
        _ => anyhow::bail!("Invalid GUI preference"),
      },
      Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(GuiVariant::Tauri),
      Err(error) => Err(error.into()),
    }
  }
  #[cfg(not(target_os = "linux"))]
  Ok(GuiVariant::Tauri)
}

#[cfg(target_os = "linux")]
pub fn save_preference(variant: GuiVariant) -> anyhow::Result<()> {
  use std::io::Write;
  let path = preference_path()?;
  let directory = path.parent().unwrap();
  std::fs::create_dir_all(directory)?;
  let mut file = tempfile::NamedTempFile::new_in(directory)?;
  writeln!(file, "{}", variant.binary_name())?;
  file.persist(path)?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn nsh_selection_keeps_both_launch_and_upgrade_paths_on_nsh() {
    let variant = GuiVariant::Nsh;
    assert_eq!(variant.primary().file_name().unwrap(), "gpgui-nsh");
    assert_eq!(variant.update_target().file_name().unwrap(), "gpgui-nsh");
    assert_eq!(
      variant.update_target().parent(),
      GuiVariant::Tauri.update_target().parent()
    );
    assert_eq!(serde_json::to_string(&variant).unwrap(), "\"nsh\"");
  }
}
