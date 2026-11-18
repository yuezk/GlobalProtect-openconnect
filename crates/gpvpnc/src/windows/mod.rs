//! Windows network installation driven by OpenConnect's normal configuration callback.
mod configuration;
mod dns;
mod ip_helper;
mod recovery;
use configuration::Configuration;
use std::io;

pub struct NetworkConfigurator {
  connection: Option<Connection>,
  recovery_key: String,
}
struct Connection {
  dns: dns::Installed,
  network: ip_helper::Installed,
}
impl NetworkConfigurator {
  /// The owner supplies an HKLM key under its administrator-controlled namespace.
  pub fn new(recovery_key: &str) -> io::Result<Self> {
    recovery::Journal::begin(recovery_key)?;
    Ok(Self {
      connection: None,
      recovery_key: recovery_key.into(),
    })
  }
  pub fn configure(&mut self, reason: &str, environment: &[(String, Option<String>)]) -> io::Result<()> {
    match reason {
      "pre-init" | "attempt-reconnect" => Ok(()),
      "connect" | "reconnect" => {
        let configuration = Configuration::parse(environment)?;
        self.disconnect()?;
        let network = ip_helper::Installed::apply(&configuration, &self.recovery_key)?;
        let dns = dns::Installed::apply(&configuration)?;
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

/// Restore process-owned physical routes and DNS policy before service admission.
pub fn recover(recovery_key: &str) -> io::Result<()> {
  if let Some(journal) = recovery::Journal::open(recovery_key)? {
    for (name, route) in journal.routes()? {
      ip_helper::recover_route(&route)?;
      journal.forget(&name)?;
    }
  }
  dns::clear_stale_policies()
}
