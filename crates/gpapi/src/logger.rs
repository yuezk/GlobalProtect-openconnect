use std::sync::OnceLock;

use anyhow::bail;
use env_logger::Logger;
use log::Level;
use log_reload::{ReloadHandle, ReloadLog};

static LOG_HANDLE: OnceLock<ReloadHandle<log_reload::LevelFilter<Logger>>> = OnceLock::new();

pub fn init(level: Level) -> anyhow::Result<()> {
  // Initialize the env_logger and global max level to trace, the logs will be
  // filtered by the outer logger
  let logger = env_logger::builder().filter_level(log::LevelFilter::Trace).build();
  init_with_logger(level, logger)?;

  Ok(())
}

pub fn init_with_logger(level: Level, logger: Logger) -> anyhow::Result<()> {
  if let Some(_) = LOG_HANDLE.get() {
    bail!("Logger already initialized")
  } else {
    log::set_max_level(log::LevelFilter::Trace);

    // Create a new logger that will filter the logs based on the max level
    let level_filter_logger = log_reload::LevelFilter::new(level, logger);

    let reload_log = ReloadLog::new(level_filter_logger);
    let handle = reload_log.handle();

    // Register the logger to be used by the log crate
    log::set_boxed_logger(Box::new(reload_log))?;
    LOG_HANDLE
      .set(handle)
      .map_err(|_| anyhow::anyhow!("Failed to set the logger"))?;
  }

  Ok(())
}

pub fn set_max_level(level: Level) -> anyhow::Result<()> {
  let Some(handle) = LOG_HANDLE.get() else {
    bail!("Logger not initialized")
  };

  handle
    .modify(|logger| logger.set_level(level))
    .map_err(|e| anyhow::anyhow!(e))
}
