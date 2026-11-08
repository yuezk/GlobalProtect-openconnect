//! Client identity capabilities, separate from peer certificate policy.
use pkcs8::{
  ObjectIdentifier, PrivateKeyInfoRef,
  der::{
    Decode, Reader, Tag, TagNumber, Tagged,
    asn1::{AnyRef, BitStringRef, ContextSpecific, OctetStringRef},
  },
};
use rustls::pki_types::PrivateKeyDer;

use crate::utils::request::RequestIdentityError;

#[cfg(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
))]
#[path = "native_key.rs"]
mod implementation;
#[cfg(not(any(
  target_os = "macos",
  target_os = "linux",
  target_os = "freebsd",
  target_os = "openbsd"
)))]
#[path = "portable_key.rs"]
mod implementation;

pub(in crate::utils) use implementation::ClientKey;

const RSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.1");
const RSA_PSS: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.1.10");
const EC: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
const ED25519: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.112");
const ED448: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.101.113");
const P521: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.3.132.0.35");
const DSA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10040.4.1");

fn validate_algorithm(oid: ObjectIdentifier) -> Result<(), RequestIdentityError> {
  match oid {
    RSA | EC | ED25519 => Ok(()),
    RSA_PSS => Err(RequestIdentityError::UnsupportedKeyAlgorithm("RSA-PSS")),
    ED448 => Err(RequestIdentityError::UnsupportedKeyAlgorithm("Ed448")),
    DSA => Err(RequestIdentityError::UnsupportedKeyAlgorithm("DSA")),
    _ => Err(RequestIdentityError::UnsupportedKeyAlgorithm(
      "an unknown key algorithm",
    )),
  }
}

fn validate_ec_parameters(parameters: Option<AnyRef<'_>>) -> Result<(), RequestIdentityError> {
  if let Some(parameters) = parameters {
    if parameters.tag() == Tag::ObjectIdentifier {
      let curve = parameters
        .decode_as::<ObjectIdentifier>()
        .map_err(|_| RequestIdentityError::InvalidKey)?;
      if curve == P521 {
        return Err(RequestIdentityError::UnsupportedKeyAlgorithm("P-521"));
      }
      return Ok(());
    }
    return Err(RequestIdentityError::UnsupportedKeyAlgorithm(
      "EC with explicit parameters",
    ));
  }
  Ok(())
}

fn validate_ec_point(point: &[u8]) -> Result<(), RequestIdentityError> {
  if matches!(point.first(), Some(2 | 3)) {
    return Err(RequestIdentityError::UnsupportedKeyAlgorithm(
      "EC with compressed public keys",
    ));
  }
  Ok(())
}

fn validate_sec1(bytes: &[u8]) -> anyhow::Result<()> {
  let (parameters, point) = AnyRef::from_der(bytes)
    .and_then(|sequence| {
      sequence.sequence(|reader| {
        let version: u8 = reader.decode()?;
        if version != 1 {
          return Err(reader.error(Tag::Integer.value_error()));
        }
        let _: &OctetStringRef = reader.decode()?;
        let parameters = ContextSpecific::<AnyRef<'_>>::decode_explicit(reader, TagNumber(0))?.map(|field| field.value);
        let point =
          ContextSpecific::<BitStringRef<'_>>::decode_explicit(reader, TagNumber(1))?.map(|field| field.value);
        Ok((parameters, point))
      })
    })
    .map_err(|_| RequestIdentityError::InvalidKey)?;
  validate_ec_parameters(parameters)?;
  if let Some(point) = point {
    validate_ec_point(point.as_bytes().ok_or(RequestIdentityError::InvalidKey)?)?;
  }
  Ok(())
}

pub(super) fn validate_private_key(key: &PrivateKeyDer<'_>) -> anyhow::Result<()> {
  match key {
    PrivateKeyDer::Pkcs8(key) => {
      let info = PrivateKeyInfoRef::from_der(key.secret_pkcs8_der()).map_err(|_| RequestIdentityError::InvalidKey)?;
      validate_algorithm(info.algorithm.oid)?;
      if info.algorithm.oid == EC {
        validate_ec_parameters(info.algorithm.parameters)?;
        validate_sec1(info.private_key.as_bytes())?;
      }
    }
    PrivateKeyDer::Sec1(key) => validate_sec1(key.secret_sec1_der())?,
    PrivateKeyDer::Pkcs1(_) => {}
    _ => return Err(RequestIdentityError::UnsupportedKeyFormat.into()),
  }
  Ok(())
}

pub(in crate::utils) fn validate_certificate(bytes: &[u8]) -> anyhow::Result<()> {
  let certificate = x509_cert::Certificate::from_der(bytes).map_err(|_| RequestIdentityError::InvalidCertificate)?;
  let public_key = certificate.tbs_certificate().subject_public_key_info();
  validate_algorithm(public_key.algorithm.oid)?;
  if public_key.algorithm.oid == EC {
    validate_ec_parameters(public_key.algorithm.parameters.as_ref().map(|value| value.into()))?;
    validate_ec_point(
      public_key
        .subject_public_key
        .as_bytes()
        .ok_or(RequestIdentityError::InvalidCertificate)?,
    )?;
  }
  Ok(())
}
