//! Shared observation of native tunnels, independent of terminal and service transport.

mod configuration;
mod monitor;

pub use monitor::{TunnelMonitor, TunnelUpdate};
