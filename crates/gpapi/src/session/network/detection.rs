use std::net::IpAddr;

use anyhow::ensure;
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(try_from = "DetectionFields")]
pub struct InternalHostDetection {
  targets: Vec<DetectionTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
struct DetectionTarget {
  address: IpAddr,
  hostname: String,
}

#[derive(Deserialize)]
struct DetectionFields {
  targets: Vec<DetectionTarget>,
}

impl TryFrom<DetectionFields> for InternalHostDetection {
  type Error = anyhow::Error;

  fn try_from(fields: DetectionFields) -> anyhow::Result<Self> {
    Self::new(
      fields
        .targets
        .into_iter()
        .map(|target| (target.address, target.hostname))
        .collect(),
    )
  }
}

impl InternalHostDetection {
  pub fn new(targets: Vec<(IpAddr, String)>) -> anyhow::Result<Self> {
    ensure!(
      !targets.is_empty() && targets.len() <= 2,
      "Invalid internal detection target count"
    );
    let targets = targets
      .into_iter()
      .map(|(address, hostname)| {
        validate_target(address, &hostname)?;
        Ok(DetectionTarget { address, hostname })
      })
      .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Self { targets })
  }

  pub async fn detect(&self, cancellation: &CancellationToken) -> anyhow::Result<bool> {
    for target in &self.targets {
      if super::native::detect(target.address, &target.hostname, cancellation).await? {
        return Ok(true);
      }
    }
    Ok(false)
  }
}

pub(super) fn validate_target(address: IpAddr, hostname: &str) -> anyhow::Result<()> {
  ensure!(
    !address.is_unspecified() && !address.is_loopback() && !address.is_multicast(),
    "Invalid internal detection address"
  );
  ensure!(
    !hostname.is_empty()
      && hostname.len() <= 253
      && hostname.is_ascii()
      && hostname
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
    "Invalid internal detection hostname"
  );
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn detection_inputs_are_retained_and_validated_on_deserialization() {
    let detection = InternalHostDetection::new(vec![("192.0.2.10".parse().unwrap(), "host.example".into())]).unwrap();
    let value = serde_json::to_value(&detection).unwrap();
    assert!(serde_json::from_value::<InternalHostDetection>(value).is_ok());
    assert!(InternalHostDetection::new(vec![]).is_err());
    assert!(InternalHostDetection::new(vec![("127.0.0.1".parse().unwrap(), "host.example".into())]).is_err());
  }
}
