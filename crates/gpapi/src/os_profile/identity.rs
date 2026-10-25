use serde::{Deserialize, Serialize};
use specta::Type;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HostIdentity {
  computer: String,
  host_id: String,
  #[serde(rename = "serialno")]
  serial_number: String,
  #[serde(rename = "macAddr")]
  mac_address: String,
}

impl HostIdentity {
  /// Values already collected by the caller. Missing observations stay empty.
  pub fn from_parts(computer: String, host_id: String, serialno: String, mac_addr: String) -> Self {
    Self {
      computer,
      host_id,
      serial_number: serialno,
      mac_address: mac_addr,
    }
  }
  /// Identity supplied by an application; no hardware attributes are invented.
  pub fn application(computer: String, installation_id: String) -> Self {
    Self {
      computer,
      host_id: installation_id,
      serial_number: String::new(),
      mac_address: String::new(),
    }
  }

  pub fn computer(&self) -> &str {
    &self.computer
  }

  pub fn host_id(&self) -> &str {
    &self.host_id
  }

  pub fn serialno(&self) -> &str {
    &self.serial_number
  }

  pub fn mac_addr(&self) -> &str {
    &self.mac_address
  }

  pub fn with_computer(mut self, computer: String) -> Self {
    if !computer.trim().is_empty() {
      self.computer = computer;
    }
    self
  }
}
