use std::{
  ffi::{c_int, c_void},
  fmt,
  ptr::NonNull,
  sync::{Arc, Mutex},
};

use crate::utils::request::RequestIdentityError;
use rustls::{
  SignatureAlgorithm, SignatureScheme,
  pki_types::{PrivateKeyDer, SubjectPublicKeyInfoDer},
  sign::{Signer, SigningKey},
};

unsafe extern "C" {
  fn gp_client_key_new(data: *const u8, length: usize, kind: *mut c_int, output: *mut *mut c_void) -> c_int;
  fn gp_client_key_delete(key: *mut c_void);
  fn gp_client_key_public(key: *mut c_void, output: *mut *mut u8, length: *mut usize) -> c_int;
  fn gp_client_key_matches(key: *mut c_void, cert: *const u8, length: usize) -> c_int;
  fn gp_client_key_sign(
    key: *mut c_void,
    scheme: u16,
    message: *const u8,
    length: usize,
    output: *mut *mut u8,
    output_length: *mut usize,
  ) -> c_int;
  fn gp_key_free(data: *mut u8, length: usize);
}

struct NativeKey(NonNull<c_void>);

// The handle is not thread-affine. All access and final release occur under its
// Rust owner; shared operations use the mutex in ClientKey.
unsafe impl Send for NativeKey {}

impl Drop for NativeKey {
  fn drop(&mut self) {
    unsafe { gp_client_key_delete(self.0.as_ptr()) };
  }
}

struct NativeBytes {
  data: *mut u8,
  length: usize,
}
impl Drop for NativeBytes {
  fn drop(&mut self) {
    unsafe { gp_key_free(self.data, self.length) };
  }
}
impl NativeBytes {
  fn new() -> Self {
    Self {
      data: std::ptr::null_mut(),
      length: 0,
    }
  }
  fn copy(&self) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
      !self.data.is_null() && self.length > 0,
      "Client certificate operation failed"
    );
    Ok(unsafe { std::slice::from_raw_parts(self.data, self.length) }.to_vec())
  }
}

#[derive(Clone)]
pub(in crate::utils) struct ClientKey {
  key: Arc<Mutex<NativeKey>>,
  kind: c_int,
  public_key: Vec<u8>,
}

impl fmt::Debug for ClientKey {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("ClientKey")
      .field("algorithm", &self.algorithm())
      .finish_non_exhaustive()
  }
}

impl ClientKey {
  pub(in crate::utils) fn load(key: &PrivateKeyDer<'_>) -> anyhow::Result<Self> {
    super::validate_private_key(key)?;
    let bytes = key.secret_der();
    let mut kind = 0;
    let mut pointer = std::ptr::null_mut();
    let result = unsafe { gp_client_key_new(bytes.as_ptr(), bytes.len(), &mut kind, &mut pointer) };
    if result == 2 {
      let algorithm = match kind {
        3 => "P-521",
        5 => "RSA-PSS",
        6 => "an unsupported elliptic curve",
        7 => "DSA",
        8 => "Ed448",
        _ => "an unknown key algorithm",
      };
      return Err(RequestIdentityError::UnsupportedKeyAlgorithm(algorithm).into());
    }
    if result != 0 {
      return Err(RequestIdentityError::InvalidKey.into());
    }
    let key = NativeKey(NonNull::new(pointer).ok_or(RequestIdentityError::InvalidKey)?);
    let mut exported = NativeBytes::new();
    let result = unsafe { gp_client_key_public(key.0.as_ptr(), &mut exported.data, &mut exported.length) };
    anyhow::ensure!(result == 0, "Could not read the client certificate's public key");
    Ok(Self {
      key: Arc::new(Mutex::new(key)),
      kind,
      public_key: exported.copy()?,
    })
  }

  pub(in crate::utils) fn matches_certificate(&self, certificate: &[u8]) -> anyhow::Result<bool> {
    let key = self
      .key
      .lock()
      .map_err(|_| anyhow::anyhow!("The client certificate is unavailable"))?;
    match unsafe { gp_client_key_matches(key.0.as_ptr(), certificate.as_ptr(), certificate.len()) } {
      1 => Ok(true),
      0 => Ok(false),
      _ => Err(RequestIdentityError::InvalidCertificate.into()),
    }
  }

  fn schemes(&self) -> &'static [SignatureScheme] {
    use SignatureScheme::*;
    match self.kind {
      0 => &[
        RSA_PSS_SHA512,
        RSA_PSS_SHA384,
        RSA_PSS_SHA256,
        RSA_PKCS1_SHA512,
        RSA_PKCS1_SHA384,
        RSA_PKCS1_SHA256,
      ],
      // Rustls does not expose the negotiated TLS version here. Keep TLS 1.3
      // curve/hash requirements; TLS 1.2 cross-curve hash offers remain unsupported.
      1 => &[ECDSA_NISTP256_SHA256],
      2 => &[ECDSA_NISTP384_SHA384],
      4 => &[ED25519],
      _ => &[],
    }
  }
}

impl SigningKey for ClientKey {
  fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
    self
      .schemes()
      .iter()
      .copied()
      .find(|scheme| offered.contains(scheme))
      .map(|scheme| {
        Box::new(ClientSigner {
          key: self.clone(),
          scheme,
        }) as Box<dyn Signer>
      })
  }
  fn public_key(&self) -> Option<SubjectPublicKeyInfoDer<'_>> {
    Some(SubjectPublicKeyInfoDer::from(self.public_key.as_slice()))
  }
  fn algorithm(&self) -> SignatureAlgorithm {
    match self.kind {
      0 => SignatureAlgorithm::RSA,
      4 => SignatureAlgorithm::ED25519,
      _ => SignatureAlgorithm::ECDSA,
    }
  }
}

#[derive(Debug)]
struct ClientSigner {
  key: ClientKey,
  scheme: SignatureScheme,
}
impl Signer for ClientSigner {
  fn sign(&self, message: &[u8]) -> Result<Vec<u8>, rustls::Error> {
    let error = || rustls::Error::General("Could not sign with the client certificate".into());
    let key = self.key.key.lock().map_err(|_| error())?;
    let mut output = NativeBytes::new();
    let result = unsafe {
      gp_client_key_sign(
        key.0.as_ptr(),
        self.scheme.into(),
        message.as_ptr(),
        message.len(),
        &mut output.data,
        &mut output.length,
      )
    };
    if result != 0 {
      return Err(error());
    }
    output.copy().map_err(|_| error())
  }
  fn scheme(&self) -> SignatureScheme {
    self.scheme
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use openssl::{
    hash::MessageDigest,
    pkey::PKey,
    rsa::Padding,
    sign::{RsaPssSaltlen, Verifier},
  };

  fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
      std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/files/client-identity")
        .join(name),
    )
    .unwrap()
  }

  fn native_key(bytes: &[u8]) -> ClientKey {
    let key = rustls_pemfile::private_key(&mut std::io::Cursor::new(bytes))
      .unwrap()
      .unwrap();
    ClientKey::load(&key).unwrap()
  }

  #[test]
  fn rsa_signatures_use_the_offered_scheme_and_tls_pss_salt_length() {
    let bytes = fixture("rsa-pkcs8.pem");
    let key = native_key(&bytes);
    let oracle = PKey::private_key_from_pem(&bytes).unwrap();
    let message = b"client authentication regression";
    for (scheme, digest, pss) in [
      (SignatureScheme::RSA_PSS_SHA256, MessageDigest::sha256(), true),
      (SignatureScheme::RSA_PSS_SHA384, MessageDigest::sha384(), true),
      (SignatureScheme::RSA_PSS_SHA512, MessageDigest::sha512(), true),
      (SignatureScheme::RSA_PKCS1_SHA256, MessageDigest::sha256(), false),
      (SignatureScheme::RSA_PKCS1_SHA384, MessageDigest::sha384(), false),
      (SignatureScheme::RSA_PKCS1_SHA512, MessageDigest::sha512(), false),
    ] {
      let signer = key.choose_scheme(&[scheme]).unwrap();
      assert_eq!(signer.scheme(), scheme);
      let signature = signer.sign(message).unwrap();
      let mut verifier = Verifier::new(digest, &oracle).unwrap();
      if pss {
        verifier.set_rsa_padding(Padding::PKCS1_PSS).unwrap();
        verifier.set_rsa_pss_saltlen(RsaPssSaltlen::DIGEST_LENGTH).unwrap();
        verifier.set_rsa_mgf1_md(digest).unwrap();
      }
      assert!(verifier.verify_oneshot(&signature, message).unwrap(), "{scheme:?}");
    }
    assert!(key.choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256]).is_none());
  }

  #[test]
  fn p256_signatures_verify_with_an_independent_backend() {
    let bytes = fixture("ec-pkcs8.pem");
    let key = native_key(&bytes);
    let oracle = PKey::private_key_from_pem(&bytes).unwrap();
    let signer = key.choose_scheme(&[SignatureScheme::ECDSA_NISTP256_SHA256]).unwrap();
    let message = b"native client key signature";
    let signature = signer.sign(message).unwrap();
    let mut verifier = Verifier::new(MessageDigest::sha256(), &oracle).unwrap();
    assert!(verifier.verify_oneshot(&signature, message).unwrap());
    assert!(key.choose_scheme(&[SignatureScheme::RSA_PSS_SHA256]).is_none());
  }

  #[test]
  fn unsupported_key_types_fail_at_import() {
    for (name, algorithm) in [("ec-p521.pem", "P-521"), ("rsa-pss.pem", "RSA-PSS")] {
      let bytes = fixture(name);
      let private_key = rustls_pemfile::private_key(&mut std::io::Cursor::new(bytes))
        .unwrap()
        .unwrap();
      let error = ClientKey::load(&private_key).unwrap_err();
      assert!(matches!(error.downcast_ref::<RequestIdentityError>(),
        Some(RequestIdentityError::UnsupportedKeyAlgorithm(actual)) if *actual == algorithm));
    }
  }

  #[test]
  fn a_key_remains_usable_when_shared_clients_sign_concurrently() {
    let key = native_key(&fixture("rsa-pkcs8.pem"));
    let mut workers = Vec::new();
    for _ in 0..4 {
      let key = key.clone();
      workers.push(std::thread::spawn(move || {
        key
          .choose_scheme(&[SignatureScheme::RSA_PSS_SHA256])
          .unwrap()
          .sign(b"shared identity")
          .unwrap()
      }));
    }
    let oracle = PKey::private_key_from_pem(&fixture("rsa-pkcs8.pem")).unwrap();
    for worker in workers {
      let signature = worker.join().unwrap();
      let mut verifier = Verifier::new(MessageDigest::sha256(), &oracle).unwrap();
      verifier.set_rsa_padding(Padding::PKCS1_PSS).unwrap();
      verifier.set_rsa_pss_saltlen(RsaPssSaltlen::DIGEST_LENGTH).unwrap();
      assert!(verifier.verify_oneshot(&signature, b"shared identity").unwrap());
    }
  }
}
