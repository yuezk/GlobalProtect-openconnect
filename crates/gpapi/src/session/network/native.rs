use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, ensure};
use dns_lookup::{AddrFamily, AddrInfoHints, SockType, getaddrinfo};
use tokio_util::sync::CancellationToken;

use super::*;

const MAX_ADDRESSES: usize = 64;

#[doc(hidden)]
pub fn detect_in_helper(address: std::net::IpAddr, hostname: &str) -> anyhow::Result<bool> {
  super::detection::validate_target(address, hostname)?;
  Ok(dns_lookup::lookup_addr(&address).is_ok_and(|name| name.eq_ignore_ascii_case(hostname)))
}

pub(super) async fn detect(
  address: std::net::IpAddr,
  hostname: &str,
  cancellation: &CancellationToken,
) -> anyhow::Result<bool> {
  super::detection::validate_target(address, hostname)?;
  let args = vec![
    "detect-internal-host".into(),
    "--address".into(),
    address.to_string(),
    "--hostname".into(),
    hostname.into(),
  ];
  let output = run_helper(common::binary_paths::gpclient(), args, cancellation.clone()).await?;
  serde_json::from_slice(&output).context("Invalid internal detection helper response")
}

/// Entry point for gpclient's isolated native resolver helper. This blocking
/// function must run in the child process, never a connection runtime thread.
#[doc(hidden)]
pub fn lookup_in_helper(host: &str, port: u16, disable_ipv6: bool) -> anyhow::Result<Vec<SocketAddr>> {
  validate_host(host, port)?;
  let hints = AddrInfoHints {
    address: if disable_ipv6 { AddrFamily::Inet.into() } else { 0 },
    socktype: SockType::Stream.into(),
    ..AddrInfoHints::default()
  };
  let service = port.to_string();
  let mut addresses = Vec::new();
  for result in getaddrinfo(Some(host), Some(&service), Some(hints)).map_err(io::Error::from)? {
    let address = result?.sockaddr;
    if !addresses.contains(&address) {
      ensure!(
        addresses.len() < MAX_ADDRESSES,
        "Native resolver returned too many gateway addresses"
      );
      addresses.push(address);
    }
  }
  validate_addresses(&addresses, port, disable_ipv6)?;
  Ok(addresses)
}

pub(super) async fn resolve(
  host: &str,
  port: u16,
  disable_ipv6: bool,
  cancellation: &CancellationToken,
) -> anyhow::Result<Vec<SocketAddr>> {
  validate_host(host, port)?;
  let mut args = vec!["resolve-gateway".into(), host.into(), "--port".into(), port.to_string()];
  if disable_ipv6 {
    args.push("--disable-ipv6".into());
  }
  run_resolver(
    common::binary_paths::gpclient(),
    args,
    port,
    disable_ipv6,
    cancellation.clone(),
  )
  .await
}

async fn run_resolver(
  executable: PathBuf,
  args: Vec<String>,
  port: u16,
  disable_ipv6: bool,
  cancellation: CancellationToken,
) -> anyhow::Result<Vec<SocketAddr>> {
  let output = run_helper(executable, args, cancellation).await?;
  let addresses: Vec<SocketAddr> = serde_json::from_slice(&output).context("Invalid resolver helper response")?;
  validate_addresses(&addresses, port, disable_ipv6)?;
  Ok(addresses)
}

async fn run_helper(
  executable: PathBuf,
  args: Vec<String>,
  cancellation: CancellationToken,
) -> anyhow::Result<Vec<u8>> {
  // Await the owned worker even on cancellation. CollectorCommands kills and
  // waits for the helper process and any descendants before returning.
  tokio::task::spawn_blocking(move || {
    let budget = CollectionBudget::new(INSPECTION_TIMEOUT);
    let check = || {
      if cancellation.is_cancelled() {
        return Err(io::Error::new(
          io::ErrorKind::Interrupted,
          "Gateway resolution cancelled",
        ));
      }
      budget.check()
    };
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let output = CollectorCommands::new(&check)
      .run_executable(&executable, &args)?
      .context("Native resolver helper is unavailable")?;
    check()?;
    ensure!(output.status.success(), "Native gateway resolution failed");
    Ok(output.stdout)
  })
  .await?
}

fn validate_host(host: &str, port: u16) -> anyhow::Result<()> {
  ensure!(port != 0, "Gateway port is zero");
  ensure!(
    !host.is_empty() && host.len() <= 253 && !host.contains('\0') && !host.contains(['/', ':']),
    "Invalid gateway hostname"
  );
  Ok(())
}

fn validate_addresses(addresses: &[SocketAddr], port: u16, disable_ipv6: bool) -> anyhow::Result<()> {
  ensure!(
    !addresses.is_empty() && addresses.len() <= MAX_ADDRESSES,
    "Invalid gateway address count"
  );
  ensure!(
    addresses.iter().all(|address| {
      address.port() == port && !address.ip().is_unspecified() && (!disable_ipv6 || address.is_ipv4())
    }),
    "Invalid gateway address from resolver helper"
  );
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn actual_helper_output_is_validated_before_admission() {
    let addresses = run_resolver(
      "/bin/sh".into(),
      vec!["-c".into(), "printf '[\"192.0.2.1:443\"]'".into()],
      443,
      true,
      CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(addresses, vec!["192.0.2.1:443".parse::<SocketAddr>().unwrap()]);
    assert!(
      run_resolver(
        "/bin/sh".into(),
        vec!["-c".into(), "printf '[\"192.0.2.1:444\"]'".into()],
        443,
        true,
        CancellationToken::new(),
      )
      .await
      .is_err()
    );
  }

  #[tokio::test]
  async fn cancellation_kills_and_joins_the_running_resolver_process() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let path = file.path().to_owned();
    let cancellation = CancellationToken::new();
    let worker = tokio::spawn(run_resolver(
      "/bin/sh".into(),
      vec![
        "-c".into(),
        "printf '%s' \"$$\" > \"$1\"; sleep 30 & wait".into(),
        "resolver-test".into(),
        path.to_string_lossy().into_owned(),
      ],
      443,
      false,
      cancellation.clone(),
    ));
    let pid = tokio::time::timeout(Duration::from_secs(2), async {
      loop {
        if let Ok(pid) = std::fs::read_to_string(&path).unwrap().parse::<i32>() {
          break pid;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      }
    })
    .await
    .unwrap();
    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_secs(1), worker)
      .await
      .unwrap()
      .unwrap()
      .unwrap_err();
    assert_eq!(
      error.downcast::<io::Error>().unwrap().kind(),
      io::ErrorKind::Interrupted
    );
    assert_eq!(
      nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None),
      Err(nix::errno::Errno::ESRCH)
    );
  }

  #[test]
  fn native_lookup_preserves_port_and_ipv4_policy() {
    let addresses = lookup_in_helper("localhost", 444, true).unwrap();
    assert!(
      addresses
        .iter()
        .all(|address| address.is_ipv4() && address.ip().is_loopback() && address.port() == 444)
    );
  }

  #[test]
  fn helper_response_validation_retains_ipv6_scope_and_rejects_wrong_port() {
    let addresses = vec!["[fe80::1%7]:443".parse().unwrap(), "192.0.2.1:443".parse().unwrap()];
    validate_addresses(&addresses, 443, false).unwrap();
    assert_eq!(
      serde_json::from_str::<Vec<SocketAddr>>(&serde_json::to_string(&addresses).unwrap()).unwrap(),
      addresses
    );
    assert!(validate_addresses(&addresses, 444, false).is_err());
    assert!(validate_addresses(&addresses, 443, true).is_err());
  }

  #[test]
  fn rejects_invalid_helper_requests_before_lookup() {
    assert!(lookup_in_helper("gateway.example/path", 443, false).is_err());
    assert!(lookup_in_helper("gateway.example", 0, false).is_err());
  }
}
