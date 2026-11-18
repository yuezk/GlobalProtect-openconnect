//! Native Linux and BSD computer-domain collection.
use std::io;

#[cfg(unix)]
use gpapi::process::collection::CollectorCommands;

#[cfg(target_os = "linux")]
pub(super) fn collect(commands: &CollectorCommands<'_>) -> io::Result<String> {
  linux_domain(|program, args| read(commands, program, args))
}

#[cfg(target_os = "freebsd")]
pub(super) fn collect(commands: &CollectorCommands<'_>) -> io::Result<String> {
  read(commands, "hostname", &["-d"])
}

#[cfg(target_os = "openbsd")]
pub(super) fn collect(commands: &CollectorCommands<'_>) -> io::Result<String> {
  read(commands, "hostname", &[]).map(|hostname| hostname_domain(&hostname))
}

#[cfg(unix)]
fn read(commands: &CollectorCommands<'_>, program: &str, args: &[&str]) -> io::Result<String> {
  let Some(output) = commands.run(program, args)? else {
    return Ok(String::new());
  };
  let text = String::from_utf8(output.stdout).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
  // A nonzero exit status does not invalidate a reported domain.
  Ok(first_line(&text))
}

#[cfg(any(target_os = "linux", test))]
fn linux_domain(mut read: impl FnMut(&str, &[&str]) -> io::Result<String>) -> io::Result<String> {
  let realm = read("realm", &["list", "--name-only"])?;
  if !realm.is_empty() {
    return Ok(realm);
  }
  read("dnsdomainname", &[])
}

fn first_line(output: &str) -> String {
  output.lines().next().unwrap_or_default().trim().to_owned()
}

#[cfg(any(target_os = "openbsd", test))]
fn hostname_domain(hostname: &str) -> String {
  hostname
    .split_once('.')
    .map(|(_, domain)| domain)
    .unwrap_or_default()
    .to_owned()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(unix)]
  #[test]
  fn uses_first_stdout_line_even_with_nonzero_exit() {
    let budget = gpapi::collection::CollectionBudget::new(std::time::Duration::from_secs(5));
    let commands = CollectorCommands::new(&budget);
    let domain = read(
      &commands,
      "sh",
      &["-c", "printf ' Corp.Example.test \\nsecond.test\\n'; exit 7"],
    )
    .unwrap();
    assert_eq!(domain, "Corp.Example.test");
  }

  #[test]
  fn linux_prefers_realm_and_only_queries_dns_when_empty() {
    let realm = linux_domain(|program, args| {
      assert_eq!(program, "realm");
      assert_eq!(args, ["list", "--name-only"]);
      Ok("Corp.Example.test".into())
    })
    .unwrap();
    assert_eq!(realm, "Corp.Example.test");

    let mut calls = Vec::new();
    let dns = linux_domain(|program, _| {
      calls.push(program.to_string());
      if program == "realm" {
        Ok(String::new())
      } else {
        Ok("dns.example.test".into())
      }
    })
    .unwrap();
    assert_eq!(calls, ["realm", "dnsdomainname"]);
    assert_eq!(dns, "dns.example.test");
    assert_eq!(linux_domain(|_, _| Ok(String::new())).unwrap(), "");
  }

  #[test]
  fn interrupted_collection_does_not_continue_to_dns() {
    let result = linux_domain(|program, _| {
      assert_eq!(program, "realm");
      Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"))
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
  }

  #[test]
  fn reads_only_first_line_and_preserves_domain_case() {
    assert_eq!(first_line(" Corp.Example.test \r\nsecond.test\n"), "Corp.Example.test");
    assert_eq!(first_line("\nsecond.test\n"), "");
    assert_eq!(first_line(""), "");
  }

  #[test]
  fn openbsd_uses_hostname_suffix_without_inventing_a_domain() {
    assert_eq!(hostname_domain("host.Corp.Example.test"), "Corp.Example.test");
    assert_eq!(hostname_domain("host"), "");
    assert_eq!(hostname_domain("host."), "");
    assert_eq!(hostname_domain(""), "");
  }
}
