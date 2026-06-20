use clap::{ValueEnum, builder::PossibleValue};

use crate::{
  gp_params::CscMode,
  os_profile::{ClientOs, runtime_client_os},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Os {
  #[value(name = "Linux")]
  Linux,
  #[value(name = "Windows")]
  Windows,
  #[value(name = "Mac")]
  Mac,
}

impl From<Os> for ClientOs {
  fn from(value: Os) -> Self {
    match value {
      Os::Linux => ClientOs::Linux,
      Os::Windows => ClientOs::Windows,
      Os::Mac => ClientOs::Mac,
    }
  }
}

impl Default for Os {
  fn default() -> Self {
    match runtime_client_os() {
      ClientOs::Linux => Os::Linux,
      ClientOs::Windows => Os::Windows,
      ClientOs::Mac => Os::Mac,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn default_os_uses_runtime_client_os() {
    assert_eq!(ClientOs::from(Os::default()), runtime_client_os());
  }
}

impl ValueEnum for CscMode {
  fn value_variants<'a>() -> &'a [Self] {
    &[CscMode::Auto, CscMode::Yes, CscMode::No]
  }

  fn to_possible_value(&self) -> Option<PossibleValue> {
    Some(PossibleValue::new(self.as_str()))
  }

  fn from_str(input: &str, _: bool) -> Result<Self, String> {
    match input.to_lowercase().as_str() {
      "auto" => Ok(CscMode::Auto),
      "yes" => Ok(CscMode::Yes),
      "no" => Ok(CscMode::No),
      _ => Err(format!("Invalid CSC mode: {}", input)),
    }
  }
}
