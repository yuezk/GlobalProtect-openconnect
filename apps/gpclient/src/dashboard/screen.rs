use std::{
  fs::File,
  sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
  },
  time::{Duration, Instant},
};

use anyhow::{Context as _, ensure};
use crossterm::{
  cursor::Show,
  event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
  execute,
  terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use gpapi::session::tunnel::{TunnelList, TunnelPhase, TunnelState, TunnelStatisticsState};
use log::Level;
use nix::unistd::{getpgrp, tcgetpgrp};
use ratatui::{
  Terminal,
  backend::CrosstermBackend,
  layout::{Constraint, Layout},
  style::{Color, Modifier, Style},
  text::{Line, Span},
  widgets::{Block, Borders, Paragraph, Wrap},
};
use tokio_util::sync::CancellationToken;

use super::{
  Command, Content,
  notice::{Notice, NoticeSlot},
  output::{History, plain_text},
  restoration::TerminalRestoration,
};

pub(super) struct Context {
  pub(super) terminal: File,
  pub(super) output: Arc<File>,
  pub(super) saved_stdout: Option<Arc<File>>,
  pub(super) saved_stdin_flags: nix::fcntl::FdFlag,
  pub(super) history: Arc<Mutex<History>>,
  pub(super) passthrough: Arc<AtomicBool>,
  pub(super) gate: Arc<Mutex<()>>,
  pub(super) stop: Arc<AtomicBool>,
  pub(super) cancellation: CancellationToken,
  pub(super) notice: NoticeSlot,
  pub(super) server: String,
  pub(super) content: Content,
  pub(super) commands: mpsc::Receiver<Command>,
}

struct Screen {
  terminal: Terminal<CrosstermBackend<File>>,
  restore: Arc<TerminalRestoration>,
  controlling_terminal: File,
  passthrough: Arc<AtomicBool>,
  gate: Arc<Mutex<()>>,
  active: bool,
}

impl Screen {
  fn new(context: &Context) -> anyhow::Result<Self> {
    let restore = TerminalRestoration::install(
      context.terminal.try_clone()?,
      context.output.clone(),
      context.saved_stdout.clone(),
      context.saved_stdin_flags,
    )?;
    let terminal = Terminal::new(CrosstermBackend::new(context.output.try_clone()?))?;
    let mut screen = Self {
      terminal,
      restore,
      controlling_terminal: context.terminal.try_clone()?,
      passthrough: context.passthrough.clone(),
      gate: context.gate.clone(),
      active: false,
    };
    screen.enter()?;
    Ok(screen)
  }

  fn enter(&mut self) -> anyhow::Result<()> {
    if self.active {
      return Ok(());
    }
    ensure!(
      tcgetpgrp(&self.controlling_terminal)? == getpgrp(),
      "Dashboard is no longer in the foreground"
    );
    let _gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
    self.passthrough.store(false, Ordering::Release);
    // Arm before changing termios, including partial initialization failures.
    self.restore.arm();
    self.active = true;
    enable_raw_mode()?;
    execute!(self.terminal.backend_mut(), EnterAlternateScreen)?;
    // Terminal::clear queries cursor position through process stdout. Fullscreen
    // resize clears and resets buffers through our saved terminal backend only.
    self.terminal.resize(self.terminal.size()?.into())?;
    Ok(())
  }

  fn leave(&mut self) {
    if !self.active {
      return;
    }
    let _gate = self.gate.lock().unwrap_or_else(|error| error.into_inner());
    let _ = disable_raw_mode();
    let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
    self.restore.disarm();
    self.passthrough.store(true, Ordering::Release);
    self.active = false;
  }

  fn suspend(&mut self) {
    self.leave();
    // SIGSTOP avoids recursion through our SIGTSTP listener. The shell resumes us.
    unsafe {
      nix::libc::raise(nix::libc::SIGSTOP);
    }
  }
}

impl Drop for Screen {
  fn drop(&mut self) {
    self.leave();
  }
}

pub(super) fn run(context: Context, ready: mpsc::SyncSender<anyhow::Result<()>>) -> anyhow::Result<()> {
  let screen = Screen::new(&context);
  let mut screen = match screen {
    Ok(screen) => {
      let _ = ready.send(Ok(()));
      screen
    }
    Err(error) => {
      let _ = ready.send(Err(anyhow::anyhow!("{error:#}")));
      return Err(error);
    }
  };
  let mut logs = false;
  let mut next_draw = Instant::now();
  let mut dirty = true;
  while !context.stop.load(Ordering::Acquire) {
    for command in context.commands.try_iter() {
      match command {
        Command::Suspend => screen.suspend(),
        Command::Resume => {}
      }
    }
    if !screen.active {
      if tcgetpgrp(&context.terminal)? == getpgrp() {
        screen.enter()?;
      } else {
        std::thread::sleep(Duration::from_millis(100));
        continue;
      }
    }
    if dirty || Instant::now() >= next_draw {
      draw(&mut screen.terminal, &context, logs)?;
      next_draw = Instant::now() + Duration::from_secs(1);
      dirty = false;
    }
    if event::poll(Duration::from_millis(100)).context("Cannot read dashboard input")? {
      if let Event::Key(key) = event::read()? {
        if key.kind != KeyEventKind::Press {
          continue;
        }
        match key.code {
          KeyCode::Tab | KeyCode::BackTab => {
            logs = !logs;
            dirty = true;
          }
          KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            context.cancellation.cancel();
            dirty = true;
          }
          KeyCode::Char('z') if key.modifiers.contains(KeyModifiers::CONTROL) => screen.suspend(),
          _ => {}
        }
      }
    }
  }
  Ok(())
}

fn phase_label(phase: TunnelPhase) -> &'static str {
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

fn list(value: &TunnelList) -> String {
  let content = if value.items.is_empty() {
    "none".into()
  } else {
    value.items.join(", ")
  };
  if value.truncated {
    format!("{content} [truncated; {} total]", value.total_count)
  } else {
    content
  }
}

fn row(label: &str, value: impl Into<String>) -> Line<'static> {
  Line::from(vec![
    Span::styled(format!("{label:<16}  "), Style::default().fg(Color::DarkGray)),
    Span::raw(plain_text(value.into().as_bytes())),
  ])
}

fn tunnel_sections(
  state: &TunnelState,
  statistics: &gpapi::session::tunnel::TunnelStatisticsSnapshot,
) -> (Vec<Line<'static>>, Vec<Line<'static>>) {
  let mut lines = vec![
    row("Status", phase_label(state.phase)),
    row("Transport", state.transport.label()),
    row("Uptime", uptime(state.elapsed_ms)),
    Line::default(),
  ];
  let mut settings = Vec::new();
  if let Some(configuration) = &state.configuration {
    settings.push(row(
      "Interface",
      configuration.interface.clone().unwrap_or_else(|| "none".into()),
    ));
    settings.push(row(
      "Gateway IP",
      configuration.server_ip.clone().unwrap_or_else(|| "none".into()),
    ));
    settings.push(row(
      "Assigned IPv4",
      configuration
        .ipv4
        .as_ref()
        .map(|address| address.display())
        .unwrap_or_else(|| "none".into()),
    ));
    settings.push(row(
      "Assigned IPv6",
      configuration
        .ipv6
        .as_ref()
        .map(|address| address.display())
        .unwrap_or_else(|| "none".into()),
    ));
    settings.push(Line::default());
  } else {
    settings.push(row("Configuration", "unavailable"));
  }
  if let Some(view) = statistics.view() {
    let label = match view.state {
      TunnelStatisticsState::Disabled => "disabled",
      TunnelStatisticsState::Waiting => "waiting for sample",
      TunnelStatisticsState::Fresh => "live",
      TunnelStatisticsState::Stale => "stale",
      TunnelStatisticsState::Ended => "ended",
    };
    lines.push(row("Statistics", label));
    if let Some(sample) = &view.sample {
      lines.push(row("Bytes received", bytes(sample.received_bytes)));
      lines.push(row("Bytes sent", bytes(sample.sent_bytes)));
      lines.push(row("Packets received", sample.received_packets.to_string()));
      lines.push(row("Packets sent", sample.sent_packets.to_string()));
      if view.state != TunnelStatisticsState::Fresh {
        if let Some(age) = view.sample_age_ms {
          lines.push(row("Sample age", format!("{} seconds", age / 1000)));
        }
      }
    }
  } else {
    lines.push(row("Statistics", "unavailable"));
  }
  if let Some(configuration) = &state.configuration {
    settings.push(row("DNS", list(&configuration.dns)));
    settings.push(row("Search domains", list(&configuration.search_domains)));
    settings.push(row("Split DNS", list(&configuration.split_dns)));
    settings.push(row("Included routes", list(&configuration.included_routes)));
    settings.push(row("Excluded routes", list(&configuration.excluded_routes)));
    settings.push(row(
      "MTU",
      configuration
        .mtu
        .map(|mtu| mtu.to_string())
        .unwrap_or_else(|| "none".into()),
    ));
    if configuration.truncated {
      settings.push(row("Configuration", "truncated"));
    }
  }
  (settings, lines)
}

fn draw(terminal: &mut Terminal<CrosstermBackend<File>>, context: &Context, logs: bool) -> anyhow::Result<()> {
  terminal.draw(|frame| {
    let area = frame.area();
    if area.height < 8 || area.width < 24 {
      frame.render_widget(
        Paragraph::new("GP Connect\nCtrl+C Disconnect").wrap(Wrap { trim: false }),
        area,
      );
      return;
    }
    let [header, body, notice, footer] = Layout::vertical([
      Constraint::Length(3),
      Constraint::Min(1),
      Constraint::Length(3),
      Constraint::Length(1),
    ])
    .areas(area);
    let view = if logs { "Live logs" } else { "Dashboard" };
    frame.render_widget(
      Paragraph::new(format!(" GP Connect · {view}"))
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::BOTTOM)),
      header,
    );
    let mut lines = vec![row("Server", context.server.clone()), Line::default()];
    let mut side_panel = None;
    if logs {
      let history = context.history.lock().unwrap_or_else(|error| error.into_inner());
      let tail = history.tail(body.height as usize);
      lines = tail.into_iter().map(Line::raw).collect();
      let skip = lines.len().saturating_sub(body.height as usize);
      lines.drain(..skip);
    } else {
      match &context.content {
        Content::Tunnel(monitor) => {
          let (settings, statistics) = tunnel_sections(&monitor.state(), &monitor.statistics());
          if body.width >= 80 {
            lines.extend(settings);
            side_panel = Some(statistics);
          } else {
            lines.extend(statistics);
            lines.push(Line::default());
            lines.extend(settings);
          }
        }
        Content::Internal(members) => {
          lines.push(row("Status", "Internal network session"));
          lines.push(Line::raw("Tunnel statistics unavailable (non-tunnel session)"));
          lines.push(Line::default());
          lines.extend(
            members
              .lock()
              .unwrap_or_else(|error| error.into_inner())
              .values()
              .map(|member| Line::raw(plain_text(member.as_bytes()))),
          );
        }
      }
    }
    if let Some(statistics) = side_panel {
      let [settings_area, statistics_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)])
          .spacing(2)
          .areas(body);
      frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), settings_area);
      frame.render_widget(Paragraph::new(statistics).wrap(Wrap { trim: false }), statistics_area);
    } else {
      frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), body);
    }
    let current = context.notice.lock().unwrap_or_else(|error| error.into_inner()).clone();
    let current = match &context.content {
      Content::Tunnel(monitor) => {
        let state = monitor.state();
        Some(notice_for_status(current, state.phase, state.transport))
      }
      Content::Internal(_) => current,
    };
    if let Some(current) = current {
      let color = if current.level <= Level::Warn {
        Color::Yellow
      } else {
        Color::Gray
      };
      frame.render_widget(
        Paragraph::new(current.message)
          .style(Style::default().fg(color))
          .wrap(Wrap { trim: false })
          .block(Block::default().title("Latest event").borders(Borders::TOP)),
        notice,
      );
    }
    let ending = if context.cancellation.is_cancelled() {
      "Disconnecting…"
    } else {
      "Ctrl+C Disconnect"
    };
    frame.render_widget(
      Paragraph::new(format!(" Tab Switch view    {ending}")).style(Style::default().fg(Color::DarkGray)),
      footer,
    );
  })?;
  Ok(())
}

fn notice_for_status(
  notice: Option<Notice>,
  phase: TunnelPhase,
  transport: gpapi::session::tunnel::TunnelTransport,
) -> Notice {
  notice.unwrap_or_else(|| Notice {
    level: Level::Info,
    message: format!("{} · {}", phase_label(phase), transport.label()),
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn units_and_long_sessions_are_explicit() {
    assert_eq!(bytes(1024), "1.0 KiB");
    assert_eq!(bytes(0), "0 B");
    assert_eq!(uptime(90_061_000), "25:01:01");
  }

  #[test]
  fn status_transitions_cannot_overwrite_an_existing_warning() {
    use gpapi::session::tunnel::TunnelTransport;
    let warning = Notice {
      level: Level::Warn,
      message: "ESP unavailable; using SSL".into(),
    };
    let first = notice_for_status(Some(warning), TunnelPhase::Connected, TunnelTransport::SslTls);
    let reconnecting = notice_for_status(Some(first), TunnelPhase::Reconnecting, TunnelTransport::Unavailable);
    let resumed = notice_for_status(Some(reconnecting), TunnelPhase::Connected, TunnelTransport::IpsecEsp);
    assert_eq!(resumed.level, Level::Warn);
    assert_eq!(resumed.message, "ESP unavailable; using SSL");
  }
}
