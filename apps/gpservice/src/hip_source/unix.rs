use anyhow::{Context, ensure};
use openconnect::{HipScript, HipScriptEnvironment};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
pub(super) fn user_script(path: &str, desktop_uid: Option<u32>) -> anyhow::Result<openconnect::HipSource> {
  let uid = desktop_uid.context("HIP custom scripts require a verified desktop user")?;
  ensure!(uid != 0, "HIP custom scripts require a non-root desktop user");
  let path = PathBuf::from(path);
  ensure!(path.is_absolute(), "HIP custom script path must be absolute");
  let metadata = fs::symlink_metadata(&path).context("Failed to inspect the HIP script")?;
  ensure!(
    metadata.is_file() && !metadata.file_type().is_symlink() && metadata.permissions().mode() & 0o111 != 0,
    "HIP custom script must be an executable regular file"
  );
  let script =
    HipScript::new(path.to_string_lossy().into_owned(), Some(uid))?.with_environment(script_environment(uid)?)?;
  Ok(openconnect::HipSource::Script(script))
}
pub(crate) fn script_environment(uid: u32) -> anyhow::Result<HipScriptEnvironment> {
  let user =
    nix::unistd::User::from_uid(nix::unistd::Uid::from_raw(uid))?.context("HIP execution user is unavailable")?;
  let home = user.dir.to_string_lossy().into_owned();
  Ok(HipScriptEnvironment {
    variables: vec![
      (
        "PATH".into(),
        gpapi::process::collection::command_path(uid == 0)?
          .into_string()
          .map_err(|_| anyhow::anyhow!("HIP command path is not valid UTF-8"))?,
      ),
      ("LC_ALL".into(), "C".into()),
      ("HOME".into(), home.clone()),
      ("USER".into(), user.name.clone()),
      ("LOGNAME".into(), user.name),
    ],
    cwd: home,
  })
}
