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
  fn gp_pkcs12_identity(
    input: *const u8,
    length: usize,
    password: *const c_char,
    certificate: *mut *mut u8,
    certificate_length: *mut usize,
    key: *mut *mut u8,
    key_length: *mut usize,
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

pub(in crate::utils) fn archive(
  bytes: &[u8],
  passphrase: Option<&str>,
) -> anyhow::Result<(Zeroizing<Vec<u8>>, SecretDocument)> {
  let mut password = Zeroizing::new(passphrase.unwrap_or_default().as_bytes().to_vec());
  anyhow::ensure!(
    !password.contains(&0),
    "PKCS#12 passphrases containing NUL are not supported"
  );
  password.push(0);
  let mut certificate = ExportedKey {
    data: std::ptr::null_mut(),
    length: 0,
  };
  let mut key = ExportedKey {
    data: std::ptr::null_mut(),
    length: 0,
  };
  let result = unsafe {
    gp_pkcs12_identity(
      bytes.as_ptr(),
      bytes.len(),
      password.as_ptr().cast(),
      &mut certificate.data,
      &mut certificate.length,
      &mut key.data,
      &mut key.length,
    )
  };
  match result {
    0 => {
      anyhow::ensure!(
        !certificate.data.is_null() && certificate.length > 0,
        "Invalid certificate file"
      );
      anyhow::ensure!(!key.data.is_null() && key.length > 0, "Invalid private key");
      let certificate =
        Zeroizing::new(unsafe { std::slice::from_raw_parts(certificate.data, certificate.length) }.to_vec());
      let key = SecretDocument::try_from(unsafe { std::slice::from_raw_parts(key.data, key.length) })
        .map_err(|_| RequestIdentityError::InvalidKey)?;
      Ok((certificate, key))
    }
    1 if passphrase.is_none() => Err(RequestIdentityError::NoPassphrase("PKCS#12").into()),
    1 => Err(RequestIdentityError::InvalidPkcs12(p12_keystore::error::Error::InvalidData).into()),
    2 => Err(RequestIdentityError::IncompletePkcs12.into()),
    3 => Err(RequestIdentityError::AmbiguousPkcs12.into()),
    4 => Err(RequestIdentityError::UnsupportedEncryption.into()),
    6 => Err(RequestIdentityError::KeyMismatch.into()),
    _ => Err(RequestIdentityError::InvalidCertificate.into()),
  }
}
