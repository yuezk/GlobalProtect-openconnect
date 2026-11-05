mod client_os;
pub(crate) mod defaults;
#[cfg(feature = "desktop")]
pub mod desktop;
mod identity;
mod profile;
mod projection;
mod serial_number;

pub(crate) use client_os::PreloginParamLocation;
use client_os::target;
pub use client_os::{ClientOs, PreloginBrowserMode, WebviewUserAgent, WebviewUserAgentTransform};
use common::constants::{GP_CLIENT_VERSION_LINUX, GP_CLIENT_VERSION_MACOS, GP_CLIENT_VERSION_WINDOWS, GP_USER_AGENT};
use defaults::*;
pub use identity::HostIdentity;
use profile::ProfileHostIdentity;
pub use profile::{HostSnapshot, OsProfile, OsProfileBuilder};
use serde::{Deserialize, Serialize};
use specta::Type;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn test_snapshot() -> HostSnapshot {
  HostSnapshot {
    runtime_os: ClientOs::Mac,
    identity: HostIdentity::from_parts(
      "test-computer".into(),
      "test-host-id".into(),
      "test-serial".into(),
      "aa:bb:cc:dd:ee:ff".into(),
    ),
    os_version: defaults::os_version(ClientOs::Mac),
    software_version: defaults::software_version(ClientOs::Mac),
  }
}
