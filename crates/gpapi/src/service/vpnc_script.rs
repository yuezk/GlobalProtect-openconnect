use anyhow::bail;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::utils::base64;

pub const MAX_VPNC_SCRIPT_SOURCE_VALUE: usize = 32 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum VpncScriptSource {
  File,
  Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VpncScriptMetadata {
  pub source: VpncScriptSource,
  pub value: String,
}

impl VpncScriptMetadata {
  pub fn new(source: VpncScriptSource, value: String) -> anyhow::Result<Self> {
    if value.trim().is_empty() {
      bail!("VPNC script source is empty");
    }
    if value.contains('\0') {
      bail!("VPNC script source contains a null character");
    }
    if value.len() > MAX_VPNC_SCRIPT_SOURCE_VALUE {
      bail!(
        "VPNC script source exceeds the {} byte limit",
        MAX_VPNC_SCRIPT_SOURCE_VALUE
      );
    }
    Ok(Self { source, value })
  }

  pub fn validate(&self) -> anyhow::Result<()> {
    Self::new(self.source, self.value.clone()).map(|_| ())
  }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct InstallVpncScriptRequest {
  pub metadata: VpncScriptMetadata,
  contents: String,
}

impl InstallVpncScriptRequest {
  pub fn new(metadata: VpncScriptMetadata, contents: &[u8]) -> Self {
    Self {
      metadata,
      contents: base64::encode(contents),
    }
  }

  pub fn contents(&self) -> anyhow::Result<Vec<u8>> {
    base64::decode_to_vec(&self.contents)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn install_request_round_trips_binary_contents() {
    let metadata = VpncScriptMetadata::new(VpncScriptSource::File, "/tmp/vpnc-script".to_owned()).unwrap();
    let request = InstallVpncScriptRequest::new(metadata, b"#!/bin/sh\n");
    let encoded = serde_json::to_vec(&request).unwrap();
    let decoded: InstallVpncScriptRequest = serde_json::from_slice(&encoded).unwrap();

    assert_eq!(decoded.contents().unwrap(), b"#!/bin/sh\n");
  }

  #[test]
  fn metadata_rejects_invalid_source_value() {
    assert!(VpncScriptMetadata::new(VpncScriptSource::Command, String::new()).is_err());
    assert!(VpncScriptMetadata::new(VpncScriptSource::Command, "echo\0value".to_owned()).is_err());
    assert!(VpncScriptMetadata::new(VpncScriptSource::Command, "x".repeat(MAX_VPNC_SCRIPT_SOURCE_VALUE + 1)).is_err());
  }
}
