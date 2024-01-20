use clap::{builder::PossibleValue, ValueEnum};

use crate::gp_params::ClientOs;

#[derive(Debug, Clone)]
pub enum Os {
  Linux,
  Windows,
  Mac,
}

impl Os {
  pub fn as_str(&self) -> &'static str {
    match self {
      Os::Linux => "Linux",
      Os::Windows => "Windows",
      Os::Mac => "Mac",
    }
  }
}

impl From<&str> for Os {
  fn from(os: &str) -> Self {
    match os.to_lowercase().as_str() {
      "linux" => Os::Linux,
      "windows" => Os::Windows,
      "mac" => Os::Mac,
      _ => Os::Linux,
    }
  }
}

impl From<&Os> for ClientOs {
  fn from(value: &Os) -> Self {
    match value {
      Os::Linux => ClientOs::Linux,
      Os::Windows => ClientOs::Windows,
      Os::Mac => ClientOs::Mac,
    }
  }
}

impl ValueEnum for Os {
  fn value_variants<'a>() -> &'a [Self] {
    &[Os::Linux, Os::Windows, Os::Mac]
  }

  fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
    match self {
      Os::Linux => Some(PossibleValue::new("Linux")),
      Os::Windows => Some(PossibleValue::new("Windows")),
      Os::Mac => Some(PossibleValue::new("Mac")),
    }
  }

  fn from_str(input: &str, _: bool) -> Result<Self, String> {
    match input.to_lowercase().as_str() {
      "linux" => Ok(Os::Linux),
      "windows" => Ok(Os::Windows),
      "mac" => Ok(Os::Mac),
      _ => Err(format!("Invalid OS: {}", input)),
    }
  }
}
