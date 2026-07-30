pub(crate) fn trusted_csd_uid() -> anyhow::Result<Option<u32>> {
  let uid = gpapi::process::users::get_non_root_user()?.uid();
  if uid == 0 {
    anyhow::bail!("The HIP script user must not be root");
  }
  Ok(Some(uid))
}
