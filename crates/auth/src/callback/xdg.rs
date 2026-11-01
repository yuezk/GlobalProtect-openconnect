use std::{
  ffi::OsStr,
  fs,
  io::{ErrorKind, Write},
  path::{Path, PathBuf},
  process::{Command, Output},
};

use anyhow::{Context, bail};
use log::warn;

const MIME_TYPE: &str = "x-scheme-handler/globalprotectcallback";
const HANDLER: &str = "gpgui.desktop";

pub fn ensure_auth_callback_handler() -> anyhow::Result<()> {
  ensure_lxqt_mime_tool()?;

  let current_handler = query_callback_handler(MIME_TYPE)?;
  if !should_register_handler(&current_handler) {
    return Ok(());
  }

  let output = Command::new("xdg-mime")
    .args(["default", HANDLER, MIME_TYPE])
    .output()
    .context("xdg-mime is required for external-browser authentication")?;
  if !output.status.success() {
    log_command_failure("register callback handler", &output);
    bail!("Failed to register the globalprotectcallback URL handler");
  }
  // xdg-mime writes mimeapps.list, but a user desktop override takes precedence.
  if let Some(desktop) = std::env::var_os("XDG_CURRENT_DESKTOP") {
    update_desktop_override(&config_home()?, &desktop)?;
  }
  if query_callback_handler(MIME_TYPE)? != HANDLER {
    bail!("The globalprotectcallback URL handler registration did not take effect");
  }

  Ok(())
}

fn config_home() -> anyhow::Result<PathBuf> {
  if let Some(path) = std::env::var_os("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .filter(|path| path.is_absolute())
  {
    return Ok(path);
  }
  let home = std::env::var_os("HOME")
    .map(PathBuf::from)
    .filter(|path| path.is_absolute())
    .context("Cannot locate the user configuration directory")?;
  Ok(home.join(".config"))
}

fn update_desktop_override(config_home: &Path, current_desktop: &OsStr) -> anyhow::Result<()> {
  let Some(current_desktop) = current_desktop.to_str() else {
    return Ok(());
  };
  for desktop in current_desktop.split(':').filter(|name| !name.is_empty()) {
    let filename = format!("{}-mimeapps.list", desktop.to_ascii_lowercase());
    if Path::new(&filename).file_name() != Some(OsStr::new(&filename)) {
      continue;
    }
    let path = config_home.join(filename);
    let contents = match fs::read_to_string(&path) {
      Ok(contents) => contents,
      Err(error) if error.kind() == ErrorKind::NotFound => continue,
      Err(error) => return Err(error).with_context(|| format!("Cannot read {}", path.display())),
    };
    let Some(updated) = replace_callback_default(&contents) else {
      continue;
    };
    if updated != contents {
      // Follow user-managed symlinks and replace atomically to preserve the rest of the file.
      let target = fs::canonicalize(&path)?;
      let mut file = tempfile::NamedTempFile::new_in(target.parent().context("Missing configuration directory")?)?;
      file.as_file().set_permissions(fs::metadata(&target)?.permissions())?;
      file.write_all(updated.as_bytes())?;
      file
        .persist(&target)
        .with_context(|| format!("Cannot update {}", path.display()))?;
    }
    // Only the first desktop override with a callback default can take precedence.
    break;
  }
  Ok(())
}

fn replace_callback_default(contents: &str) -> Option<String> {
  let mut in_defaults = false;
  let mut found = false;
  let mut updated = String::with_capacity(contents.len());
  for line in contents.split_inclusive('\n') {
    let trimmed = line.trim();
    if trimmed.starts_with('[') {
      in_defaults = trimmed == "[Default Applications]";
    }
    if in_defaults
      && let Some((key, _)) = line.split_once('=')
      && key.trim() == MIME_TYPE
    {
      let ending = if line.ends_with("\r\n") {
        "\r\n"
      } else if line.ends_with('\n') {
        "\n"
      } else {
        ""
      };
      updated.push_str(&format!("{key}={HANDLER};{ending}"));
      found = true;
    } else {
      updated.push_str(line);
    }
  }
  found.then_some(updated)
}

fn query_callback_handler(mime_type: &str) -> anyhow::Result<String> {
  let output = Command::new("xdg-mime")
    .args(["query", "default", mime_type])
    .output()
    .context("xdg-mime is required for external-browser authentication")?;
  if !output.status.success() {
    log_command_failure("query callback handler", &output);
    bail!("Failed to query the globalprotectcallback URL handler");
  }
  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn ensure_lxqt_mime_tool() -> anyhow::Result<()> {
  if !is_lxqt_desktop(std::env::var_os("XDG_CURRENT_DESKTOP").as_deref()) {
    return Ok(());
  }

  let result = Command::new("qtxdg-mat").args(["defapp", "--help"]).output();
  match result {
    Ok(output) if output.status.success() => Ok(()),
    Ok(output) => {
      let stderr = String::from_utf8_lossy(&output.stderr);
      warn!(
        "qtxdg-mat readiness check failed with {}: {}",
        output.status,
        stderr.trim()
      );
      bail!("Install qtxdg-tools for browser SSO.")
    }
    Err(err) => {
      warn!("Failed to run qtxdg-mat readiness check: {err}");
      bail!("Install qtxdg-tools for browser SSO.")
    }
  }
}

fn is_lxqt_desktop(current_desktop: Option<&OsStr>) -> bool {
  current_desktop
    .and_then(OsStr::to_str)
    .is_some_and(|desktop| desktop.split(':').any(|name| name.eq_ignore_ascii_case("lxqt")))
}

fn log_command_failure(action: &str, output: &Output) {
  let stderr = String::from_utf8_lossy(&output.stderr);
  warn!("xdg-mime failed to {action} with {}: {}", output.status, stderr.trim());
}

fn should_register_handler(current_handler: &str) -> bool {
  current_handler != HANDLER
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{fs, os::unix::fs::PermissionsExt};
  use tempfile::tempdir;

  #[test]
  fn preserves_other_sections_and_line_endings() {
    let contents = "# Comment\r\n[Added Associations]\r\nx-scheme-handler/globalprotectcallback=other.desktop;\r\n[Default Applications]\r\ntext/plain=editor.desktop;\r\n x-scheme-handler/globalprotectcallback =old.desktop;backup.desktop;";
    assert_eq!(
      replace_callback_default(contents).unwrap(),
      "# Comment\r\n[Added Associations]\r\nx-scheme-handler/globalprotectcallback=other.desktop;\r\n[Default Applications]\r\ntext/plain=editor.desktop;\r\n x-scheme-handler/globalprotectcallback =gpgui.desktop;",
    );
    assert!(
      replace_callback_default("[Added Associations]\nx-scheme-handler/globalprotectcallback=old.desktop;\n").is_none()
    );
  }

  #[test]
  fn updates_only_first_active_desktop_override() {
    let directory = tempdir().unwrap();
    let config = directory.path();
    let original = "[Default Applications]\nx-scheme-handler/globalprotectcallback=old.desktop;\n";
    fs::write(
      config.join("other-mimeapps.list"),
      "[Default Applications]\ntext/plain=editor.desktop;\n",
    )
    .unwrap();
    fs::write(config.join("niri-mimeapps.list"), original).unwrap();
    fs::write(config.join("fallback-mimeapps.list"), original).unwrap();
    update_desktop_override(config, OsStr::new("Other:Niri:Fallback")).unwrap();
    assert_eq!(
      fs::read_to_string(config.join("niri-mimeapps.list")).unwrap(),
      "[Default Applications]\nx-scheme-handler/globalprotectcallback=gpgui.desktop;\n"
    );
    assert_eq!(
      fs::read_to_string(config.join("fallback-mimeapps.list")).unwrap(),
      original
    );
    assert_eq!(
      fs::read_to_string(config.join("other-mimeapps.list")).unwrap(),
      "[Default Applications]\ntext/plain=editor.desktop;\n"
    );
  }

  #[test]
  fn leaves_sessions_without_user_overrides_unchanged() {
    let directory = tempdir().unwrap();
    update_desktop_override(directory.path(), OsStr::new("Niri")).unwrap();
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
  }

  #[test]
  fn preserves_symlink_and_file_permissions() {
    let directory = tempdir().unwrap();
    let target = directory.path().join("managed.list");
    let link = directory.path().join("niri-mimeapps.list");
    fs::write(
      &target,
      "[Default Applications]\nx-scheme-handler/globalprotectcallback=old.desktop;\n",
    )
    .unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    update_desktop_override(directory.path(), OsStr::new("Niri")).unwrap();
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o640);
    assert!(fs::read_to_string(&target).unwrap().contains("=gpgui.desktop;"));
  }

  #[test]
  fn registers_callback_despite_desktop_override() {
    if std::env::var_os("GP_TEST_CALLBACK_REGISTRATION").is_some() {
      ensure_auth_callback_handler().unwrap();
      return;
    }

    let directory = tempdir().unwrap();
    let config = directory.path().join("config");
    fs::create_dir(&config).unwrap();
    fs::write(
      config.join("niri-mimeapps.list"),
      "# Keep this comment\n[Default Applications]\ntext/plain=editor.desktop;\nx-scheme-handler/globalprotectcallback=old.desktop;\n[Added Associations]\ntext/html=browser.desktop;\n",
    ).unwrap();
    let script = directory.path().join("xdg-mime");
    // Model xdg-mime's generic writer and desktop-specific lookup precedence.
    fs::write(
      &script,
      r#"#!/bin/sh
if [ "$1" = default ]; then
  printf '[Default Applications]\n%s=%s;\n' "$3" "$2" > "$XDG_CONFIG_HOME/mimeapps.list"
else
  for desktop in $(printf '%s' "$XDG_CURRENT_DESKTOP" | tr ':' ' ' | tr '[:upper:]' '[:lower:]') ''; do
    prefix=${desktop:+$desktop-}
    file="$XDG_CONFIG_HOME/${prefix}mimeapps.list"
    [ -f "$file" ] || continue
    handler=$(sed -n 's|^x-scheme-handler/globalprotectcallback=\([^;]*\).*|\1|p' "$file")
    if [ -n "$handler" ]; then printf '%s\n' "$handler"; exit 0; fi
  done
fi
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(
      std::iter::once(directory.path().to_path_buf()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let output = Command::new(std::env::current_exe().unwrap())
      .args([
        "--exact",
        "callback::xdg::tests::registers_callback_despite_desktop_override",
        "--nocapture",
      ])
      .env("GP_TEST_CALLBACK_REGISTRATION", "1")
      .env("XDG_CURRENT_DESKTOP", "Niri:Other")
      .env("XDG_CONFIG_HOME", &config)
      .env("PATH", path)
      .output()
      .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
      fs::read_to_string(config.join("niri-mimeapps.list")).unwrap(),
      "# Keep this comment\n[Default Applications]\ntext/plain=editor.desktop;\nx-scheme-handler/globalprotectcallback=gpgui.desktop;\n[Added Associations]\ntext/html=browser.desktop;\n",
    );
  }

  #[test]
  fn replaces_any_other_callback_handler() {
    assert!(!should_register_handler("gpgui.desktop"));
    assert!(should_register_handler(""));
    assert!(should_register_handler("gpauth.desktop"));
    assert!(should_register_handler("other-vpn.desktop"));
  }

  #[test]
  fn detects_lxqt_desktop_session() {
    assert!(is_lxqt_desktop(Some(OsStr::new("LXQt"))));
    assert!(is_lxqt_desktop(Some(OsStr::new("LXQt:KDE"))));
    assert!(is_lxqt_desktop(Some(OsStr::new("lxqt"))));
    assert!(!is_lxqt_desktop(Some(OsStr::new("LXDE"))));
    assert!(!is_lxqt_desktop(None));
  }
}
