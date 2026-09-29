//! Neutral device-anchor transport model and deterministic derivation.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum AnchorSource {
  UnixMac,
  UnixRandom,
  MacosIokit,
}

impl From<AnchorSource> for u8 {
  fn from(source: AnchorSource) -> Self {
    match source {
      AnchorSource::UnixMac => 1,
      AnchorSource::UnixRandom => 2,
      AnchorSource::MacosIokit => 3,
    }
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidAnchorSource(u8);

impl fmt::Display for InvalidAnchorSource {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "unsupported device anchor source {}", self.0)
  }
}

impl std::error::Error for InvalidAnchorSource {}

impl TryFrom<u8> for AnchorSource {
  type Error = InvalidAnchorSource;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      1 => Ok(Self::UnixMac),
      2 => Ok(Self::UnixRandom),
      3 => Ok(Self::MacosIokit),
      value => Err(InvalidAnchorSource(value)),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceAnchor {
  pub hardware_id: String,
  pub source: AnchorSource,
}

impl DeviceAnchor {
  pub fn validate(&self) -> bool {
    URL_SAFE_NO_PAD
      .decode(&self.hardware_id)
      .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(&bytes) == self.hardware_id)
  }

  pub fn derive(source: AnchorSource, seed: impl AsRef<[u8]>) -> Self {
    let domain = match source {
      AnchorSource::UnixMac => b"gp-device-anchor:v1:unix-mac:".as_slice(),
      AnchorSource::UnixRandom => b"gp-device-anchor:v1:unix-random:".as_slice(),
      AnchorSource::MacosIokit => b"gp-device-anchor:v1:macos-platform-uuid:".as_slice(),
    };
    let mut input = domain.to_vec();
    input.extend_from_slice(seed.as_ref());
    Self {
      hardware_id: URL_SAFE_NO_PAD.encode(openssl::sha::sha256(&input)),
      source,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hashes_original_seed_order_with_stable_domain_separation() {
    let mac = DeviceAnchor::derive(AnchorSource::UnixMac, [0, 17, 34, 51, 68, 85]);
    assert!(mac.validate());
    assert_eq!(mac.hardware_id, "UWDOx4AVJEuCAYfJBaPioyGtRfAkJBhMYSi8LuFsE1A");
    assert_ne!(
      mac.hardware_id,
      DeviceAnchor::derive(AnchorSource::UnixMac, [85, 68, 51, 34, 17, 0]).hardware_id
    );
    assert_ne!(
      mac.hardware_id,
      DeviceAnchor::derive(AnchorSource::UnixRandom, [0, 17, 34, 51, 68, 85]).hardware_id
    );
  }

  #[test]
  fn source_uses_stable_numeric_json_codes() {
    assert_eq!(serde_json::to_string(&AnchorSource::UnixMac).unwrap(), "1");
    assert_eq!(serde_json::to_string(&AnchorSource::UnixRandom).unwrap(), "2");
    assert_eq!(serde_json::to_string(&AnchorSource::MacosIokit).unwrap(), "3");
    assert_eq!(
      serde_json::from_str::<AnchorSource>("1").unwrap(),
      AnchorSource::UnixMac
    );
    assert!(serde_json::from_str::<AnchorSource>("0").is_err());
    assert!(serde_json::from_str::<AnchorSource>("4").is_err());
    assert!(serde_json::from_str::<AnchorSource>(r#""unix_mac""#).is_err());
  }
}
