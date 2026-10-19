use std::sync::{Arc, Mutex, OnceLock};

use log::{Level, Log};

use super::output::plain_text;

#[derive(Clone)]
pub(super) struct Notice {
  pub(super) level: Level,
  pub(super) message: String,
}

pub(super) type NoticeSlot = Arc<Mutex<Option<Notice>>>;
static NOTICES: OnceLock<NoticeSlot> = OnceLock::new();

pub(super) fn slot() -> NoticeSlot {
  NOTICES.get_or_init(Arc::default).clone()
}

pub(crate) fn session_notice(level: Level, message: &str) {
  if let Some(slot) = NOTICES.get() {
    let mut end = message.len().min(8192);
    while !message.is_char_boundary(end) {
      end -= 1;
    }
    *slot.lock().unwrap_or_else(|error| error.into_inner()) = Some(Notice {
      level,
      message: plain_text(message[..end].as_bytes()),
    });
  }
}

/// Metadata goes to the notice slot; descriptor capture owns the sole history entry.
pub(crate) struct NoticeLogger {
  inner: env_logger::Logger,
}

impl NoticeLogger {
  pub(crate) fn new(inner: env_logger::Logger) -> Self {
    let _ = slot();
    Self { inner }
  }
}

impl Log for NoticeLogger {
  fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
    self.inner.enabled(metadata)
  }
  fn log(&self, record: &log::Record<'_>) {
    if self.inner.matches(record) {
      if record.level() <= Level::Warn {
        session_notice(record.level(), &record.args().to_string());
      }
      self.inner.log(record);
    }
  }
  fn flush(&self) {
    self.inner.flush();
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn notice_tap_honors_message_filters_and_keeps_warning_severity() {
    let logger = NoticeLogger::new(
      env_logger::Builder::new()
        .parse_filters("warn/allowed")
        .target(env_logger::Target::Pipe(Box::new(std::io::sink())))
        .build(),
    );
    session_notice(Level::Info, "previous event");
    logger.log(
      &log::Record::builder()
        .level(Level::Warn)
        .args(format_args!("filtered warning"))
        .build(),
    );
    assert_eq!(slot().lock().unwrap().as_ref().unwrap().message, "previous event");
    logger.log(
      &log::Record::builder()
        .level(Level::Warn)
        .args(format_args!("allowed warning"))
        .build(),
    );
    let current = slot();
    let notice = current.lock().unwrap();
    assert_eq!(notice.as_ref().unwrap().level, Level::Warn);
    assert_eq!(notice.as_ref().unwrap().message, "allowed warning");
  }
}
