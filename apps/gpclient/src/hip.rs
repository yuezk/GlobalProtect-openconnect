use std::time::Duration;

use clap::Args;
use gpapi::{
  clap::args::Os,
  os_profile::{ClientOs, OsProfileBuilder},
  process::collection::CollectionBudget,
};
use gphip::{ReportContext, ReportInput, generate_report_with_control};

#[derive(Args)]
pub(crate) struct HipArgs {
  #[arg(long, help = "The GP client version, e.g., 6.2.4-49")]
  client_version: String,

  #[arg(long, value_enum, help = "The client OS")]
  client_os: Os,

  #[arg(long, hide = true)]
  os_version: Option<String>,

  #[arg(long, help = "Use this runtime host ID when building the OS profile")]
  host_id: Option<String>,

  #[arg(long, help = "The authentication cookie")]
  cookie: String,

  #[arg(long, help = "The client IPv4 address")]
  client_ip: Option<String>,

  #[arg(long, help = "The client IPv6 address")]
  client_ipv6: Option<String>,

  #[arg(long, help = "The MD5 digest to encode into the HIP report")]
  md5: String,
}

pub(crate) struct HipHandler<'a> {
  args: &'a HipArgs,
}

impl<'a> HipHandler<'a> {
  pub(crate) fn new(args: &'a HipArgs) -> Self {
    Self { args }
  }

  pub(crate) async fn handle(&self) -> anyhow::Result<()> {
    let budget = CollectionBudget::new(Duration::from_secs(60));
    let client_os = ClientOs::from(self.args.client_os);
    let mut builder = OsProfileBuilder::new(client_os).client_version(self.args.client_version.clone());
    if let Some(os_version) = self.args.os_version.as_deref() {
      builder = builder.os_version(os_version);
    }
    if let Some(host_id) = self.args.host_id.as_deref() {
      builder = builder.host_id_override(host_id);
    }
    let input = ReportInput {
      profile: builder.build_with_control(&budget)?,
      context: ReportContext::Connected {
        cookie: self.args.cookie.clone(),
        client_ip: self.args.client_ip.clone(),
        client_ipv6: self.args.client_ipv6.clone(),
        md5: self.args.md5.clone(),
      },
    };
    println!("{}", generate_report_with_control(&input, &budget)?);
    Ok(())
  }
}
