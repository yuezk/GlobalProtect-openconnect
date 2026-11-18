//! Native macOS Active Directory domain collection.
use gpapi::process::collection::CollectorCommands;
use std::{io, path::Path};

pub(super) fn collect(commands: &CollectorCommands<'_>) -> io::Result<String> {
  let Some(output) = commands.run_executable(Path::new("/usr/sbin/dsconfigad"), &["-show"])? else {
    return Ok(String::new());
  };
  let text = String::from_utf8(output.stdout).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
  // A nonzero exit status does not invalidate a reported domain.
  Ok(parse_domain(&text))
}

fn parse_domain(output: &str) -> String {
  output
    .lines()
    .find_map(|line| {
      let (label, value) = line.split_once('=')?;
      (label.trim() == "Active Directory Domain").then(|| value.trim().to_owned())
    })
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn reads_ad_domain_without_using_other_domain_fields() {
    assert_eq!(
      parse_domain(
        "Forest = forest.test\nActive Directory Domain    = Corp.Example.test \t\r\nComputer Account = HOST$\n"
      ),
      "Corp.Example.test"
    );
    assert_eq!(
      parse_domain("Active Directory Domain = first.test\nActive Directory Domain = second.test\n"),
      "first.test"
    );
  }

  #[test]
  fn unbound_or_empty_domain_stays_empty() {
    for output in [
      "",
      "Not bound to Active Directory\n",
      "Forest = forest.test\n",
      "Active Directory Domain = \t\n",
    ] {
      assert_eq!(parse_domain(output), "");
    }
  }
}
