use std::path::Path;

use tokio::process::Command;

pub(super) fn apply(command: &mut Command, user_id: u32, home_dir: &Path) {
  platform::apply(command, user_id, home_dir);
}

#[cfg(target_os = "linux")]
mod platform {
  use std::{collections::HashMap, env, fs, os::unix::fs::MetadataExt, path::Path};

  use tokio::process::Command;

  const DESKTOP_SESSION_ENV: &[&str] = &[
    "BROWSER",
    "DBUS_SESSION_BUS_ADDRESS",
    "DESKTOP_SESSION",
    "DISPLAY",
    "MOZ_ENABLE_WAYLAND",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "XDG_CONFIG_HOME",
    "XDG_CURRENT_DESKTOP",
    "XDG_DATA_DIRS",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
  ];

  pub(super) fn apply(command: &mut Command, user_id: u32, home_dir: &Path) {
    let session_env = collect(user_id);

    for key in DESKTOP_SESSION_ENV {
      if let Some(value) = env_value(key).or_else(|| session_env.get(*key).cloned()) {
        command.env(key, value);
      }
    }

    let runtime_dir = env_value("XDG_RUNTIME_DIR")
      .or_else(|| session_env.get("XDG_RUNTIME_DIR").cloned())
      .unwrap_or_else(|| format!("/run/user/{user_id}"));
    command.env("XDG_RUNTIME_DIR", &runtime_dir);

    if env_value("DBUS_SESSION_BUS_ADDRESS")
      .or_else(|| session_env.get("DBUS_SESSION_BUS_ADDRESS").cloned())
      .is_none()
    {
      command.env("DBUS_SESSION_BUS_ADDRESS", format!("unix:path={runtime_dir}/bus"));
    }

    if env_value("XDG_DATA_HOME")
      .or_else(|| session_env.get("XDG_DATA_HOME").cloned())
      .is_none()
    {
      command.env("XDG_DATA_HOME", home_dir.join(".local/share"));
    }

    if env_value("XDG_CONFIG_HOME")
      .or_else(|| session_env.get("XDG_CONFIG_HOME").cloned())
      .is_none()
    {
      command.env("XDG_CONFIG_HOME", home_dir.join(".config"));
    }

    if env_value("XDG_DATA_DIRS")
      .or_else(|| session_env.get("XDG_DATA_DIRS").cloned())
      .is_none()
    {
      command.env("XDG_DATA_DIRS", default_xdg_data_dirs());
    }
  }

  fn collect(user_id: u32) -> HashMap<String, String> {
    let mut best = HashMap::new();
    let mut best_score = 0;

    let Ok(entries) = fs::read_dir("/proc") else {
      return best;
    };

    for entry in entries.flatten() {
      let path = entry.path();
      if !is_process_dir(&path) || !owned_by_user(&path, user_id) {
        continue;
      }

      let env = read_environ(&path);
      let score = score_env(&env);
      if score > best_score {
        best_score = score;
        best = env;
      }
    }

    best
  }

  fn is_process_dir(path: &Path) -> bool {
    path
      .file_name()
      .and_then(|name| name.to_str())
      .is_some_and(|name| name.bytes().all(|byte| byte.is_ascii_digit()))
  }

  fn owned_by_user(path: &Path, user_id: u32) -> bool {
    path.metadata().is_ok_and(|metadata| metadata.uid() == user_id)
  }

  fn read_environ(process_dir: &Path) -> HashMap<String, String> {
    let path = process_dir.join("environ");
    let Ok(bytes) = fs::read(path) else {
      return HashMap::new();
    };

    parse_environ(&bytes)
  }

  fn parse_environ(bytes: &[u8]) -> HashMap<String, String> {
    bytes
      .split(|byte| *byte == b'\0')
      .filter_map(|entry| {
        let index = entry.iter().position(|byte| *byte == b'=')?;
        let (key, value) = (&entry[..index], &entry[index + 1..]);
        let key = std::str::from_utf8(key).ok()?;
        if !DESKTOP_SESSION_ENV.contains(&key) {
          return None;
        }
        let value = String::from_utf8_lossy(value).to_string();
        Some((key.to_string(), value))
      })
      .filter_map(|(key, value)| non_empty(Some(value)).map(|value| (key, value)))
      .collect()
  }

  fn score_env(env: &HashMap<String, String>) -> usize {
    let mut score = 0;

    if env.contains_key("DBUS_SESSION_BUS_ADDRESS") {
      score += 4;
    }
    if env.contains_key("XDG_RUNTIME_DIR") {
      score += 3;
    }
    if env.contains_key("WAYLAND_DISPLAY") || env.contains_key("DISPLAY") {
      score += 3;
    }
    if env.contains_key("XDG_CURRENT_DESKTOP") {
      score += 2;
    }
    if env.contains_key("XDG_DATA_DIRS") {
      score += 1;
    }

    score
  }

  fn env_value(key: &str) -> Option<String> {
    non_empty(env::var(key).ok())
  }

  fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|value| !value.trim().is_empty())
  }

  fn default_xdg_data_dirs() -> &'static str {
    "/usr/local/share:/usr/share:/var/lib/snapd/desktop"
  }

  #[cfg(test)]
  mod tests {
    use super::*;

    #[test]
    fn parse_environ_keeps_only_desktop_session_values() {
      let env = parse_environ(b"DISPLAY=:0\0PASSWORD=secret\0XDG_DATA_DIRS=/usr/share\0EMPTY= \0");

      assert_eq!(env.get("DISPLAY").map(String::as_str), Some(":0"));
      assert_eq!(env.get("XDG_DATA_DIRS").map(String::as_str), Some("/usr/share"));
      assert!(!env.contains_key("PASSWORD"));
      assert!(!env.contains_key("EMPTY"));
    }

    #[test]
    fn score_prefers_graphical_session_env() {
      let plain = HashMap::from([("XDG_DATA_DIRS".to_string(), "/usr/share".to_string())]);
      let graphical = HashMap::from([
        (
          "DBUS_SESSION_BUS_ADDRESS".to_string(),
          "unix:path=/run/user/1000/bus".to_string(),
        ),
        ("XDG_RUNTIME_DIR".to_string(), "/run/user/1000".to_string()),
        ("WAYLAND_DISPLAY".to_string(), "wayland-0".to_string()),
      ]);

      assert!(score_env(&graphical) > score_env(&plain));
    }

    #[test]
    fn non_empty_ignores_missing_or_blank_values() {
      assert_eq!(non_empty(None), None);
      assert_eq!(non_empty(Some(" ".to_string())), None);
      assert_eq!(non_empty(Some("value".to_string())), Some("value".to_string()));
    }
  }
}

#[cfg(not(target_os = "linux"))]
mod platform {
  use std::path::Path;

  use tokio::process::Command;

  pub(super) fn apply(_command: &mut Command, _user_id: u32, _home_dir: &Path) {}
}
