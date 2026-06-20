use serde::{Deserialize, Serialize};

use crate::{os_profile::HostIdentity, service::vpn_state::VpnState};

/// The single DTO carrying all host facts gpservice ships to gpgui.
///
/// Populated by gpservice at WS connection time and sent inside `VpnEnv`.
/// gpgui stores it in a process-wide `OnceLock` and routes `AppSettings`,
/// `Constants`, and `OsProfile` construction through it.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HostInfo {
  /// Host identity collected by gpservice for the runtime OS.
  pub host_identity: HostIdentity,
}

/// Represents the VPN environment configuration.
/// When a client connects, the gpservice sends the current VPN environment
/// to the client so that it can configure itself accordingly.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VpnEnv {
  /// The VPN connection state
  pub vpn_state: VpnState,

  /// The default VPN script path
  pub vpnc_script: Option<String>,

  /// The default CSD wrapper script path
  pub csd_wrapper: Option<String>,

  /// The gpauth executable path
  /// Used by the client to launch gpauth for authentication
  pub auth_executable: String,

  /// Host identity collected by gpservice.
  pub host_info: HostInfo,
}
