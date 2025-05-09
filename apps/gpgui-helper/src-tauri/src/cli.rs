use clap::Parser;
use gpapi::{
  clap::InfoLevelVerbosity,
  utils::{base64, env_utils},
};
use log::info;

use crate::app::App;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");
const GP_API_KEY: &[u8; 32] = &[0; 32];

#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {
  #[arg(long, help = "Read the API key from stdin")]
  api_key_on_stdin: bool,

  #[arg(long, default_value = env!("CARGO_PKG_VERSION"), help = "The version of the GUI")]
  gui_version: String,

  #[command(flatten)]
  verbose: InfoLevelVerbosity,
}

impl Cli {
  fn run(&self) -> anyhow::Result<()> {
    let api_key = self.read_api_key()?;
    let app = App::new(api_key, &self.gui_version);

    env_utils::patch_gui_runtime_env(false);

    app.run()
  }

  fn read_api_key(&self) -> anyhow::Result<Vec<u8>> {
    if self.api_key_on_stdin {
      let mut api_key = String::new();
      std::io::stdin().read_line(&mut api_key)?;

      let api_key = base64::decode_to_vec(api_key.trim())?;

      Ok(api_key)
    } else {
      Ok(GP_API_KEY.to_vec())
    }
  }
}

fn init_logger(cli: &Cli) {
  env_logger::builder()
    .filter_level(cli.verbose.log_level_filter())
    .init();
}

pub fn run() {
  let cli = Cli::parse();

  init_logger(&cli);
  info!("gpgui-helper started: {}", VERSION);

  if let Err(e) = cli.run() {
    eprintln!("{}", e);
    std::process::exit(1);
  }
}
