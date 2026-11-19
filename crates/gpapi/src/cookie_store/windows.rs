use std::{
  ffi::OsString,
  io,
  os::windows::ffi::OsStringExt,
  path::{Path, PathBuf},
  ptr, slice,
};
use windows_sys::Win32::{
  Foundation::LocalFree,
  Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData},
  UI::Shell::{CSIDL_LOCAL_APPDATA, SHGetFolderPathW},
};
use zeroize::Zeroize;

pub(super) fn default_path() -> Option<PathBuf> {
  let mut path = [0; 260];
  let result = unsafe {
    SHGetFolderPathW(
      ptr::null_mut(),
      CSIDL_LOCAL_APPDATA as i32,
      ptr::null_mut(),
      0,
      path.as_mut_ptr(),
    )
  };
  let length = path.iter().position(|&unit| unit == 0).filter(|&length| length > 0)?;
  (result >= 0).then(|| PathBuf::from(OsString::from_wide(&path[..length])).join(r"GP Connect\gpclient\cookie.bin"))
}

pub(super) fn encode(mut bytes: Vec<u8>) -> io::Result<Vec<u8>> {
  let result = transform(&mut bytes, true);
  bytes.zeroize();
  result
}

pub(super) fn decode(mut bytes: Vec<u8>) -> io::Result<Vec<u8>> {
  transform(&mut bytes, false)
}

// DPAPI encrypts for the current Windows user, including at custom paths.
fn transform(bytes: &mut [u8], protect: bool) -> io::Result<Vec<u8>> {
  let input = CRYPT_INTEGER_BLOB {
    cbData: u32::try_from(bytes.len()).map_err(io::Error::other)?,
    pbData: bytes.as_mut_ptr(),
  };
  let mut output = CRYPT_INTEGER_BLOB::default();
  let success = unsafe {
    if protect {
      CryptProtectData(
        &input,
        ptr::null(),
        ptr::null(),
        ptr::null(),
        ptr::null(),
        CRYPTPROTECT_UI_FORBIDDEN,
        &mut output,
      )
    } else {
      CryptUnprotectData(
        &input,
        ptr::null_mut(),
        ptr::null(),
        ptr::null(),
        ptr::null(),
        CRYPTPROTECT_UI_FORBIDDEN,
        &mut output,
      )
    }
  };
  if success == 0 {
    return Err(io::Error::last_os_error());
  }
  if output.pbData.is_null() {
    return Err(io::Error::other("Windows returned no cookie cache data"));
  }
  let buffer = unsafe { slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
  let result = buffer.to_vec();
  buffer.zeroize();
  unsafe { LocalFree(output.pbData.cast()) };
  Ok(result)
}

pub(super) fn prepare_directory(_: &Path) -> io::Result<()> {
  Ok(())
}

pub(super) use prepare_directory as prepare_file;
