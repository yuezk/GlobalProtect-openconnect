use snow::{Builder, HandshakeState, TransportState, params::NoiseParams};
use zeroize::Zeroizing;

use super::{MAX_PLAINTEXT, MAX_PRODUCT_VERSION, MAX_WS_MESSAGE, TransportError};

const NOISE_PATTERN: &str = "Noise_NNpsk0_25519_ChaChaPoly_SHA256";
const NOISE_TAG_SIZE: usize = 16;

pub struct NoiseInitiator {
  state: Option<HandshakeState>,
}

impl NoiseInitiator {
  pub fn new(
    product_version: &str,
    service_instance_id: uuid::Uuid,
    session_id: uuid::Uuid,
    psk: &[u8; 32],
  ) -> Result<Self, TransportError> {
    let params = noise_params()?;
    let prologue = prologue(product_version, service_instance_id, session_id)?;
    Ok(Self {
      state: Some(
        Builder::new(params)
          .prologue(&prologue)?
          .psk(0, psk)?
          .build_initiator()?,
      ),
    })
  }

  pub fn write_first(&mut self) -> Result<Vec<u8>, TransportError> {
    let state = self.state.as_mut().ok_or(TransportError::InvalidState)?;
    let mut output = vec![0_u8; 128];
    let len = state.write_message(&[], &mut output)?;
    output.truncate(len);
    Ok(output)
  }

  pub fn read_response(mut self, message: &[u8]) -> Result<NoiseTransport, TransportError> {
    if message.len() > MAX_WS_MESSAGE {
      return Err(TransportError::MessageTooLarge);
    }
    let mut state = self.state.take().ok_or(TransportError::InvalidState)?;
    let mut output = vec![0_u8; 128];
    state.read_message(message, &mut output)?;
    Ok(NoiseTransport::new(state.into_transport_mode()?))
  }
}

pub struct NoiseResponder {
  state: Option<HandshakeState>,
}

impl NoiseResponder {
  pub fn new(
    product_version: &str,
    service_instance_id: uuid::Uuid,
    session_id: uuid::Uuid,
    psk: &[u8; 32],
  ) -> Result<Self, TransportError> {
    let params = noise_params()?;
    let prologue = prologue(product_version, service_instance_id, session_id)?;
    Ok(Self {
      state: Some(
        Builder::new(params)
          .prologue(&prologue)?
          .psk(0, psk)?
          .build_responder()?,
      ),
    })
  }

  pub fn read_first(&mut self, message: &[u8]) -> Result<(), TransportError> {
    if message.len() > MAX_WS_MESSAGE {
      return Err(TransportError::MessageTooLarge);
    }
    let state = self.state.as_mut().ok_or(TransportError::InvalidState)?;
    let mut output = vec![0_u8; 128];
    state.read_message(message, &mut output)?;
    Ok(())
  }

  pub fn write_response(mut self) -> Result<(Vec<u8>, NoiseTransport), TransportError> {
    let mut state = self.state.take().ok_or(TransportError::InvalidState)?;
    let mut output = vec![0_u8; 128];
    let len = state.write_message(&[], &mut output)?;
    output.truncate(len);
    let transport = NoiseTransport::new(state.into_transport_mode()?);
    Ok((output, transport))
  }
}

pub struct NoiseTransport {
  state: TransportState,
}

impl NoiseTransport {
  fn new(state: TransportState) -> Self {
    Self { state }
  }

  pub fn encrypt<T: serde::Serialize>(&mut self, message: &T) -> Result<Vec<u8>, TransportError> {
    let plaintext = Zeroizing::new(super::message::encode(message)?);
    let mut output = vec![0_u8; plaintext.len() + NOISE_TAG_SIZE];
    let len = self.state.write_message(&plaintext, &mut output)?;
    output.truncate(len);
    Ok(output)
  }

  pub fn decrypt<T: for<'de> serde::Deserialize<'de>>(&mut self, message: &[u8]) -> Result<T, TransportError> {
    if message.len() > MAX_WS_MESSAGE || message.len() < NOISE_TAG_SIZE {
      return Err(if message.len() > MAX_WS_MESSAGE {
        TransportError::MessageTooLarge
      } else {
        TransportError::MalformedMessage
      });
    }
    let mut plaintext = Zeroizing::new(vec![0_u8; message.len()]);
    let len = self.state.read_message(message, &mut plaintext)?;
    plaintext.truncate(len);
    if plaintext.len() > MAX_PLAINTEXT {
      return Err(TransportError::MessageTooLarge);
    }
    super::message::decode(&plaintext)
  }
}

fn noise_params() -> Result<NoiseParams, TransportError> {
  NOISE_PATTERN.parse().map_err(|_| TransportError::InvalidState)
}

fn prologue(
  product_version: &str,
  service_instance_id: uuid::Uuid,
  session_id: uuid::Uuid,
) -> Result<Vec<u8>, TransportError> {
  if product_version.is_empty() || product_version.len() > MAX_PRODUCT_VERSION {
    return Err(TransportError::MessageTooLarge);
  }
  let mut value = Vec::with_capacity(5 + 2 + product_version.len() + 32);
  value.extend_from_slice(b"GPWS\0");
  value.extend_from_slice(&(product_version.len() as u16).to_be_bytes());
  value.extend_from_slice(product_version.as_bytes());
  value.extend_from_slice(service_instance_id.as_bytes());
  value.extend_from_slice(session_id.as_bytes());
  Ok(value)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
  struct Payload(String);

  #[test]
  fn peers_exchange_directional_messages() {
    let instance = uuid::Uuid::new_v4();
    let session = uuid::Uuid::new_v4();
    let psk = [7_u8; 32];
    let mut initiator = NoiseInitiator::new("2.6.4", instance, session, &psk).unwrap();
    let mut responder = NoiseResponder::new("2.6.4", instance, session, &psk).unwrap();

    let first = initiator.write_first().unwrap();
    responder.read_first(&first).unwrap();
    let (response, mut server) = responder.write_response().unwrap();
    let mut client = initiator.read_response(&response).unwrap();

    let encrypted = client.encrypt(&Payload("hello".into())).unwrap();
    assert_eq!(server.decrypt::<Payload>(&encrypted).unwrap(), Payload("hello".into()));
    assert!(server.decrypt::<Payload>(&encrypted).is_err());

    let response = server.encrypt(&Payload("world".into())).unwrap();
    assert_eq!(client.decrypt::<Payload>(&response).unwrap(), Payload("world".into()));
  }

  #[test]
  fn wrong_psk_fails_handshake() {
    let instance = uuid::Uuid::new_v4();
    let session = uuid::Uuid::new_v4();
    let mut initiator = NoiseInitiator::new("2.6.4", instance, session, &[1; 32]).unwrap();
    let mut responder = NoiseResponder::new("2.6.4", instance, session, &[2; 32]).unwrap();
    let first = initiator.write_first().unwrap();
    assert!(responder.read_first(&first).is_err());
  }

  #[test]
  fn transcript_identity_mismatch_fails_handshake() {
    let instance = uuid::Uuid::new_v4();
    let session = uuid::Uuid::new_v4();
    let psk = [3_u8; 32];
    for mut responder in [
      NoiseResponder::new("2.6.5", instance, session, &psk).unwrap(),
      NoiseResponder::new("2.6.4", uuid::Uuid::new_v4(), session, &psk).unwrap(),
      NoiseResponder::new("2.6.4", instance, uuid::Uuid::new_v4(), &psk).unwrap(),
    ] {
      let mut initiator = NoiseInitiator::new("2.6.4", instance, session, &psk).unwrap();
      let first = initiator.write_first().unwrap();
      assert!(responder.read_first(&first).is_err());
    }
  }

  #[test]
  fn transport_rejects_tampering_truncation_and_reordering() {
    let instance = uuid::Uuid::new_v4();
    let session = uuid::Uuid::new_v4();
    let psk = [9_u8; 32];
    let mut initiator = NoiseInitiator::new("2.6.4", instance, session, &psk).unwrap();
    let mut responder = NoiseResponder::new("2.6.4", instance, session, &psk).unwrap();
    let first = initiator.write_first().unwrap();
    responder.read_first(&first).unwrap();
    let (response, mut server) = responder.write_response().unwrap();
    let mut client = initiator.read_response(&response).unwrap();

    let first = client.encrypt(&Payload("first".into())).unwrap();
    let second = client.encrypt(&Payload("second".into())).unwrap();
    assert!(server.decrypt::<Payload>(&second).is_err());

    let mut initiator = NoiseInitiator::new("2.6.4", instance, session, &psk).unwrap();
    let mut responder = NoiseResponder::new("2.6.4", instance, session, &psk).unwrap();
    let handshake = initiator.write_first().unwrap();
    responder.read_first(&handshake).unwrap();
    let (response, mut server) = responder.write_response().unwrap();
    let mut client = initiator.read_response(&response).unwrap();
    let mut tampered = client.encrypt(&Payload("value".into())).unwrap();
    tampered[0] ^= 1;
    assert!(server.decrypt::<Payload>(&tampered).is_err());
    assert!(server.decrypt::<Payload>(&first[..NOISE_TAG_SIZE - 1]).is_err());
  }
}
