use anyhow::{bail, Result};
use log::{info, warn};
use std::env;
use std::path::{Path, PathBuf};

/// Determines if the current process is running with root privileges
pub fn is_running_as_root() -> bool {
  // Check if running as root user
  if whoami::username() == "root" {
    return true;
  }

  // Check if running via sudo (SUDO_USER environment variable exists)
  if env::var("SUDO_USER").is_ok() {
    return true;
  }

  // Check effective user ID on Unix systems
  #[cfg(unix)]
  {
    unsafe {
      return libc::geteuid() == 0;
    }
  }

  #[cfg(not(unix))]
  false
}

/// Gets the appropriate runtime directory based on user privileges
pub fn get_runtime_dir() -> Result<PathBuf> {
  if is_running_as_root() {
    // For root users, use system-wide runtime directory
    Ok(PathBuf::from("/var/run"))
  } else {
    // For non-root users, prefer XDG_RUNTIME_DIR, fallback to user-specific directory
    if let Ok(xdg_runtime) = env::var("XDG_RUNTIME_DIR") {
      let path = PathBuf::from(xdg_runtime);
      if path.exists() {
        return Ok(path);
      }
    }

    // Fallback to ~/.local/state
    if let Some(home) = dirs::home_dir() {
      let local_state = home.join(".local").join("state").join("globalprotect");
      return Ok(local_state);
    }

    bail!("Unable to determine appropriate runtime directory")
  }
}

/// Gets the appropriate lock file path for the service
pub fn get_service_lock_path() -> Result<PathBuf> {
  let runtime_dir = get_runtime_dir()?;
  Ok(runtime_dir.join("gpservice.lock"))
}

/// Gets the appropriate lock file path for the client
pub fn get_client_lock_path() -> Result<PathBuf> {
  let runtime_dir = get_runtime_dir()?;
  Ok(runtime_dir.join("gpclient.lock"))
}

/// Gets the appropriate path for the disconnected PID file
pub fn get_disconnected_pid_path() -> Result<PathBuf> {
  if is_running_as_root() {
    // For root users, use /tmp (traditional location)
    Ok(PathBuf::from("/tmp/gpservice_disconnected.pid"))
  } else {
    // For non-root users, use user-specific temp directory
    let runtime_dir = get_runtime_dir()?;
    Ok(runtime_dir.join("gpservice_disconnected.pid"))
  }
}

/// Checks if we have write permission to the specified path
pub fn check_write_permission<P: AsRef<Path>>(path: P) -> Result<()> {
  let path = path.as_ref();

  // Check if parent directory exists and is writable
  if let Some(parent) = path.parent() {
    if !parent.exists() {
      // Try to create the directory structure
      if let Err(e) = std::fs::create_dir_all(parent) {
        bail!(
          "Cannot create directory '{}': {}. This may require elevated privileges.",
          parent.display(),
          e
        );
      }
      info!("Created directory: {}", parent.display());
    }

    // Test write permission by creating a temporary file
    let test_file = parent.join(".gptest_write_permission");
    match std::fs::write(&test_file, "test") {
      Ok(_) => {
        // Clean up test file
        let _ = std::fs::remove_file(&test_file);
        Ok(())
      }
      Err(e) => {
        bail!(
          "No write permission to directory '{}': {}. {}",
          parent.display(),
          e,
          get_permission_suggestion(parent)
        );
      }
    }
  } else {
    bail!("Invalid path: {}", path.display());
  }
}

/// Provides suggestions for resolving permission issues
fn get_permission_suggestion(path: &Path) -> String {
  if path.starts_with("/var/run") || path.starts_with("/tmp") {
    if is_running_as_root() {
      "The process is running as root but still cannot write to this system directory. Check filesystem permissions."
        .to_string()
    } else {
      "Try running with elevated privileges using 'sudo' or 'pkexec', or run as a regular user to use user-specific directories.".to_string()
    }
  } else {
    format!(
      "Ensure the directory '{}' exists and is writable by the current user ({})",
      path.display(),
      whoami::username()
    )
  }
}

/// Ensures a lock file path is valid and accessible, creating directories as needed
pub fn ensure_lock_file_accessible<P: AsRef<Path>>(path: P) -> Result<()> {
  let path = path.as_ref();

  info!("Checking access to lock file: {}", path.display());

  // Check write permission
  check_write_permission(path)?;

  // If file already exists, check if we can read it
  if path.exists() {
    match std::fs::read_to_string(path) {
      Ok(_) => {
        info!("Lock file exists and is readable: {}", path.display());
      }
      Err(e) => {
        warn!("Lock file exists but cannot be read: {}. Error: {}", path.display(), e);
      }
    }
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_is_running_as_root() {
    // This test will vary based on how it's run
    let is_root = is_running_as_root();
    println!("Running as root: {}", is_root);
  }

  #[test]
  fn test_get_runtime_dir() {
    let runtime_dir = get_runtime_dir().unwrap();
    println!("Runtime directory: {}", runtime_dir.display());
    assert!(runtime_dir.is_absolute());
  }

  #[test]
  fn test_lock_paths() {
    let service_lock = get_service_lock_path().unwrap();
    let client_lock = get_client_lock_path().unwrap();

    println!("Service lock path: {}", service_lock.display());
    println!("Client lock path: {}", client_lock.display());

    assert!(service_lock.file_name().unwrap() == "gpservice.lock");
    assert!(client_lock.file_name().unwrap() == "gpclient.lock");
  }
}
