use std::fmt;

use ::base64::{Engine, engine::general_purpose};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::utils::base64;

use super::{MAX_CREDENTIAL_FRAME, MAX_PRODUCT_VERSION, TransportError};

pub struct SessionCredential {
  service_instance_id: Uuid,
  session_id: Uuid,
  secret: Zeroizing<[u8; 32]>,
  product_version: String,
}

impl SessionCredential {
  pub fn generate(service_instance_id: Uuid, product_version: &str) -> Result<Self, TransportError> {
    if product_version.is_empty() || product_version.len() > MAX_PRODUCT_VERSION {
      return Err(TransportError::InvalidCredential);
    }

    let mut secret = Zeroizing::new([0_u8; 32]);
    getrandom::fill(secret.as_mut()).map_err(|_| TransportError::InvalidState)?;

    Ok(Self {
      service_instance_id,
      session_id: Uuid::new_v4(),
      secret,
      product_version: product_version.to_owned(),
    })
  }

  pub fn service_instance_id(&self) -> Uuid {
    self.service_instance_id
  }

  pub fn session_id(&self) -> Uuid {
    self.session_id
  }

  pub fn secret(&self) -> &[u8; 32] {
    &self.secret
  }

  pub fn product_version(&self) -> &str {
    &self.product_version
  }

  pub fn encode_frame(&self) -> Result<Zeroizing<Vec<u8>>, TransportError> {
    let encoded_secret = Zeroizing::new(base64::encode(self.secret()));
    let dto = CredentialDtoRef {
      service_instance_id: self.service_instance_id,
      session_id: self.session_id,
      secret: encoded_secret.as_str(),
      product_version: &self.product_version,
    };
    let payload = Zeroizing::new(serde_json::to_vec(&dto)?);

    if payload.len() + 2 > MAX_CREDENTIAL_FRAME || payload.len() > u16::MAX as usize {
      return Err(TransportError::MessageTooLarge);
    }

    let mut frame = Zeroizing::new(Vec::with_capacity(payload.len() + 2));
    frame.extend_from_slice(&(payload.len() as u16).to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
  }

  pub fn decode_frame(frame: &[u8]) -> Result<Self, TransportError> {
    if frame.len() < 2 || frame.len() > MAX_CREDENTIAL_FRAME {
      return Err(TransportError::InvalidCredential);
    }

    let payload_len = u16::from_be_bytes([frame[0], frame[1]]) as usize;
    if payload_len == 0 || payload_len + 2 != frame.len() {
      return Err(TransportError::InvalidCredential);
    }

    let dto: CredentialDto = serde_json::from_slice(&frame[2..]).map_err(|_| TransportError::InvalidCredential)?;
    let CredentialDto {
      service_instance_id,
      session_id,
      secret,
      product_version,
    } = dto;
    let encoded_secret = secret;
    if product_version.is_empty() || product_version.len() > MAX_PRODUCT_VERSION {
      return Err(TransportError::InvalidCredential);
    }

    let mut secret = Zeroizing::new([0_u8; 32]);
    let decoded_len = general_purpose::STANDARD
      .decode_slice(encoded_secret.as_bytes(), secret.as_mut())
      .map_err(|_| TransportError::InvalidCredential)?;
    if decoded_len != secret.len() {
      return Err(TransportError::InvalidCredential);
    }

    Ok(Self {
      service_instance_id,
      session_id,
      secret,
      product_version,
    })
  }
}

impl fmt::Debug for SessionCredential {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("SessionCredential")
      .field("service_instance_id", &self.service_instance_id)
      .field("session_id", &self.session_id)
      .field("secret", &"[REDACTED]")
      .field("product_version", &self.product_version)
      .finish()
  }
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct CredentialDtoRef<'a> {
  service_instance_id: Uuid,
  session_id: Uuid,
  secret: &'a str,
  product_version: &'a str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CredentialDto {
  service_instance_id: Uuid,
  session_id: Uuid,
  secret: Zeroizing<String>,
  product_version: String,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn credential_round_trips_without_debugging_secret() {
    let credential = SessionCredential::generate(Uuid::new_v4(), "2.6.4").unwrap();
    let frame = credential.encode_frame().unwrap();
    let decoded = SessionCredential::decode_frame(&frame).unwrap();

    assert_eq!(decoded.service_instance_id(), credential.service_instance_id());
    assert_eq!(decoded.session_id(), credential.session_id());
    assert_eq!(decoded.secret(), credential.secret());
    assert_eq!(decoded.product_version(), credential.product_version());
    assert!(!format!("{credential:?}").contains(&base64::encode(credential.secret())));
  }

  #[test]
  fn credential_rejects_malformed_lengths() {
    assert!(SessionCredential::decode_frame(&[]).is_err());
    assert!(SessionCredential::decode_frame(&[0, 0]).is_err());
    assert!(SessionCredential::decode_frame(&[0, 2, b'{']).is_err());
    assert!(SessionCredential::decode_frame(&vec![0; MAX_CREDENTIAL_FRAME + 1]).is_err());
  }
}
