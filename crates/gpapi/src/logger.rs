use std::sync::OnceLock;

use anyhow::bail;
use env_logger::Logger;
use log::{warn, Level};
use log_reload::{ReloadHandle, ReloadLog};

static LOG_HANDLE: OnceLock<ReloadHandle<log_reload::LevelFilter<Logger>>> = OnceLock::new();

pub fn init(level: Level) {
  // Initialize the env_logger and global max level to trace, the logs will be
  // filtered by the outer logger
  let logger = env_logger::builder().filter_level(log::LevelFilter::Trace).build();
  init_with_logger(level, logger);
}

pub fn init_with_logger(level: Level, logger: Logger) {
  if let Some(_) = LOG_HANDLE.get() {
    warn!("Logger already initialized");
    return;
  }

  log::set_max_level(log::LevelFilter::Trace);

  // Create a new logger that will filter the logs based on the max level
  let level_filter_logger = log_reload::LevelFilter::new(level, logger);

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
