//! Observed tunnel data, shared by standalone clients and service consumers.
//! These values describe negotiated settings, not verified operating-system state.

use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

pub const MAX_CONFIGURATION_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TunnelPhase {
  Establishing,
  Connected,
  Reconnecting,
  Ended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TunnelTransport {
  Negotiating,
  IpsecEsp,
  SslTls,
  Unavailable,
}

impl TunnelTransport {
  pub fn is_active(self) -> bool {
    matches!(self, Self::IpsecEsp | Self::SslTls)
  }

  pub fn label(self) -> &'static str {
    match self {
      Self::Negotiating => "Negotiating",
      Self::IpsecEsp => "IPsec (ESP)",
      Self::SslTls => "SSL/TLS",
      Self::Unavailable => "Unavailable",
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelList {
  pub items: Vec<String>,
  pub total_count: u32,
  pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelAddress {
  pub address: String,
  pub netmask: Option<String>,
  pub prefix_length: Option<u8>,
}

impl TunnelAddress {
  pub fn display(&self) -> String {
    match self.prefix_length {
      Some(prefix) => format!("{}/{prefix}", self.address),
      None => match &self.netmask {
        Some(netmask) => format!("{} (mask {netmask})", self.address),
        None => self.address.clone(),
      },
    }
  }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelConfiguration {
  pub interface: Option<String>,
  pub ipv4: Option<TunnelAddress>,
  pub ipv6: Option<TunnelAddress>,
  pub server_ip: Option<String>,
  pub dns: TunnelList,
  pub search_domains: TunnelList,
  pub split_dns: TunnelList,
  pub included_routes: TunnelList,
  pub excluded_routes: TunnelList,
  pub mtu: Option<u32>,
  pub truncated: bool,
}

impl TunnelConfiguration {
  /// Bound observation payloads without modifying native network configuration.
  /// Whole list entries are omitted before shortening scalar text. Counts and
  /// truncation flags preserve the distinction between absent and omitted data.
  /// Returns false if even the fixed JSON shape cannot fit the supplied budget.
  pub fn limit_encoded_size(&mut self, limit: usize) -> bool {
    while serde_json::to_vec(self).expect("configuration is serializable").len() > limit {
      self.truncated = true;
      let lists = [
        &mut self.excluded_routes,
        &mut self.included_routes,
        &mut self.split_dns,
        &mut self.search_domains,
        &mut self.dns,
      ];
      if let Some(list) = lists.into_iter().find(|list| !list.items.is_empty()) {
        list.items.pop();
        list.truncated = true;
        continue;
      }
      let mut scalars = vec![&mut self.interface, &mut self.server_ip];
      if let Some(address) = &mut self.ipv4 {
        scalars.push(&mut address.netmask);
        if address.address.len() > 128 {
          address.address.truncate(address.address.floor_char_boundary(128));
          address.prefix_length = None;
        }
      }
      if let Some(address) = &mut self.ipv6 {
        scalars.push(&mut address.netmask);
        if address.address.len() > 128 {
          address.address.truncate(address.address.floor_char_boundary(128));
          address.prefix_length = None;
        }
      }
      match scalars
        .into_iter()
        .max_by_key(|value| value.as_ref().map_or(0, String::len))
      {
        Some(Some(text)) if !text.is_empty() => {
          text.truncate(text.floor_char_boundary(text.len() / 2));
        }
        _ => return serde_json::to_vec(self).expect("configuration is serializable").len() <= limit,
      }
    }
    true
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelState {
  pub tunnel_id: Uuid,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub sequence: u64,
  pub phase: TunnelPhase,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub configuration_revision: u64,
  pub configuration: Option<TunnelConfiguration>,
  pub transport: TunnelTransport,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStatistics {
  pub tunnel_id: Uuid,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub sequence: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub state_sequence: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub configuration_revision: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub elapsed_ms: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub received_bytes: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub sent_bytes: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub received_packets: u64,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub sent_packets: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TunnelStatisticsState {
  Disabled,
  Waiting,
  Fresh,
  Stale,
  Ended,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStatisticsView {
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub sequence: u64,
  pub tunnel_id: Uuid,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub state_sequence: u64,
  pub phase: TunnelPhase,
  pub transport: TunnelTransport,
  pub state: TunnelStatisticsState,
  #[serde(with = "decimal")]
  #[specta(type = String)]
  pub elapsed_ms: u64,
  #[serde(with = "optional_decimal")]
  #[specta(type = Option<String>)]
  pub sample_age_ms: Option<u64>,
  pub sample: Option<TunnelStatistics>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TunnelUnavailableReason {
  NoTunnel,
  NonTunnel,
  ObservationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TunnelStatisticsSnapshot {
  Available(TunnelStatisticsView),
  Unavailable { reason: TunnelUnavailableReason },
}

impl TunnelStatisticsSnapshot {
  pub fn unavailable(reason: TunnelUnavailableReason) -> Self {
    Self::Unavailable { reason }
  }

  pub fn view(&self) -> Option<&TunnelStatisticsView> {
    match self {
      Self::Available(view) => Some(view),
      Self::Unavailable { .. } => None,
    }
  }

  pub fn tunnel_id(&self) -> Option<Uuid> {
    self.view().map(|view| view.tunnel_id)
  }

  pub fn sequence(&self) -> Option<u64> {
    self.view().map(|view| view.sequence)
  }

  pub fn state_sequence(&self) -> Option<u64> {
    self.view().map(|view| view.state_sequence)
  }

  pub fn disabled(&self) -> Self {
    let mut snapshot = self.clone();
    if let Self::Available(view) = &mut snapshot {
      if view.state != TunnelStatisticsState::Ended {
        view.state = TunnelStatisticsState::Disabled;
      }
    }
    snapshot
  }

  pub fn stale(&self) -> Self {
    let mut snapshot = self.clone();
    if let Self::Available(view) = &mut snapshot {
      if matches!(
        view.state,
        TunnelStatisticsState::Fresh | TunnelStatisticsState::Waiting
      ) {
        view.state = TunnelStatisticsState::Stale;
      }
    }
    snapshot
  }
}

pub(crate) mod decimal {
  use serde::{Deserialize, Deserializer, Serializer};

  pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.collect_str(value)
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
      return Err(serde::de::Error::custom("expected an unsigned decimal string"));
    }
    value.parse().map_err(serde::de::Error::custom)
  }
}

mod optional_decimal {
  use serde::{Deserialize, Deserializer, Serialize, Serializer};

  pub fn serialize<S: Serializer>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error> {
    value.map(|value| value.to_string()).serialize(serializer)
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<u64>, D::Error> {
    Option::<String>::deserialize(deserializer)?
      .map(|value| {
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
          return Err(serde::de::Error::custom("expected an unsigned decimal string"));
        }
        value.parse().map_err(serde::de::Error::custom)
      })
      .transpose()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn counters_above_javascript_precision_round_trip_as_decimal_strings() {
    let sample = TunnelStatistics {
      tunnel_id: Uuid::nil(),
      sequence: 1,
      state_sequence: 1,
      configuration_revision: 1,
      elapsed_ms: 1000,
      received_bytes: (1_u64 << 53) + 1,
      sent_bytes: u64::MAX,
      received_packets: 3,
      sent_packets: 4,
    };
    let value = serde_json::to_value(&sample).unwrap();
    assert_eq!(value["receivedBytes"], "9007199254740993");
    assert_eq!(value["sentBytes"], "18446744073709551615");
    assert_eq!(
      serde_json::from_value::<TunnelStatistics>(value.clone()).unwrap(),
      sample
    );
    let mut numeric = value;
    numeric["receivedBytes"] = serde_json::json!(9007199254740993_u64);
    assert!(serde_json::from_value::<TunnelStatistics>(numeric).is_err());
  }

  #[test]
  fn unavailable_has_no_synthetic_counters() {
    let snapshot = TunnelStatisticsSnapshot::unavailable(TunnelUnavailableReason::NonTunnel);
    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value, serde_json::json!({"kind": "unavailable", "reason": "nonTunnel"}));
    assert_eq!(
      serde_json::from_value::<TunnelStatisticsSnapshot>(value).unwrap(),
      snapshot
    );
  }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NegotiatedNetwork {
  pub addresses: Vec<std::net::IpAddr>,
  pub dns: Vec<std::net::IpAddr>,
  pub search: Vec<String>,
  pub mtu: usize,
}
