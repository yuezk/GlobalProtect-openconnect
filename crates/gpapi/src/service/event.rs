use serde::{Deserialize, Serialize};

use crate::service::{session::SessionInfo, vpn_env::VpnEnv, vpn_state::VpnState};

/// Events that can be emitted by the service
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum WsEvent {
  VpnEnv(VpnEnv),
  VpnState(VpnState),
  SessionInfo(Option<SessionInfo>),
  ActiveGui,
  ResumeConnection,
}
