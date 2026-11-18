//! Ownership records for physical routes, retained across process termination.
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{io, net::IpAddr};
use winreg::{RegKey, enums::*};

const PREFIX: &str = "Route-";
#[derive(Serialize, Deserialize)]
pub(super) struct Route {
  pub prefix: IpNet,
  pub next_hop: IpAddr,
  pub interface_luid: u64,
  pub metric: u32,
  pub prefix_scope: u32,
  pub next_hop_scope: u32,
}
pub(super) struct Journal(RegKey);
impl Journal {
  pub fn begin(path: &str) -> io::Result<Self> {
    let (parent, name) = path
      .rsplit_once('\\')
      .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Recovery key must have a registry parent"))?;
    let (parent, _) = RegKey::predef(HKEY_LOCAL_MACHINE).create_subkey(parent)?;
    // ActiveStore routes and this volatile key share the kernel's lifetime.
    // HKLM parent permissions restrict writes to SYSTEM and administrators.
    let (key, _) = parent.create_subkey_with_options_flags(name, REG_OPTION_VOLATILE, KEY_READ | KEY_WRITE)?;
    let journal = Self(key);
    if !journal.routes()?.is_empty() {
      return Err(io::Error::other(
        "Recover the previous Windows network state before connecting",
      ));
    }
    Ok(journal)
  }
  pub fn open(path: &str) -> io::Result<Option<Self>> {
    match RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(path, KEY_READ | KEY_WRITE) {
      Ok(key) => Ok(Some(Self(key))),
      Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
      Err(error) => Err(error),
    }
  }
  pub fn routes(&self) -> io::Result<Vec<(String, Route)>> {
    self
      .0
      .enum_values()
      .filter_map(|value| match value {
        Ok((name, _)) if name.starts_with(PREFIX) => Some((|| {
          let text: String = self.0.get_value(&name)?;
          let route = serde_json::from_str(&text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
          Ok((name, route))
        })()),
        Ok(_) => None,
        Err(error) => Some(Err(error)),
      })
      .collect()
  }
  pub fn record(&self, route: &Route) -> io::Result<String> {
    let name = format!("{PREFIX}{}", uuid::Uuid::new_v4());
    self
      .0
      .set_value(&name, &serde_json::to_string(route).map_err(io::Error::other)?)?;
    Ok(name)
  }
  pub fn forget(&self, name: &str) -> io::Result<()> {
    match self.0.delete_value(name) {
      Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
      result => result,
    }
  }
}
