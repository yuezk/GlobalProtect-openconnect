//! Dashboard presentation only; connection ownership and sampling stay outside this module.
use crossterm::event::KeyCode;
use gpapi::session::tunnel::{TunnelList, TunnelPhase, TunnelState, TunnelStatisticsSnapshot, TunnelStatisticsState};
use ratatui::{
  Frame,
  buffer::Buffer,
  layout::Rect,
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Paragraph, Widget, Wrap},
};

use super::output::plain_text;

const MAX_CONTENT_WIDTH: u16 = 120;
const LABEL_WIDTH: usize = 12;

#[derive(Default)]
pub(super) struct DetailScroll {
  offset: u16,
  maximum: u16,
  page: u16,
}

impl DetailScroll {
  pub(super) fn handle_key(&mut self, key: KeyCode) {
    self.offset = match key {
      KeyCode::Up => self.offset.saturating_sub(1),
      KeyCode::Down => self.offset.saturating_add(1),
      KeyCode::PageUp => self.offset.saturating_sub(self.page),
      KeyCode::PageDown => self.offset.saturating_add(self.page),
      KeyCode::Home => 0,
      KeyCode::End => self.maximum,
      _ => self.offset,
    }
    .min(self.maximum);
  }

  pub(super) fn can_scroll(&self) -> bool {
    self.maximum > 0
  }

  fn update(&mut self, content_height: u16, viewport_height: u16) {
    self.maximum = if viewport_height == 0 {
      0
    } else {
      content_height.saturating_sub(viewport_height)
    };
    self.offset = self.offset.min(self.maximum);
    self.page = viewport_height.saturating_sub(1).max(1);
  }
}

pub(super) fn phase_label(phase: TunnelPhase) -> &'static str {
  match phase {
    TunnelPhase::Establishing => "Establishing",
    TunnelPhase::Connected => "Connected",
    TunnelPhase::Reconnecting => "Reconnecting",
    TunnelPhase::Ended => "Disconnected",
  }
}

fn uptime(milliseconds: u64) -> String {
  let seconds = milliseconds / 1000;
  format!("{:02}:{:02}:{:02}", seconds / 3600, (seconds / 60) % 60, seconds % 60)
}

fn bytes(value: u64) -> String {
  const UNITS: [&str; 7] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
  let mut amount = value as f64;
  let mut unit = 0;
  while amount >= 1024.0 && unit < UNITS.len() - 1 {
    amount /= 1024.0;
    unit += 1;
  }
  if unit == 0 {
    format!("{value} B")
  } else {
    format!("{amount:.1} {}", UNITS[unit])
  }
}

fn muted(text: impl Into<String>) -> Span<'static> {
  Span::styled(text.into(), Style::default().fg(Color::DarkGray))
}

fn heading(text: impl Into<String>) -> Line<'static> {
  Line::styled(
    text.into(),
    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
  )
}

pub(super) fn row(label: &str, value: impl Into<String>) -> Line<'static> {
  Line::from(vec![
    muted(format!("{label:<LABEL_WIDTH$}  ")),
    Span::raw(plain_text(value.into().as_bytes())),
  ])
}

fn paragraph(lines: Vec<Line<'static>>) -> Paragraph<'static> {
  Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn summary(server: &str, state: &TunnelState, width: u16) -> Paragraph<'static> {
  let status = Span::styled(
    format!("{:<LABEL_WIDTH$}  ", phase_label(state.phase)),
    Style::default().fg(if state.phase == TunnelPhase::Connected {
      Color::Green
    } else {
      Color::Yellow
    }),
  );
  let mut lines = if width >= 64 {
    vec![Line::from(vec![
      status,
      Span::raw(format!(
        "{}    Uptime {}",
        state.transport.label(),
        uptime(state.elapsed_ms)
      )),
    ])]
  } else {
    vec![
      Line::from(status),
      row("Transport", state.transport.label()),
      row("Uptime", uptime(state.elapsed_ms)),
    ]
  };
  lines.insert(0, row("Server", server));
  paragraph(lines)
}

fn traffic(statistics: &TunnelStatisticsSnapshot, width: u16) -> Paragraph<'static> {
  let mut lines = vec![heading("TRAFFIC")];
  if let Some(view) = statistics.view() {
    let label = match view.state {
      TunnelStatisticsState::Disabled => "disabled",
      TunnelStatisticsState::Waiting => "waiting for sample",
      TunnelStatisticsState::Fresh => "live",
      TunnelStatisticsState::Stale => "stale",
      TunnelStatisticsState::Ended => "ended",
    };
    lines[0] = heading(format!("TRAFFIC · {label}"));
    if let Some(sample) = &view.sample {
      if width >= 56 {
        let rows = [
          ("Bytes", bytes(sample.received_bytes), bytes(sample.sent_bytes)),
          (
            "Packets",
            sample.received_packets.to_string(),
            sample.sent_packets.to_string(),
          ),
        ];
        // Size to the values, while keeping room for full u64 counters and a
        // visible gap even when both numeric columns are filled.
        let numeric_width = rows
          .iter()
          .flat_map(|(_, received, sent)| [received.len(), sent.len()])
          .max()
          .unwrap_or(0)
          .max(10);
        lines.push(Line::from(muted(format!(
          "{:<10}{:>numeric_width$}    {:>numeric_width$}",
          "", "Received", "Sent"
        ))));
        for (label, received, sent) in rows {
          lines.push(Line::from(vec![
            muted(format!("{label:<10}")),
            Span::raw(format!("{received:>numeric_width$}    {sent:>numeric_width$}")),
          ]));
        }
      } else {
        lines.push(row("Received", bytes(sample.received_bytes)));
        lines.push(row("Sent", bytes(sample.sent_bytes)));
        lines.push(row("Packets in", sample.received_packets.to_string()));
        lines.push(row("Packets out", sample.sent_packets.to_string()));
      }
      if view.state != TunnelStatisticsState::Fresh {
        if let Some(age) = view.sample_age_ms {
          lines.push(row("Sample age", format!("{} seconds", age / 1000)));
        }
      }
    }
  } else {
    lines.push(row("Statistics", "unavailable"));
  }
  paragraph(lines)
}

fn list_rows(lines: &mut Vec<Line<'static>>, label: &str, value: &TunnelList) {
  if value.items.is_empty() && !value.truncated {
    lines.push(row(label, "none"));
  } else {
    for (index, item) in value.items.iter().enumerate() {
      lines.push(row(if index == 0 { label } else { "" }, item));
    }
    if value.truncated {
      lines.push(row("", format!("[truncated; {} total]", value.total_count)));
    }
  }
}

fn routes(lines: &mut Vec<Line<'static>>, label: &str, list: &TunnelList, width: u16) {
  lines.push(row(label, list.total_count.to_string()));
  // Only whole entries move between columns. Long individual entries wrap through
  // Paragraph rather than disappearing off the edge of a narrow terminal.
  let entries: Vec<_> = list.items.iter().map(|item| plain_text(item.as_bytes())).collect();
  let cell_width = entries.iter().map(|item| Line::raw(item).width()).max().unwrap_or(0) + 3;
  let columns = (width as usize / cell_width).clamp(1, 3);
  for chunk in entries.chunks(columns) {
    let mut spans = Vec::new();
    for (index, entry) in chunk.iter().enumerate() {
      spans.push(Span::raw(entry.clone()));
      if index + 1 < chunk.len() {
        spans.push(Span::raw(" ".repeat(cell_width - Line::raw(entry).width())));
      }
    }
    lines.push(Line::from(spans));
  }
  if list.truncated {
    lines.push(Line::raw(format!(
      "[truncated; showing {} of {}]",
      list.items.len(),
      list.total_count
    )));
  }
}

fn details(state: &TunnelState, statistics: &TunnelStatisticsSnapshot, width: u16) -> Buffer {
  let traffic = traffic(statistics, width);
  let traffic_height = traffic.line_count(width) as u16;
  let mut tunnel = vec![heading("TUNNEL")];
  let mut dns = vec![heading("DNS")];
  let mut route_lines = vec![heading("ROUTES")];
  if let Some(config) = &state.configuration {
    tunnel.extend([
      row("Interface", config.interface.as_deref().unwrap_or("none")),
      row(
        "IPv4",
        config
          .ipv4
          .as_ref()
          .map(|address| address.display())
          .unwrap_or_else(|| "none".into()),
      ),
      row(
        "IPv6",
        config
          .ipv6
          .as_ref()
          .map(|address| address.display())
          .unwrap_or_else(|| "none".into()),
      ),
      row("Gateway", config.server_ip.as_deref().unwrap_or("none")),
      row(
        "MTU",
        config.mtu.map(|mtu| mtu.to_string()).unwrap_or_else(|| "none".into()),
      ),
    ]);
    list_rows(&mut dns, "Servers", &config.dns);
    list_rows(&mut dns, "Domains", &config.search_domains);
    list_rows(&mut dns, "Split DNS", &config.split_dns);
    routes(&mut route_lines, "Included", &config.included_routes, width);
    routes(&mut route_lines, "Excluded", &config.excluded_routes, width);
    if config.truncated {
      route_lines.push(row("Configuration", "truncated"));
    }
  } else {
    tunnel.push(row("Configuration", "unavailable"));
  }
  let tunnel = paragraph(tunnel);
  let dns = paragraph(dns);
  let routes = paragraph(route_lines);
  let columns = width >= 88;
  let column_width = if columns { (width - 4) / 2 } else { width };
  let tunnel_height = tunnel.line_count(column_width) as u16;
  let dns_height = dns.line_count(column_width) as u16;
  let network_height = if columns {
    tunnel_height.max(dns_height)
  } else {
    tunnel_height + 1 + dns_height
  };
  let network_y = traffic_height + 1;
  let routes_y = network_y + network_height + 1;
  let height = routes_y + routes.line_count(width) as u16;
  let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
  traffic.render(Rect::new(0, 0, width, traffic_height), &mut buffer);
  tunnel.render(Rect::new(0, network_y, column_width, tunnel_height), &mut buffer);
  let (dns_x, dns_y) = if columns {
    (column_width + 4, network_y)
  } else {
    (0, network_y + tunnel_height + 1)
  };
  dns.render(Rect::new(dns_x, dns_y, column_width, dns_height), &mut buffer);
  routes.render(Rect::new(0, routes_y, width, height - routes_y), &mut buffer);
  buffer
}

pub(super) fn render_tunnel(
  frame: &mut Frame,
  area: Rect,
  server: &str,
  state: &TunnelState,
  statistics: &TunnelStatisticsSnapshot,
  scroll: &mut DetailScroll,
) {
  if area.is_empty() {
    return;
  }
  let area = Rect {
    width: area.width.min(MAX_CONTENT_WIDTH),
    ..area
  };
  let full_summary = summary(server, state, area.width);
  let summary = if full_summary.line_count(area.width) + 2 > area.height as usize {
    Paragraph::new(phase_label(state.phase))
  } else {
    full_summary
  };
  let summary_height = (summary.line_count(area.width) as u16).min(area.height);
  frame.render_widget(summary, Rect::new(area.x, area.y, area.width, summary_height));
  let body_y = summary_height.saturating_add(1).min(area.height);
  let viewport = Rect::new(area.x, area.y + body_y, area.width, area.height - body_y);
  let buffer = details(state, statistics, area.width);
  scroll.update(buffer.area.height, viewport.height);
  // Render the full detail layout with Ratatui, then expose just the scrolled rows.
  // This preserves wrapped fields and aligned columns through terminal resizes.
  for y in 0..viewport.height {
    if y + scroll.offset >= buffer.area.height {
      break;
    }
    for x in 0..viewport.width {
      frame.buffer_mut()[(viewport.x + x, viewport.y + y)] = buffer[(x, y + scroll.offset)].clone();
    }
  }
}

#[cfg(test)]
mod tests {
  use gpapi::session::tunnel::{TunnelStatistics, TunnelStatisticsView, TunnelTransport};
  use ratatui::{Terminal, backend::TestBackend};

  use super::*;

  fn state() -> TunnelState {
    serde_json::from_value(serde_json::json!({
      "tunnelId": "00000000-0000-0000-0000-000000000001",
      "sequence": "1", "phase": "connected", "configurationRevision": "1",
      "transport": "ipsecEsp", "elapsedMs": "177000",
      "configuration": {
        "interface": "tun0", "serverIp": "220.191.185.154",
        "ipv4": {"address": "10.197.121.152", "prefixLength": 32, "netmask": null},
        "ipv6": null, "mtu": 1439, "truncated": false,
        "dns": {"items": ["10.197.82.140", "10.15.70.11"], "totalCount": 2, "truncated": false},
        "searchDomains": {"items": ["corp.microstrategy.com"], "totalCount": 1, "truncated": false},
        "splitDns": {"items": [], "totalCount": 0, "truncated": false},
        "includedRoutes": {"items": ["10.15.70.11/32", "10.197.82.140/32", "185.199.108.0/22", "2001:db8:1234:5678::/64"], "totalCount": 5, "truncated": true},
        "excludedRoutes": {"items": ["192.168.0.0/16"], "totalCount": 1, "truncated": false}
      }
    })).unwrap()
  }

  fn statistics(state: &TunnelState) -> TunnelStatisticsSnapshot {
    TunnelStatisticsSnapshot::Available(TunnelStatisticsView {
      sequence: 1,
      tunnel_id: state.tunnel_id,
      state_sequence: 1,
      phase: state.phase,
      transport: TunnelTransport::IpsecEsp,
      state: TunnelStatisticsState::Fresh,
      elapsed_ms: state.elapsed_ms,
      sample_age_ms: Some(0),
      sample: Some(TunnelStatistics {
        tunnel_id: state.tunnel_id,
        sequence: 1,
        state_sequence: 1,
        configuration_revision: 1,
        elapsed_ms: state.elapsed_ms,
        received_bytes: 1_258_291,
        sent_bytes: 71_578,
        received_packets: u64::MAX,
        sent_packets: 702,
      }),
    })
  }

  fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
      .map(|y| {
        (0..buffer.area.width)
          .map(|x| buffer[(x, y)].symbol())
          .collect::<String>()
          .trim_end()
          .to_string()
      })
      .collect::<Vec<_>>()
      .join("\n")
  }

  #[test]
  fn server_and_transport_share_a_column_through_status_changes() {
    let mut state = state();
    for width in [40, 64, 120] {
      for phase in [
        TunnelPhase::Establishing,
        TunnelPhase::Connected,
        TunnelPhase::Reconnecting,
        TunnelPhase::Ended,
      ] {
        state.phase = phase;
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, 8));
        summary("vpn.example.com", &state, width).render(buffer.area, &mut buffer);
        let rendered = text(&buffer);
        let server = rendered.lines().find(|line| line.contains("vpn.example.com")).unwrap();
        let transport = rendered.lines().find(|line| line.contains("IPsec (ESP)")).unwrap();
        assert_eq!(server.find("vpn.example.com"), Some(14));
        assert_eq!(
          transport.find("IPsec (ESP)"),
          server.find("vpn.example.com"),
          "{phase:?} at width {width}"
        );
        assert!(rendered.lines().any(|line| line.starts_with(phase_label(phase))));
      }
    }
  }

  #[test]
  fn wide_layout_groups_settings_and_keeps_routes_and_counters_intact() {
    let state = state();
    let buffer = details(&state, &statistics(&state), 100);
    let text = text(&buffer);
    let headings = text.lines().find(|line| line.starts_with("TUNNEL")).unwrap();
    assert!(headings.contains("DNS"));
    assert!(text.contains("18446744073709551615"));
    assert!(text.contains("1.2 MiB"));
    assert!(text.contains("69.9 KiB"));
    assert!(text.contains("2001:db8:1234:5678::/64"));
    assert!(text.contains("[truncated; showing 4 of 5]"));
    assert!(text.contains("192.168.0.0/16"));
    let received_column = text
      .lines()
      .find(|line| line.contains("Received"))
      .unwrap()
      .find("Received")
      .unwrap()
      + "Received".len();
    let bytes_column = text
      .lines()
      .find(|line| line.starts_with("Bytes"))
      .unwrap()
      .find("1.2 MiB")
      .unwrap()
      + "1.2 MiB".len();
    assert_eq!(received_column, bytes_column);
  }

  #[test]
  fn traffic_columns_stay_compact_with_a_visible_gap() {
    let state = state();
    let mut stats = statistics(&state);
    let TunnelStatisticsSnapshot::Available(view) = &mut stats else {
      unreachable!()
    };
    view.sample.as_mut().unwrap().received_packets = 6518;
    let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 4));
    traffic(&stats, 120).render(buffer.area, &mut buffer);
    let rendered = text(&buffer);
    for line in rendered.lines() {
      assert!(
        line.len() <= 40,
        "traffic row should not expand with the terminal: {line}"
      );
    }
    let TunnelStatisticsSnapshot::Available(view) = &mut stats else {
      unreachable!()
    };
    view.sample.as_mut().unwrap().received_packets = u64::MAX;
    view.sample.as_mut().unwrap().sent_packets = u64::MAX;
    let mut buffer = Buffer::empty(Rect::new(0, 0, 56, 4));
    traffic(&stats, 56).render(buffer.area, &mut buffer);
    assert!(text(&buffer).contains("18446744073709551615    18446744073709551615"));
  }

  #[test]
  fn wide_terminal_keeps_content_left_aligned_and_within_120_columns() {
    let state = state();
    let stats = statistics(&state);
    let mut terminal = Terminal::new(TestBackend::new(220, 40)).unwrap();
    let mut scroll = DetailScroll::default();
    terminal
      .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &stats, &mut scroll))
      .unwrap();
    let wide = terminal.backend().buffer().clone();
    terminal.backend_mut().resize(120, 40);
    terminal
      .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &stats, &mut scroll))
      .unwrap();
    for y in 0..40 {
      for x in 0..220 {
        if x < 120 {
          assert_eq!(wide[(x, y)], terminal.backend().buffer()[(x, y)]);
        } else {
          assert_eq!(wide[(x, y)].symbol(), " ");
        }
      }
    }
  }

  #[test]
  fn routes_use_at_most_three_columns_and_keep_every_entry() {
    let list = TunnelList {
      items: (1..=7).map(|n| format!("10.0.{n}.0/24")).collect(),
      total_count: 7,
      truncated: false,
    };
    let mut lines = Vec::new();
    routes(&mut lines, "Included", &list, 220);
    let mut buffer = Buffer::empty(Rect::new(0, 0, 220, 4));
    paragraph(lines).render(buffer.area, &mut buffer);
    let rendered = text(&buffer);
    let route_rows: Vec<_> = rendered.lines().skip(1).collect();
    assert_eq!(route_rows.len(), 3);
    for line in route_rows {
      assert!(line.split_whitespace().count() <= 3);
    }
    for entry in &list.items {
      assert!(rendered.contains(entry));
    }
  }

  #[test]
  fn narrow_layout_stacks_sections_without_losing_routes() {
    let state = state();
    for width in [24, 60, 87] {
      let text = text(&details(&state, &statistics(&state), width));
      assert!(text.lines().any(|line| line == "TUNNEL"));
      assert!(text.lines().any(|line| line == "DNS"));
      assert!(text.contains("2001:db8:1234:5678::/64"));
      assert!(text.contains("192.168.0.0/16"));
    }
  }

  #[test]
  fn scrolling_reaches_last_route_and_resize_clamps_offset_with_summary_pinned() {
    let state = state();
    let statistics = statistics(&state);
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
    let mut scroll = DetailScroll::default();
    terminal
      .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &statistics, &mut scroll))
      .unwrap();
    assert!(scroll.can_scroll());
    scroll.handle_key(KeyCode::End);
    terminal
      .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &statistics, &mut scroll))
      .unwrap();
    let rendered = text(terminal.backend().buffer());
    assert!(rendered.contains("Connected"));
    assert!(rendered.contains("vpn.example.com"));
    assert!(rendered.contains("192.168.0.0/16"));
    terminal.backend_mut().resize(100, 50);
    terminal
      .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &statistics, &mut scroll))
      .unwrap();
    assert!(!scroll.can_scroll());
    assert_eq!(scroll.offset, 0);
    assert!(text(terminal.backend().buffer()).contains("TRAFFIC · live"));
  }

  #[test]
  fn very_short_body_keeps_status_and_scrollable_details_visible() {
    let state = state();
    for (width, height) in [(60, 6), (24, 3)] {
      let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
      let mut scroll = DetailScroll::default();
      let stats = statistics(&state);
      terminal
        .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &stats, &mut scroll))
        .unwrap();
      assert!(text(terminal.backend().buffer()).contains("Connected"));
      assert!(text(terminal.backend().buffer()).contains("TRAFFIC"));
      scroll.handle_key(KeyCode::End);
      terminal
        .draw(|frame| render_tunnel(frame, frame.area(), "vpn.example.com", &state, &stats, &mut scroll))
        .unwrap();
      assert!(text(terminal.backend().buffer()).contains("192.168.0.0/16"));
      scroll.handle_key(KeyCode::PageUp);
      let page_up = scroll.offset;
      assert!(page_up < scroll.maximum);
      scroll.handle_key(KeyCode::Up);
      assert!(scroll.offset < page_up);
      scroll.handle_key(KeyCode::Down);
      assert_eq!(scroll.offset, page_up);
      scroll.handle_key(KeyCode::PageDown);
      assert_eq!(scroll.offset, scroll.maximum);
      scroll.handle_key(KeyCode::Home);
      assert_eq!(scroll.offset, 0);
    }
  }

  #[test]
  fn stale_and_missing_configuration_remain_explicit() {
    let mut state = state();
    let mut statistics = statistics(&state);
    let TunnelStatisticsSnapshot::Available(view) = &mut statistics else {
      unreachable!()
    };
    view.state = TunnelStatisticsState::Stale;
    view.sample_age_ms = Some(12_000);
    state.configuration = None;
    let rendered = text(&details(&state, &statistics, 100));
    assert!(rendered.contains("TRAFFIC · stale"));
    assert!(rendered.contains("12 seconds"));
    assert!(rendered.contains("unavailable"));
    assert!(rendered.contains("1.2 MiB"));
  }

  #[test]
  fn units_and_long_sessions_are_explicit() {
    assert_eq!(bytes(1024), "1.0 KiB");
    assert_eq!(bytes(0), "0 B");
    assert_eq!(uptime(90_061_000), "25:01:01");
  }
}
