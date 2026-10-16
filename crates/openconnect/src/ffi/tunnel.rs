use std::{
  ffi::{CStr, c_char, c_int, c_void},
  panic::{AssertUnwindSafe, catch_unwind},
};

use crate::{
  Vpn,
  tunnel::{Configuration, Event, NativeList, Statistics, Transport},
};

const TEXT_LIMIT: usize = 1024;
const COPY_BUDGET: usize = 16 * 1024;
const LIST_LIMIT: usize = 256;

#[repr(C)]
struct SplitInclude {
  route: *const c_char,
  next: *const SplitInclude,
}

/// Matches oc_ip_info in the bundled public openconnect.h.
#[repr(C)]
struct IpInfo {
  addr: *const c_char,
  netmask: *const c_char,
  addr6: *const c_char,
  netmask6: *const c_char,
  dns: [*const c_char; 3],
  nbns: [*const c_char; 3],
  domain: *const c_char,
  proxy_pac: *const c_char,
  mtu: c_int,
  split_dns: *const SplitInclude,
  split_includes: *const SplitInclude,
  split_excludes: *const SplitInclude,
  gateway_addr: *const c_char,
}

#[repr(C)]
struct Stats {
  tx_pkts: u64,
  tx_bytes: u64,
  rx_pkts: u64,
  rx_bytes: u64,
}

struct CopyBudget {
  remaining: usize,
  truncated: bool,
}

impl CopyBudget {
  fn new() -> Self {
    Self {
      remaining: COPY_BUDGET,
      truncated: false,
    }
  }

  fn text(&mut self, bytes: &[u8]) -> String {
    let limit = self.remaining.min(TEXT_LIMIT).min(bytes.len());
    // Lossy UTF-8 is bounded before allocation, then truncated at a character
    // boundary so replacement characters cannot exceed the retained budget.
    let mut text = String::from_utf8_lossy(&bytes[..limit]).into_owned();
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
      end -= 1;
    }
    text.truncate(end);
    self.truncated |= end < bytes.len();
    self.remaining -= text.len();
    text
  }

  unsafe fn optional(&mut self, value: *const c_char) -> Option<String> {
    if value.is_null() {
      return None;
    }
    // The native value is NUL terminated and borrowed only during the callback.
    // Bound the scan as well as the retained allocation for scalar fields.
    let length = unsafe { libc::strnlen(value, TEXT_LIMIT + 1) };
    Some(self.text(unsafe { std::slice::from_raw_parts(value.cast(), length) }))
  }

  fn push(&mut self, list: &mut NativeList, bytes: &[u8]) {
    list.total_count = list.total_count.saturating_add(1);
    // Retain whole list entries; the native list is not a user-owned buffer.
    if list.items.len() >= LIST_LIMIT || bytes.len() > TEXT_LIMIT || bytes.len() > self.remaining {
      list.truncated = true;
      self.truncated = true;
      return;
    }
    let text = self.text(bytes);
    list.truncated |= text.len() != bytes.len();
    list.items.push(text);
  }

  unsafe fn routes(&mut self, mut node: *const SplitInclude) -> NativeList {
    let mut list = NativeList::default();
    while !node.is_null() {
      let entry = unsafe { &*node };
      if !entry.route.is_null() {
        self.push(&mut list, unsafe { CStr::from_ptr(entry.route) }.to_bytes());
      }
      node = entry.next;
    }
    list
  }
}

unsafe fn configuration(interface: *const c_char, raw: &IpInfo) -> Configuration {
  let mut budget = CopyBudget::new();
  let mut result = Configuration {
    interface: unsafe { budget.optional(interface) },
    ipv4: unsafe { budget.optional(raw.addr) },
    netmask: unsafe { budget.optional(raw.netmask) },
    ipv6: unsafe { budget.optional(raw.addr6) },
    netmask6: unsafe { budget.optional(raw.netmask6) },
    server_ip: unsafe { budget.optional(raw.gateway_addr) },
    mtu: u32::try_from(raw.mtu).ok().filter(|mtu| *mtu > 0),
    ..Configuration::default()
  };
  for server in raw.dns {
    if !server.is_null() {
      budget.push(&mut result.dns, unsafe { CStr::from_ptr(server) }.to_bytes());
    }
  }
  if !raw.domain.is_null() {
    // GlobalProtect builds its search list with spaces in gpst.c.
    for domain in unsafe { CStr::from_ptr(raw.domain) }
      .to_bytes()
      .split(u8::is_ascii_whitespace)
    {
      if !domain.is_empty() {
        budget.push(&mut result.search_domains, domain);
      }
    }
  }
  result.split_dns = unsafe { budget.routes(raw.split_dns) };
  result.included_routes = unsafe { budget.routes(raw.split_includes) };
  result.excluded_routes = unsafe { budget.routes(raw.split_excludes) };
  result.truncated = budget.truncated;
  result
}

fn observe(vpn: *mut c_void, action: impl FnOnce(&Vpn)) {
  if vpn.is_null() {
    return;
  }
  // All exports are called synchronously with the live Vpn registered by connect.
  if catch_unwind(AssertUnwindSafe(|| action(unsafe { &*vpn.cast::<Vpn>() }))).is_err() {
    log::warn!("Tunnel observer panicked");
  }
}

#[unsafe(no_mangle)]
extern "C" fn vpn_tunnel_configuration(vpn: *mut c_void, interface: *const c_char, info: *const IpInfo) {
  observe(vpn, |vpn| {
    let configuration = unsafe { info.as_ref().map(|info| configuration(interface, info)) };
    vpn.on_tunnel_event(Event::Configuration(configuration));
  });
}

#[unsafe(no_mangle)]
extern "C" fn vpn_tunnel_event(vpn: *mut c_void, event: c_int) {
  observe(vpn, |vpn| match event {
    0 => vpn.on_tunnel_event(Event::Transport(Transport::Negotiating)),
    1 => vpn.on_tunnel_event(Event::Transport(Transport::IpsecEsp)),
    2 => vpn.on_tunnel_event(Event::Transport(Transport::SslTls)),
    3 => vpn.on_tunnel_event(Event::Transport(Transport::Unavailable)),
    4 => {
      vpn.on_tunnel_event(Event::Transport(Transport::Unavailable));
      vpn.on_tunnel_event(Event::Reconnecting);
    }
    5 => vpn.on_tunnel_event(Event::Established),
    6 => vpn.on_tunnel_event(Event::Reconnected),
    7 => {
      vpn.on_tunnel_event(Event::Transport(Transport::Unavailable));
      vpn.on_tunnel_event(Event::Ended);
    }
    _ => log::warn!("Unknown native tunnel observation: {event}"),
  });
}

#[unsafe(no_mangle)]
extern "C" fn vpn_tunnel_statistics(vpn: *mut c_void, stats: *const Stats) {
  observe(vpn, |vpn| {
    vpn.statistics_received();
    if let Some(stats) = unsafe { stats.as_ref() } {
      vpn.on_tunnel_event(Event::Statistics(Statistics {
        received_bytes: stats.rx_bytes,
        sent_bytes: stats.tx_bytes,
        received_packets: stats.rx_pkts,
        sent_packets: stats.tx_pkts,
      }));
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::{
    ffi::CString,
    sync::{Arc, Mutex},
  };

  fn empty_info() -> IpInfo {
    // Every pointer is nullable, and zero MTU denotes unavailable.
    unsafe { std::mem::zeroed() }
  }

  #[test]
  fn snapshot_owns_configuration_and_preserves_native_mask_semantics() {
    let domain = CString::new("one.example two.example\tthree.example").unwrap();
    let ipv6 = CString::new("2001:db8::1").unwrap();
    let mask6 = CString::new("2001:db8::1/64").unwrap();
    let server = CString::new("2001:db8::53").unwrap();
    let route = CString::new("10.0.0.0/8").unwrap();
    let node = SplitInclude {
      route: route.as_ptr(),
      next: std::ptr::null(),
    };
    let raw = IpInfo {
      domain: domain.as_ptr(),
      addr6: ipv6.as_ptr(),
      netmask6: mask6.as_ptr(),
      dns: [server.as_ptr(), std::ptr::null(), std::ptr::null()],
      split_includes: &node,
      mtu: 1400,
      ..empty_info()
    };
    let snapshot = unsafe { configuration(std::ptr::null(), &raw) };
    drop((domain, ipv6, mask6, server, route));
    assert_eq!(
      snapshot.search_domains.items,
      ["one.example", "two.example", "three.example"]
    );
    assert_eq!(snapshot.dns.items, ["2001:db8::53"]);
    assert_eq!(snapshot.netmask6.as_deref(), Some("2001:db8::1/64"));
    assert_eq!(snapshot.included_routes.items, ["10.0.0.0/8"]);
    assert_eq!(snapshot.mtu, Some(1400));
    assert!(!snapshot.truncated);
  }

  #[test]
  fn observation_budget_keeps_whole_entries_and_counts_omitted_values() {
    let domain = CString::new(
      std::iter::repeat_n("example.internal", 2000)
        .collect::<Vec<_>>()
        .join(" "),
    )
    .unwrap();
    let long_ipv6 = CString::new("界".repeat(500)).unwrap();
    let raw = IpInfo {
      domain: domain.as_ptr(),
      addr6: long_ipv6.as_ptr(),
      ..empty_info()
    };
    let snapshot = unsafe { configuration(std::ptr::null(), &raw) };
    assert!(snapshot.truncated);
    assert!(snapshot.ipv6.as_ref().unwrap().len() <= TEXT_LIMIT);
    assert_eq!(snapshot.search_domains.total_count, 2000);
    assert!(snapshot.search_domains.truncated);
    assert!(snapshot.search_domains.items.len() <= LIST_LIMIT);
    assert!(
      snapshot
        .search_domains
        .items
        .iter()
        .all(|item| item == "example.internal")
    );
    let retained = snapshot.ipv6.unwrap().len() + snapshot.search_domains.items.iter().map(String::len).sum::<usize>();
    assert!(retained <= COPY_BUDGET);
  }

  #[test]
  fn observer_deduplicates_transport_and_survives_panics() {
    let vpn = Vpn::builder("vpn.example.com", "cookie")
      .script_path("/bin/sh".to_owned())
      .build()
      .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let collected = events.clone();
    vpn.set_tunnel_observer(move |event| collected.lock().unwrap().push(event));
    let pointer = (&vpn as *const Vpn).cast_mut().cast();
    vpn_tunnel_event(pointer, 1);
    vpn_tunnel_event(pointer, 1);
    vpn_tunnel_event(pointer, 4);
    vpn_tunnel_event(pointer, 6);
    vpn_tunnel_event(pointer, 7);
    assert_eq!(
      *events.lock().unwrap(),
      [
        Event::Transport(Transport::IpsecEsp),
        Event::Transport(Transport::Unavailable),
        Event::Reconnecting,
        Event::Reconnected,
        Event::Ended,
      ]
    );
    vpn.set_tunnel_observer(|_| panic!("test observer panic"));
    vpn_tunnel_event(pointer, 5);
    vpn.clear_tunnel_observer();
    vpn_tunnel_event(pointer, 5);
  }

  #[test]
  fn statistics_callback_clears_pending_before_notifying_observer() {
    use std::{
      io::Read,
      os::{fd::AsRawFd, unix::net::UnixStream},
    };
    let vpn = Arc::new(
      Vpn::builder("vpn.example.com", "cookie")
        .script_path("/bin/sh".to_owned())
        .build()
        .unwrap(),
    );
    let (mut reader, writer) = UnixStream::pair().unwrap();
    reader.set_nonblocking(true).unwrap();
    assert!(!vpn.attach_command_pipe(writer.as_raw_fd()));
    assert!(vpn.request_statistics().unwrap());
    let received = Arc::new(Mutex::new(None));
    let captured = received.clone();
    let weak = Arc::downgrade(&vpn);
    vpn.set_tunnel_observer(move |event| {
      if let Event::Statistics(statistics) = event {
        *captured.lock().unwrap() = Some(statistics);
        assert!(weak.upgrade().unwrap().request_statistics().unwrap());
      }
    });
    let raw = Stats {
      tx_pkts: u64::MAX,
      tx_bytes: 1 << 54,
      rx_pkts: 12,
      rx_bytes: 34,
    };
    vpn_tunnel_statistics(Arc::as_ptr(&vpn).cast_mut().cast(), &raw);
    assert_eq!(
      *received.lock().unwrap(),
      Some(Statistics {
        sent_packets: u64::MAX,
        sent_bytes: 1 << 54,
        received_packets: 12,
        received_bytes: 34,
      })
    );
    let mut commands = [0; 2];
    reader.read_exact(&mut commands).unwrap();
    assert_eq!(commands, [b's', b's']);
    vpn.detach_command_pipe();
    vpn.clear_tunnel_observer();
  }
}
