use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{HandshakeRejection, MAX_PRELUDE, MAX_PRODUCT_VERSION, TransportError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientPrelude {
  pub product_version: String,
  pub service_instance_id: Uuid,
  pub session_id: Uuid,
}

impl ClientPrelude {
  pub fn encode(&self) -> Result<Vec<u8>, TransportError> {
    if self.product_version.is_empty() || self.product_version.len() > MAX_PRODUCT_VERSION {
      return Err(TransportError::MalformedMessage);
    }
    let encoded = serde_json::to_vec(self)?;
    if encoded.len() > MAX_PRELUDE {
      return Err(TransportError::MessageTooLarge);
    }
    Ok(encoded)
  }

  pub fn decode(encoded: &[u8]) -> Result<Self, TransportError> {
    if encoded.len() > MAX_PRELUDE {
      return Err(TransportError::MessageTooLarge);
    }
    let prelude: Self = serde_json::from_slice(encoded)?;
    if prelude.product_version.is_empty() || prelude.product_version.len() > MAX_PRODUCT_VERSION {
      return Err(TransportError::MalformedMessage);
    }
    Ok(prelude)
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "camelCase", deny_unknown_fields)]
pub enum ServerPrelude {
  Ready,
  Rejected(HandshakeRejection),
}

impl ServerPrelude {
  pub fn encode(&self) -> Result<Vec<u8>, TransportError> {
    let encoded = serde_json::to_vec(self)?;
    if encoded.len() > MAX_PRELUDE {
      return Err(TransportError::MessageTooLarge);
    }
    Ok(encoded)
  }

  pub fn decode(encoded: &[u8]) -> Result<Self, TransportError> {
    if encoded.len() > MAX_PRELUDE {
      return Err(TransportError::MessageTooLarge);
    }
    Ok(serde_json::from_slice(encoded)?)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn preludes_reject_unknown_fields_and_invalid_versions() {
    let instance = Uuid::new_v4();
    let session = Uuid::new_v4();
    let unknown = format!(
      r#"{{"product_version":"2.6.4","service_instance_id":"{instance}","session_id":"{session}","extra":true}}"#
    );
    assert!(ClientPrelude::decode(unknown.as_bytes()).is_err());
    assert!(
      ClientPrelude {
        product_version: String::new(),
        service_instance_id: instance,
        session_id: session,
      }
      .encode()
      .is_err()
    );
    assert!(ClientPrelude::decode(&vec![b'x'; MAX_PRELUDE + 1]).is_err());
  }
}
