//! Neutral device-anchor transport model and Linux collection.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorSource {
  LinuxMac,
  LinuxRandom,
  MacosIokit,
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
      AnchorSource::LinuxMac => b"gp-device-anchor:v1:linux-mac:".as_slice(),
      AnchorSource::LinuxRandom => b"gp-device-anchor:v1:linux-random:".as_slice(),
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

#[cfg(unix)]
pub fn collect_unix() -> anyhow::Result<DeviceAnchor> {
  if let Some(mac) = mac_address::get_mac_address()? {
    let bytes = mac.bytes();
    if bytes != [0; 6] && bytes[0] & 1 == 0 {
      return Ok(DeviceAnchor::derive(AnchorSource::LinuxMac, bytes));
    }
  }
  let mut random = [0_u8; 32];
  getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("cannot initialize device anchor"))?;
  Ok(DeviceAnchor::derive(AnchorSource::LinuxRandom, random))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn hashes_are_domain_separated_and_canonical() {
    let mac = DeviceAnchor::derive(AnchorSource::LinuxMac, [0, 17, 34, 51, 68, 85]);
    assert!(mac.validate());
    assert_ne!(
      mac.hardware_id,
      DeviceAnchor::derive(AnchorSource::LinuxRandom, [0, 17, 34, 51, 68, 85]).hardware_id
    );
  }
}
