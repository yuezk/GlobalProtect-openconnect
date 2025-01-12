use serde::{Deserialize, Serialize};

use super::vpn_state::VpnState;

/// Events that can be emitted by the service
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum WsEvent {
  VpnState(VpnState),
  ActiveGui,
  ResumeConnection,
}
