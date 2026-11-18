use clap::Parser;
use gpapi::clap::InfoLevelVerbosity;
#[derive(Parser)]
#[command(version = crate::VERSION)]
struct Cli {
  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}
pub fn run() -> anyhow::Result<()> {
  let cli = Cli::parse();
  crate::windows_service::dispatch(cli.verbose.log_level_filter().to_level().unwrap_or(log::Level::Info))
}
