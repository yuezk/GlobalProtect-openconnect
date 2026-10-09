use std::{
  net::{Ipv4Addr, Ipv6Addr},
  sync::Arc,
  time::Duration,
};

use crate::gateway::Gateway;

use super::transport::GatewayTransport;

const LOGOUT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ClientAddresses {
  pub ipv4: Option<Ipv4Addr>,
  pub ipv6: Option<Ipv6Addr>,
}

/// One issued gateway authentication and its original request context. Session
/// mode governs execution, while every mode has the same cleanup owner.
pub struct GatewaySession {
  pub gateway: Gateway,
  pub transport: GatewayTransport,
  pub addresses: ClientAddresses,
}

/// Owns issued gateway sessions independently of tunnel or HIP setup. Workers
/// borrow members; their owner joins them before consuming this ledger in logout.
/// This owner is deliberately not Clone: transfer or drain it exactly once.
#[derive(Default)]
pub struct GatewaySessions {
  members: Vec<Arc<GatewaySession>>,
}

impl GatewaySessions {
  pub fn new(members: Vec<GatewaySession>) -> Self {
    Self {
      members: members.into_iter().map(Arc::new).collect(),
    }
  }

  pub fn register(&mut self, session: GatewaySession) {
    self.members.push(Arc::new(session));
  }

  pub(crate) fn update(&mut self, session: GatewaySession) {
    if let Some(member) = self
      .members
      .iter_mut()
      .find(|member| member.gateway.server() == session.gateway.server())
    {
      *member = Arc::new(session);
    }
  }

  pub fn is_empty(&self) -> bool {
    self.members.is_empty()
  }

  /// Consume the local cleanup copies only after another owner has proved it
  /// acquired responsibility for these issued sessions.
  pub fn relinquish(self) {}

  pub fn members(&self) -> &[Arc<GatewaySession>] {
    &self.members
  }

  /// Transfer a selected tunnel login to OpenConnect immediately before its
  /// native worker starts. OpenConnect owns the live reconnect cookie and logout.
  pub fn relinquish_gateway(&mut self, server: &str) {
    if let Some(index) = self.members.iter().position(|member| member.gateway.server() == server) {
      self.members.remove(index);
    }
  }

  /// Call only after this member's worker has joined. Removing ownership before
  /// awaiting logout prevents routine final cleanup from repeating the request.
  pub async fn logout_gateway(&mut self, server: &str) {
    if let Some(index) = self.members.iter().position(|member| member.gateway.server() == server) {
      logout(self.members.remove(index)).await;
    }
  }

  pub async fn logout(self) {
    let mut workers = tokio::task::JoinSet::new();
    for session in self.members {
      workers.spawn(logout(session));
    }
    while let Some(result) = workers.join_next().await {
      if let Err(error) = result {
        log::warn!("Gateway cleanup worker failed: {error}");
      }
    }
  }
}

async fn logout(session: Arc<GatewaySession>) {
  match tokio::time::timeout(LOGOUT_TIMEOUT, session.transport.logout()).await {
    Ok(Ok(())) => {}
    Ok(Err(error)) => log::warn!("Gateway logout failed: {error}"),
    Err(_) => log::warn!("Gateway logout timed out"),
  }
}
