//! Owned observations of one native tunnel. Callbacks run synchronously on the
//! connection thread; observers must return promptly and must not call connect.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transport {
  Negotiating,
  IpsecEsp,
  SslTls,
  Unavailable,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeList {
  pub items: Vec<String>,
  pub total_count: u32,
  pub truncated: bool,
}

/// Negotiated settings, not proof that OS network configuration succeeded.
/// Copy limits apply only to observation and never alter native configuration.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Configuration {
  pub interface: Option<String>,
  pub ipv4: Option<String>,
  pub netmask: Option<String>,
  pub ipv6: Option<String>,
  pub netmask6: Option<String>,
  pub server_ip: Option<String>,
  pub dns: NativeList,
  pub search_domains: NativeList,
  pub split_dns: NativeList,
  pub included_routes: NativeList,
  pub excluded_routes: NativeList,
  pub mtu: Option<u32>,
  pub truncated: bool,
}

/// Plaintext packets crossing TUN, excluding outer transport overhead. Sent
/// counters measure local TUN reads, not confirmed delivery to the VPN server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Statistics {
  pub received_bytes: u64,
  pub sent_bytes: u64,
  pub received_packets: u64,
  pub sent_packets: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
  Configuration(Option<Configuration>),
  Established,
  Reconnecting,
  Reconnected,
  Transport(Transport),
  Statistics(Statistics),
  Ended,
}
