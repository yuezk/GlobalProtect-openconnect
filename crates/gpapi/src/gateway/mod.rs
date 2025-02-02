mod login;
mod parse_gateways;

#[cfg(unix)]
pub mod hip;

pub use login::*;
pub(crate) use parse_gateways::*;

use serde::{Deserialize, Serialize};
use specta::Type;

use std::fmt::Display;

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
pub(crate) struct PriorityRule {
  pub(crate) name: String,
  pub(crate) priority: u32,
}

#[derive(Debug, Serialize, Deserialize, Type, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Gateway {
  pub(crate) name: String,
  pub(crate) address: String,
  pub(crate) priority: u32,
  pub(crate) priority_rules: Vec<PriorityRule>,
}

impl Display for Gateway {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{} ({})", self.name, self.address)
  }
}

impl Gateway {
  pub fn new(name: String, address: String) -> Self {
    Self {
      name,
      address,
      priority: 0,
      priority_rules: vec![],
    }
  }

  pub fn name(&self) -> &str {
    &self.name
  }

  pub fn server(&self) -> &str {
    &self.address
  }
}
