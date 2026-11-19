//! Windows network installation driven by OpenConnect's normal configuration callback.
mod configuration;
mod dns;
mod ip_helper;
mod recovery;
mod system_tunnel;
use configuration::Configuration;
use std::io;
pub use system_tunnel::SystemTunnel;

pub struct NetworkConfigurator {
  connection: Option<Connection>,
  recovery_key: String,
  system_tunnel: Option<SystemTunnel>,
}
struct Connection {
  dns: dns::Installed,
  network: ip_helper::Installed,
}
impl NetworkConfigurator {
  /// Use a separate HKLM journal and DNS tag for an isolated network configuration.
  pub fn new(recovery_key: &str) -> io::Result<Self> {
    Self::with_owner(recovery_key, None)
  }

  fn with_owner(recovery_key: &str, system_tunnel: Option<SystemTunnel>) -> io::Result<Self> {
    recovery::Journal::begin(recovery_key)?;
    Ok(Self {
      connection: None,
      recovery_key: recovery_key.into(),
      system_tunnel,
    })
  }
  pub fn configure(&mut self, reason: &str, environment: &[(String, Option<String>)]) -> io::Result<()> {
    match reason {
      "pre-init" | "attempt-reconnect" => Ok(()),
      "connect" | "reconnect" => {
        let configuration = Configuration::parse(environment)?;
        self.disconnect()?;
        let network = ip_helper::Installed::apply(&configuration, &self.recovery_key)?;
        let dns = dns::Installed::apply(&configuration, &self.recovery_key, self.system_tunnel.as_ref())?;
        self.connection = Some(Connection { dns, network });
        Ok(())
      }
      "disconnect" => self.disconnect(),
      _ => Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("Unknown VPN configuration reason: {reason}"),
      )),
    }
  }
  fn disconnect(&mut self) -> io::Result<()> {
    if let Some(mut connection) = self.connection.take() {
      let dns = connection.dns.restore();
      let network = connection.network.restore();
      dns.and(network)
    } else {
      Ok(())
    }
  }
}

/// Recover an isolated journal and only the DNS rules recorded under its name.
pub fn recover(recovery_key: &str) -> io::Result<()> {
  recover_routes(recovery_key)?;
  dns::remove_journal_policy(recovery_key)
}

fn recover_routes(recovery_key: &str) -> io::Result<()> {
  if let Some(journal) = recovery::Journal::open(recovery_key)? {
    for (name, route) in journal.routes()? {
      ip_helper::recover_route(&route)?;
      journal.forget(&name)?;
    }
  }
  Ok(())
}
