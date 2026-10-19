use std::io;

pub type NetworkConfigurationHandler = dyn Fn(&NetworkConfiguration) -> io::Result<()> + Send + Sync;

/// OpenConnect's configuration inputs for one lifecycle event.
/// Values are copied from OpenConnect before calling the handler. A missing
/// value means the variable must be unset, rather than inherited by a child.
#[derive(Debug, Clone)]
pub struct NetworkConfiguration {
  pub reason: String,
  pub environment: Vec<(String, Option<String>)>,
}
