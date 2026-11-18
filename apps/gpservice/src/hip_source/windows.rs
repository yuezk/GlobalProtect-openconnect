pub(super) fn user_script(_path: &str, _desktop_uid: Option<u32>) -> anyhow::Result<openconnect::HipSource> {
  anyhow::bail!("Custom HIP scripts are unavailable on Windows")
}
