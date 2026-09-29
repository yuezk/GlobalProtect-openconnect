use std::{
  env, fs,
  io::{self, Write},
};

use anyhow::{Context, ensure};
use clap::Parser;
use gpapi::{
  clap::args::Os,
  os_profile::{ClientOs, OsProfileBuilder},
};
use gphip::{ReportContext, ReportInput, generate_report, refresh_edited_report};
use gpservice::hip_runner_state::{MAX_STATE_BYTES, RunnerReport, STATE_NAME, validate_root_owned_path};

#[derive(Parser)]
struct Args {
  #[arg(long)]
  client_version: String,
  #[arg(long, value_enum)]
  client_os: Os,
  #[arg(long)]
  os_version: Option<String>,
  #[arg(long)]
  host_id: Option<String>,
  #[arg(long)]
  cookie: Option<String>,
  #[arg(long)]
  client_ip: Option<String>,
  #[arg(long)]
  client_ipv6: Option<String>,
  #[arg(long)]
  md5: Option<String>,
  #[arg(long)]
  preview: bool,
}

fn main() {
  if let Err(err) = run() {
    eprintln!("HIP report generation failed: {err:#}");
    std::process::exit(1);
  }
}

fn run() -> anyhow::Result<()> {
  let args = Args::parse();
  #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
  let original_args: Vec<_> = env::args_os().skip(1).collect();
  let executable = env::current_exe().context("Cannot locate HIP runner")?;
  validate_root_owned_path(&executable, true)?;
  let state = executable
    .parent()
    .context("HIP runner has no parent")?
    .join(STATE_NAME);
  let metadata = fs::symlink_metadata(&state).context("Cannot inspect HIP report state")?;
  ensure!(
    metadata.is_file() && !metadata.file_type().is_symlink(),
    "Invalid HIP report state"
  );
  use std::os::unix::fs::{MetadataExt, PermissionsExt};
  ensure!(
    metadata.uid() == 0 && metadata.permissions().mode() & 0o077 == 0,
    "HIP report state is not private"
  );
  ensure!(
    metadata.len() <= MAX_STATE_BYTES as u64,
    "HIP report state is too large"
  );
  let bytes = fs::read(&state).context("Cannot read HIP report state")?;
  ensure!(bytes.len() <= MAX_STATE_BYTES, "HIP report state is too large");
  let report: RunnerReport = serde_json::from_slice(&bytes).context("Invalid HIP report state")?;

  // This binary is single-threaded. Clear the VPN and desktop environment
  // before host collectors invoke any external system utilities.
  for (key, _) in env::vars_os() {
    unsafe { env::remove_var(key) };
  }
  unsafe {
    env::set_var("PATH", "/usr/bin:/bin:/usr/sbin:/sbin");
    env::set_var("LC_ALL", "C");
    env::set_var(
      "HOME",
      if cfg!(target_os = "macos") {
        "/var/root"
      } else {
        "/root"
      },
    );
  }

  let report = match report {
    RunnerReport::ApprovedRootScript { approval_id, owner_uid } => {
      #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd"))]
      {
        let approved = gpservice::hip_approval::resolve(&approval_id, owner_uid)?;
        let script_args = original_args.into_iter().filter(|arg| arg != "--preview");
        let status = std::process::Command::new(approved.path)
          .args(script_args)
          .status()
          .context("Approved HIP script did not start")?;
        ensure!(status.success(), "Approved HIP script failed");
        return Ok(());
      }
      #[cfg(not(any(target_os = "linux", target_os = "freebsd", target_os = "openbsd")))]
      {
        let _ = (approval_id, owner_uid);
        anyhow::bail!("Root HIP script approvals are unavailable on this platform");
      }
    }
    report => report,
  };

  let mut builder = OsProfileBuilder::new(ClientOs::from(args.client_os)).client_version(args.client_version);
  if let Some(host_id) = args.host_id {
    builder = builder.host_id_override(host_id);
  }
  let context = if args.preview {
    ReportContext::Preview
  } else {
    ReportContext::Connected {
      cookie: args.cookie.context("Missing HIP cookie")?,
      client_ip: args.client_ip,
      client_ipv6: args.client_ipv6,
      md5: args.md5.context("Missing HIP digest")?,
    }
  };
  let input = ReportInput {
    profile: builder.build(),
    context,
  };
  let xml = match report {
    RunnerReport::Generated => generate_report(&input)?,
    RunnerReport::Edited { xml } => refresh_edited_report(&xml, &input)?,
    RunnerReport::ApprovedRootScript { .. } => unreachable!("approved script returned before report generation"),
  };
  io::stdout().write_all(xml.as_bytes())?;
  Ok(())
}
