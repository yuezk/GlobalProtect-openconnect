//! Generate native Windows posture on Windows, or a Windows emulation preview elsewhere.
use gpapi::os_profile::{ClientOs, OsProfile};
use gphip::{ReportContext, ReportInput};

fn main() -> anyhow::Result<()> {
  let input = ReportInput {
    profile: gpapi::os_profile::desktop::collect_profile(OsProfile::builder(ClientOs::Windows), None),
    context: ReportContext::Preview,
  };
  println!("{}", gphip::generate_report(&input)?);
  Ok(())
}
