pub(crate) fn desktop_uid() -> Option<u32> {
  match gpapi::process::users::get_non_root_user() {
    Ok(user) => Some(user.uid()),
    Err(err) => {
      log::warn!("No verified desktop user is available for custom HIP scripts: {err}");
      None
    }
  }
}
