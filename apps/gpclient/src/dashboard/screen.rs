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
use gpapi::session::tunnel::TunnelPhase;
use log::Level;
use nix::unistd::{getpgrp, tcgetpgrp};
use ratatui::{
  Terminal,
  backend::CrosstermBackend,
  layout::{Constraint, Layout},
  style::{Color, Modifier, Style},
  text::Line,
  widgets::{Block, Borders, Paragraph, Wrap},
};
use tokio_util::sync::CancellationToken;

use super::{
  Command, Content,
  layout::{DetailScroll, phase_label, render_tunnel, row},
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
  let mut scroll = DetailScroll::default();
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
      draw(&mut screen.terminal, &context, logs, &mut scroll)?;
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
          KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown | KeyCode::Home | KeyCode::End if !logs => {
            scroll.handle_key(key.code);
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

fn draw(
  terminal: &mut Terminal<CrosstermBackend<File>>,
  context: &Context,
  logs: bool,
  scroll: &mut DetailScroll,
) -> anyhow::Result<()> {
  terminal.draw(|frame| {
    let area = frame.area();
    if area.height < 8 || area.width < 24 {
      frame.render_widget(
        Paragraph::new("GP Connect\nCtrl+C Disconnect").wrap(Wrap { trim: false }),
        area,
      );
      return;
    }
    let compact = area.height < 18;
    let footer_height = if area.width < 80 { 2 } else { 1 };
    let [header, body, notice, footer] = Layout::vertical([
      Constraint::Length(2),
      Constraint::Min(1),
      Constraint::Length(if compact { 1 } else { 3 }),
      Constraint::Length(footer_height),
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
    if logs {
      let history = context.history.lock().unwrap_or_else(|error| error.into_inner());
      let tail = history.tail(body.height as usize);
      lines = tail.into_iter().map(Line::raw).collect();
      let skip = lines.len().saturating_sub(body.height as usize);
      lines.drain(..skip);
    } else {
      match &context.content {
        Content::Tunnel(monitor) => {
          render_tunnel(
            frame,
            body,
            &context.server,
            &monitor.state(),
            &monitor.statistics(),
            scroll,
          );
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
    if logs || matches!(context.content, Content::Internal(_)) {
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
      let block = if compact {
        Block::default()
      } else {
        Block::default().title("Latest event").borders(Borders::TOP)
      };
      frame.render_widget(
        Paragraph::new(current.message)
          .style(Style::default().fg(color))
          .wrap(Wrap { trim: false })
          .block(block),
        notice,
      );
    }
    let ending = if context.cancellation.is_cancelled() {
      "Disconnecting…"
    } else {
      "Ctrl+C Disconnect"
    };
    frame.render_widget(
      footer_text(logs, ending, scroll.can_scroll(), area.width).style(Style::default().fg(Color::DarkGray)),
      footer,
    );
  })?;
  Ok(())
}

fn footer_text(logs: bool, ending: &str, scrollable: bool, width: u16) -> Paragraph<'static> {
  let view = if logs { "Dashboard" } else { "Logs" };
  let scrolling = scrollable && !logs;
  let text = if width < 80 {
    format!("{ending}\nTab {view}{}", if scrolling { "  ↑↓ Scroll" } else { "" })
  } else {
    format!(
      "Tab {view}    {ending}{}",
      if scrolling { "    ↑↓/PgUp/PgDn Scroll" } else { "" }
    )
  };
  Paragraph::new(text)
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
  fn narrow_footer_keeps_disconnect_switch_and_scroll_hints_visible() {
    use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
    let area = Rect::new(0, 0, 24, 2);
    let mut buffer = Buffer::empty(area);
    footer_text(false, "Ctrl+C Disconnect", true, area.width).render(area, &mut buffer);
    let lines: Vec<String> = (0..2)
      .map(|y| (0..24).map(|x| buffer[(x, y)].symbol()).collect())
      .collect();
    assert!(lines[0].contains("Ctrl+C Disconnect"));
    assert!(lines[1].contains("Tab Logs"));
    assert!(lines[1].contains("↑↓ Scroll"));
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
