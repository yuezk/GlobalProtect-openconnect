use std::sync::OnceLock;

const DEFAULT_MACOS_VERSION: &str = "13.4.0";
#[cfg(not(target_os = "linux"))]
const DEFAULT_LINUX_DISTRO: &str = "Ubuntu 24.04.3 LTS";
#[cfg(not(target_os = "windows"))]
const DEFAULT_WINDOWS_DISTRO: &str = "Windows 11 Pro";
const DEFAULT_WINDOWS_VERSION: &str = "10.0.22631.0";
const DEFAULT_MACHINE_ID: &str = "DEADBEEF-DEAD-BEEF-DEAD-BEEFDEADBEEF";

static MACHINE_ID: OnceLock<&'static str> = OnceLock::new();
static MACOS_VERSION: OnceLock<&'static str> = OnceLock::new();
static MACOS_OS_STRING: OnceLock<String> = OnceLock::new();
static LINUX_OS_STRING: OnceLock<String> = OnceLock::new();
static WINDOWS_VERSION: OnceLock<&'static str> = OnceLock::new();
static WINDOWS_OS_STRING: OnceLock<String> = OnceLock::new();

pub fn get_macos_version() -> &'static str {
  MACOS_VERSION.get_or_init(|| {
    #[cfg(target_os = "macos")]
    {
      match os_info::get().version() {
        os_info::Version::Unknown => DEFAULT_MACOS_VERSION,
        version => Box::leak(version.to_string().into_boxed_str()),
      }
    }

    #[cfg(not(target_os = "macos"))]
    DEFAULT_MACOS_VERSION
  })
}

pub fn get_macos_os_string() -> &'static str {
  MACOS_OS_STRING.get_or_init(|| format!("Apple Mac OS X {}", get_macos_version()))
}

pub fn get_linux_os_string() -> &'static str {
  LINUX_OS_STRING.get_or_init(|| {
    #[cfg(target_os = "linux")]
    {
      format!("Linux {}", whoami::distro())
    }

    #[cfg(not(target_os = "linux"))]
    {
      format!("Linux {}", DEFAULT_LINUX_DISTRO)
    }
  })
}

pub fn get_windows_version() -> &'static str {
  WINDOWS_VERSION.get_or_init(|| {
    #[cfg(target_os = "windows")]
    {
      match os_info::get().version() {
        os_info::Version::Unknown => DEFAULT_WINDOWS_VERSION,
        version => Box::leak(format!("{}.0", version).into_boxed_str()),
      }
    }

    #[cfg(not(target_os = "windows"))]
    DEFAULT_WINDOWS_VERSION
  })
}

pub fn get_windows_os_string() -> &'static str {
  WINDOWS_OS_STRING.get_or_init(|| {
    #[cfg(target_os = "windows")]
    {
      let edition = os_info::get()
        .edition()
        .map(|edition| edition.to_string())
        .unwrap_or_else(|| whoami::distro());
      format!("Microsoft {} , 64-bit", edition)
    }

    #[cfg(not(target_os = "windows"))]
    {
      format!("Microsoft {} , 64-bit", DEFAULT_WINDOWS_DISTRO)
    }
  })
}

pub fn get_machine_id() -> &'static str {
  MACHINE_ID.get_or_init(|| {
    machine_uid::get()
      .map(|id| Box::leak(id.to_string().into_boxed_str()) as &'static str)
      .unwrap_or(DEFAULT_MACHINE_ID)
  })
}

pub fn derive_uuid(seeds: &[&str]) -> String {
  use uuid::Uuid;

  let namespace = Uuid::NAMESPACE_DNS;
  let name = format!("{}-{}", get_machine_id(), seeds.join("-"));

  Uuid::new_v5(&namespace, name.as_bytes()).hyphenated().to_string()
}
