use std::sync::OnceLock;

use anyhow::bail;
use log::{Level, Log, warn};
use log_reload::{ReloadHandle, ReloadLog};

pub const LOG_BACKEND_ENV: &str = "GP_LOG_BACKEND";
pub const MACOS_UNIFIED_LOG_BACKEND: &str = "macos-unified";

type BoxedLogger = Box<dyn Log + Send + Sync>;
static LOG_HANDLE: OnceLock<ReloadHandle<log_reload::LevelFilter<BoxedLogger>>> = OnceLock::new();

pub fn init(level: Level, subsystem: &str, category: &str) {
  #[cfg(target_os = "macos")]
  if macos_unified_log_enabled() {
    init_with_logger(level, MacosLogger::new(subsystem, category));
    return;
  }

  #[cfg(not(target_os = "macos"))]
  let _ = (subsystem, category);

  // Initialize the env_logger and global max level to trace, the logs will be
  // filtered by the outer logger
  let logger = env_logger::builder().filter_level(log::LevelFilter::Trace).build();
  init_with_logger(level, logger);
}

pub fn init_with_logger(level: Level, logger: impl Log + Send + Sync + 'static) {
  if LOG_HANDLE.get().is_some() {
    warn!("Logger already initialized");
    return;
  }

  log::set_max_level(log::LevelFilter::Trace);

  // Create a new logger that will filter the logs based on the max level
  let level_filter_logger = log_reload::LevelFilter::new(level, Box::new(logger) as BoxedLogger);

  let reload_log = ReloadLog::new(level_filter_logger);
  let handle = reload_log.handle();

  // Register the logger to be used by the log crate
  let _ = log::set_boxed_logger(Box::new(reload_log));
  let _ = LOG_HANDLE.set(handle);
}

pub fn set_max_level(level: Level) -> anyhow::Result<()> {
  let Some(handle) = LOG_HANDLE.get() else {
    bail!("Logger not initialized")
  };

  handle
    .modify(|logger| logger.set_level(level))
    .map_err(|e| anyhow::anyhow!(e))
}

pub fn macos_unified_log_enabled() -> bool {
  cfg!(target_os = "macos") && std::env::var(LOG_BACKEND_ENV).as_deref() == Ok(MACOS_UNIFIED_LOG_BACKEND)
}

#[cfg(target_os = "macos")]
pub struct MacosLogger {
  inner: oslog::OsLog,
}

#[cfg(target_os = "macos")]
impl MacosLogger {
  pub fn new(subsystem: &str, category: &str) -> Self {
    Self {
      inner: oslog::OsLog::new(subsystem, category),
    }
  }

  pub fn write(&self, level: Level, target: &str, message: &str) {
    self
      .inner
      .with_level(macos_log_level(level), &format!("[{target}] {message}"));
  }
}

#[cfg(target_os = "macos")]
impl Log for MacosLogger {
  fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
    self.inner.level_is_enabled(macos_log_level(metadata.level()))
  }

  fn log(&self, record: &log::Record<'_>) {
    if self.enabled(record.metadata()) {
      self.write(record.level(), record.target(), &record.args().to_string());
    }
  }

  fn flush(&self) {}
}

#[cfg(target_os = "macos")]
fn macos_log_level(level: Level) -> oslog::Level {
  match level {
    Level::Trace | Level::Debug => oslog::Level::Debug,
    Level::Info => oslog::Level::Default,
    Level::Warn | Level::Error => oslog::Level::Error,
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use super::*;

  #[test]
  fn info_logs_use_the_persistent_macos_level() {
    assert_eq!(macos_log_level(Level::Info) as u8, oslog::Level::Default as u8);
  }
}
