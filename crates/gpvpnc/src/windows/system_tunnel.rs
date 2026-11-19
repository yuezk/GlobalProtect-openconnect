use super::NetworkConfigurator;
use std::{
  ffi::OsString,
  fs::{File, OpenOptions},
  io,
  os::windows::{ffi::OsStringExt, fs::OpenOptionsExt},
  path::PathBuf,
  ptr,
  sync::Arc,
};
use windows_sys::Win32::{
  Foundation::ERROR_SHARING_VIOLATION,
  UI::Shell::{CSIDL_COMMON_APPDATA, SHGetFolderPathW},
};

const RECOVERY_KEY: &str = r"SOFTWARE\GP Connect\NetworkRecovery";

/// Exclusive ownership of the installed Windows system tunnel, including recovery.
#[derive(Clone)]
pub struct SystemTunnel {
  lock: Arc<File>,
}

impl SystemTunnel {
  pub fn try_acquire() -> io::Result<Option<Self>> {
    let path = machine_data()?.join(r"GP Connect\Private\system-tunnel.lock");
    let file = match OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .share_mode(0)
      .open(path)
    {
      Ok(file) => file,
      Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => return Ok(None),
      Err(error) => return Err(error),
    };
    let tunnel = Self { lock: Arc::new(file) };
    super::recover_routes(RECOVERY_KEY)?;
    super::dns::clear_stale_policies(&tunnel)?;
    Ok(Some(tunnel))
  }

  pub fn acquire() -> io::Result<Self> {
    Self::try_acquire()?.ok_or_else(|| {
      io::Error::new(
        io::ErrorKind::WouldBlock,
        "Another GP Connect system tunnel or installer is active. Disconnect the tunnel or wait for setup to finish.",
      )
    })
  }

  pub fn configurator(&self) -> io::Result<NetworkConfigurator> {
    NetworkConfigurator::with_owner(RECOVERY_KEY, Some(self.clone()))
  }

  pub(super) fn duplicate_lock(&self) -> io::Result<File> {
    self.lock.try_clone()
  }
}

fn machine_data() -> io::Result<PathBuf> {
  let mut path = [0; 260];
  let result = unsafe {
    SHGetFolderPathW(
      ptr::null_mut(),
      CSIDL_COMMON_APPDATA as i32,
      ptr::null_mut(),
      0,
      path.as_mut_ptr(),
    )
  };
  if result < 0 {
    return Err(io::Error::other(format!(
      "Windows ProgramData lookup failed ({result:#x})"
    )));
  }
  let length = path
    .iter()
    .position(|&c| c == 0)
    .filter(|&n| n > 0)
    .ok_or_else(|| io::Error::other("Windows ProgramData path is unavailable"))?;
  Ok(PathBuf::from(OsString::from_wide(&path[..length])))
}
