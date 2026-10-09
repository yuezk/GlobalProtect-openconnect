use std::net::{IpAddr, SocketAddr};

use anyhow::ensure;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::super::ClientAddresses;

/// Physical network context bound to the authenticated non-tunnel endpoint. Wire input is validated;
/// runtime transports use this context without performing another DNS lookup.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(try_from = "BindingFields", rename_all = "camelCase")]
pub struct GatewayBinding {
  endpoint: SocketAddr,
  source: SocketAddr,
  interface: String,
  interface_index: u32,
  addresses: ClientAddresses,
  network_fingerprint: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BindingFields {
  endpoint: SocketAddr,
  source: SocketAddr,
  interface: String,
  interface_index: u32,
  addresses: ClientAddresses,
  network_fingerprint: Option<String>,
}

impl TryFrom<BindingFields> for GatewayBinding {
  type Error = anyhow::Error;
  fn try_from(fields: BindingFields) -> Result<Self, Self::Error> {
    Self::new(
      fields.endpoint,
      fields.source,
      fields.interface,
      fields.interface_index,
      fields.addresses,
      fields.network_fingerprint,
    )
  }
}

impl GatewayBinding {
  pub fn new(
    endpoint: SocketAddr,
    source: SocketAddr,
    interface: String,
    interface_index: u32,
    addresses: ClientAddresses,
    network_fingerprint: impl Into<Option<String>>,
  ) -> anyhow::Result<Self> {
    let network_fingerprint = network_fingerprint.into();
    ensure!(
      network_fingerprint
        .as_ref()
        .is_none_or(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())),
      "Physical network fingerprint is invalid"
    );
    ensure!(
      endpoint.port() != 0 && !endpoint.ip().is_unspecified() && !endpoint.ip().is_multicast(),
      "Gateway endpoint is invalid"
    );
    ensure!(
      source.port() == 0 && !source.ip().is_unspecified() && !source.ip().is_multicast(),
      "Gateway source is invalid"
    );
    ensure!(
      endpoint.is_ipv4() == source.is_ipv4(),
      "Gateway address families differ"
    );
    ensure!(
      !interface.is_empty() && interface.len() < 64 && !interface.contains('\0') && interface_index != 0,
      "Gateway interface is invalid"
    );
    for address in [endpoint, source] {
      if let SocketAddr::V6(address) = address {
        ensure!(address.flowinfo() == 0, "Gateway IPv6 flow information is invalid");
        ensure!(
          address.scope_id() == 0 || address.scope_id() == interface_index,
          "Gateway IPv6 scope differs from its interface"
        );
        ensure!(
          !address.ip().is_unicast_link_local() || address.scope_id() == interface_index,
          "Gateway IPv6 scope is missing"
        );
      }
    }
    match source.ip() {
      IpAddr::V4(ip) => ensure!(
        addresses.ipv4 == Some(ip),
        "Gateway IPv4 source differs from its identity"
      ),
      IpAddr::V6(ip) => ensure!(
        addresses.ipv6 == Some(ip),
        "Gateway IPv6 source differs from its identity"
      ),
    }
    Ok(Self {
      endpoint,
      source,
      interface,
      interface_index,
      addresses,
      network_fingerprint,
    })
  }

  pub fn endpoint(&self) -> SocketAddr {
    self.endpoint
  }
  pub fn source(&self) -> SocketAddr {
    self.source
  }
  pub fn interface(&self) -> &str {
    &self.interface
  }
  pub fn interface_index(&self) -> u32 {
    self.interface_index
  }
  pub fn addresses(&self) -> ClientAddresses {
    self.addresses
  }

  pub fn network_fingerprint(&self) -> Option<&str> {
    self.network_fingerprint.as_deref()
  }

  pub fn validate_server(&self, server: &str) -> anyhow::Result<()> {
    let origin = super::super::transport::gateway_origin(server)?;
    ensure!(
      origin.port_or_known_default() == Some(self.endpoint.port()),
      "Gateway endpoint port differs"
    );
    let literal = match origin.host() {
      Some(url::Host::Ipv4(ip)) => Some(IpAddr::V4(ip)),
      Some(url::Host::Ipv6(ip)) => Some(IpAddr::V6(ip)),
      _ => None,
    };
    if let Some(ip) = literal {
      ensure!(self.endpoint.ip() == ip, "Literal gateway endpoint differs");
    }
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn ipv4_binding() -> GatewayBinding {
    GatewayBinding::new(
      "192.0.2.10:443".parse().unwrap(),
      "192.0.2.5:0".parse().unwrap(),
      "fixture".into(),
      7,
      ClientAddresses {
        ipv4: Some("192.0.2.5".parse().unwrap()),
        ipv6: None,
      },
      "a".repeat(64),
    )
    .unwrap()
  }

  #[test]
  fn wire_input_rejects_inconsistent_network_identity() {
    let original = serde_json::to_value(ipv4_binding()).unwrap();
    for (field, value) in [
      ("source", serde_json::json!("192.0.2.6:0")),
      ("source", serde_json::json!("192.0.2.5:1234")),
      ("endpoint", serde_json::json!("[2001:db8::10]:443")),
      ("interfaceIndex", serde_json::json!(0)),
      ("networkFingerprint", serde_json::json!("not-a-fingerprint")),
    ] {
      let mut invalid = original.clone();
      invalid[field] = value;
      assert!(
        serde_json::from_value::<GatewayBinding>(invalid).is_err(),
        "accepted {field}"
      );
    }
    assert!(serde_json::from_value::<GatewayBinding>(original).is_ok());
  }

  #[test]
  fn link_local_scope_survives_wire_round_trip_and_must_match_interface() {
    let addresses = ClientAddresses {
      ipv4: None,
      ipv6: Some("fe80::5".parse().unwrap()),
    };
    let binding = GatewayBinding::new(
      "[fe80::10%7]:443".parse().unwrap(),
      "[fe80::5%7]:0".parse().unwrap(),
      "fixture".into(),
      7,
      addresses,
      "a".repeat(64),
    )
    .unwrap();
    let value = serde_json::to_value(&binding).unwrap();
    let restored: GatewayBinding = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.source(), binding.source());
    assert_eq!(restored.endpoint(), binding.endpoint());
    for source in ["[fe80::5]:0", "[fe80::5%8]:0"] {
      let mut invalid = value.clone();
      invalid["source"] = serde_json::json!(source);
      assert!(serde_json::from_value::<GatewayBinding>(invalid).is_err());
    }
  }

  #[test]
  fn server_origin_must_match_endpoint_port_and_literal_address() {
    let binding = ipv4_binding();
    assert!(binding.validate_server("gateway.example.com").is_ok());
    assert!(binding.validate_server("192.0.2.10").is_ok());
    assert!(binding.validate_server("192.0.2.11").is_err());
    assert!(binding.validate_server("gateway.example.com:444").is_err());
  }
}
