use std::ffi::{c_char, c_int};

use pkcs8::SecretDocument;
use zeroize::Zeroizing;

use crate::utils::request::RequestIdentityError;

unsafe extern "C" {
  fn gp_key_import(
    input: *const u8,
    length: usize,
    pem: c_int,
    password: *const c_char,
    output: *mut *mut u8,
    output_length: *mut usize,
  ) -> c_int;
  fn gp_key_free(data: *mut u8, length: usize);
}

struct ExportedKey {
  data: *mut u8,
  length: usize,
}

impl Drop for ExportedKey {
  fn drop(&mut self) {
    // The shim owns the allocation and wipes it before releasing it.
    unsafe { gp_key_free(self.data, self.length) };
  }
}

pub(in crate::utils) fn decrypt(bytes: &[u8], pem: bool, passphrase: Option<&str>) -> anyhow::Result<SecretDocument> {
  let passphrase = passphrase.ok_or(RequestIdentityError::NoPassphrase("PEM"))?;
  anyhow::ensure!(
    !passphrase.as_bytes().contains(&0),
    "Private key passphrases containing NUL are not supported"
  );
  let mut password = Zeroizing::new(passphrase.as_bytes().to_vec());
  password.push(0);
  let mut exported = ExportedKey {
    data: std::ptr::null_mut(),
    length: 0,
  };
  // All input slices remain live for the call; the output is owned by the guard.
  let result = unsafe {
    gp_key_import(
      bytes.as_ptr(),
      bytes.len(),
      i32::from(pem),
      password.as_ptr().cast(),
      &mut exported.data,
      &mut exported.length,
    )
  };
  match result {
    0 => {
      anyhow::ensure!(!exported.data.is_null() && exported.length > 0, "Invalid private key");
      // A successful shim call returns an initialized allocation of this length.
      let bytes = unsafe { std::slice::from_raw_parts(exported.data, exported.length) };
      SecretDocument::try_from(bytes).map_err(|_| RequestIdentityError::InvalidKey.into())
    }
    1 => Err(RequestIdentityError::DecryptError("PEM").into()),
    2 => Err(RequestIdentityError::UnsupportedEncryption.into()),
    _ => Err(RequestIdentityError::InvalidKey.into()),
  }
}
