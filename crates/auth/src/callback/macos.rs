use std::{ffi::OsStr, os::raw::c_void, path::PathBuf, thread, time::Duration};

use anyhow::{Context, bail};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSBundle, NSString, NSURL};

const CALLBACK_SCHEME: &str = "globalprotectcallback";
const CALLBACK_URL: &str = "globalprotectcallback:";
const HANDLER_BUNDLE_ID: &str = "com.yuezk.gpauth";
const LOGIN_APP_NAME: &str = "GP Connect Login.app";

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
  fn LSRegisterURL(url: *const c_void, update: u8) -> i32;
}

pub(super) fn ensure_auth_callback_handler() -> anyhow::Result<()> {
  let workspace = NSWorkspace::sharedWorkspace();
  let app_paths = callback_app_paths();
  let selected_handler = current_handler(&workspace);
  match selected_handler.as_ref() {
    Some((handler, path))
      if handler == HANDLER_BUNDLE_ID && app_paths.iter().any(|candidate| same_path(candidate, path)) =>
    {
      return Ok(());
    }
    Some((handler, _)) if handler == HANDLER_BUNDLE_ID => {}
    None => {}
    Some((handler, _)) => bail!("The globalprotectcallback URL scheme is owned by {handler}"),
  }

  let app_path = app_paths
    .first()
    .context("GP Connect Login.app is required for external-browser authentication; install GP Connect")?;
  let app_url = file_url(app_path);
  register_app(&app_url)?;
  workspace.setDefaultApplicationAtURL_toOpenURLsWithScheme_completionHandler(
    &app_url,
    &NSString::from_str(CALLBACK_SCHEME),
    None,
  );

  for _ in 0..50 {
    if current_handler(&workspace)
      .is_some_and(|(handler, path)| handler == HANDLER_BUNDLE_ID && same_path(app_path, &path))
    {
      return Ok(());
    }
    thread::sleep(Duration::from_millis(100));
  }

  bail!("The globalprotectcallback URL handler registration did not take effect")
}

fn register_app(app_url: &NSURL) -> anyhow::Result<()> {
  // A nested application is not necessarily known to LaunchServices until it
  // has been launched. Register it explicitly before assigning its URL scheme.
  let status = unsafe { LSRegisterURL(app_url as *const NSURL as *const c_void, 1) };
  if status != 0 {
    bail!("Failed to register GP Connect Login.app with LaunchServices (OSStatus {status})");
  }
  Ok(())
}

fn current_handler(workspace: &NSWorkspace) -> Option<(String, PathBuf)> {
  let callback_url = NSURL::URLWithString(&NSString::from_str(CALLBACK_URL))?;
  let app_url = workspace.URLForApplicationToOpenURL(&callback_url)?;
  let identifier = bundle_identifier(&app_url)?;
  let path = app_url.path().map(|path| PathBuf::from(path.to_string()))?;
  Some((identifier, path))
}

fn callback_app_paths() -> Vec<PathBuf> {
  let mut candidates = Vec::new();

  if let Ok(executable) = std::env::current_exe() {
    for path in executable.ancestors() {
      if path.file_name() == Some(OsStr::new("Helpers")) {
        candidates.push(path.join(LOGIN_APP_NAME));
      }
      if path.extension() == Some(OsStr::new("app")) {
        candidates.push(path.to_path_buf());
      }
    }
  }

  if let Some(user_dir) = std::env::var_os("HOME").filter(|path| !path.is_empty()) {
    let applications = PathBuf::from(user_dir).join("Applications");
    candidates.push(
      applications
        .join("GP Connect.app/Contents/Helpers")
        .join(LOGIN_APP_NAME),
    );
  }

  candidates.push(PathBuf::from("/Applications/GP Connect.app/Contents/Helpers").join(LOGIN_APP_NAME));

  candidates
    .into_iter()
    .filter(|path| path.is_dir() && bundle_identifier(&file_url(path)).as_deref() == Some(HANDLER_BUNDLE_ID))
    .collect()
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
  match (left.canonicalize(), right.canonicalize()) {
    (Ok(left), Ok(right)) => left == right,
    _ => left == right,
  }
}

fn file_url(path: &std::path::Path) -> objc2::rc::Retained<NSURL> {
  NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn bundle_identifier(app_url: &NSURL) -> Option<String> {
  NSBundle::bundleWithURL(app_url)?
    .bundleIdentifier()
    .map(|identifier| identifier.to_string())
}
