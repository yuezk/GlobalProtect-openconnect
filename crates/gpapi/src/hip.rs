use serde::{Deserialize, Serialize};
use specta::Type;

/// The GUI's choice of HIP report source. gpservice resolves execution details.
#[derive(Clone, Debug, Default, Deserialize, Serialize, Type, PartialEq, Eq)]
#[serde(tag = "source", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum HipSource {
  #[default]
  Disabled,
  Generated,
  Edited {
    report_id: String,
  },
  UserScript {
    path: String,
  },
  ApprovedRootScript {
    approval_id: String,
  },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum HipApprovalStatus {
  Valid,
  Revoked,
  Corrupt,
  OtherUser,
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn source_round_trips_without_execution_uid_or_wrapper() {
    let source = HipSource::ApprovedRootScript {
      approval_id: "approved-1".into(),
    };
    let json = serde_json::to_value(&source).unwrap();
    assert_eq!(
      json,
      serde_json::json!({ "source": "approvedRootScript", "approvalId": "approved-1" })
    );
    assert_eq!(serde_json::from_value::<HipSource>(json).unwrap(), source);
  }
}

/// Collection metadata for the exact XML returned by a generated preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HipReport {
  pub xml: String,
  pub windows_updates: Option<WindowsUpdateStatus>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WindowsUpdateStatus {
  /// Present only when this report includes a completed local patch observation.
  pub checked_at: Option<u64>,
  /// Time of the observed snapshot when its patch details were too large to include.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub omitted_snapshot_at: Option<u64>,
}

impl WindowsUpdateStatus {
  pub fn refresh_cursor(&self) -> Option<u64> {
    self.checked_at.or(self.omitted_snapshot_at)
  }
}
