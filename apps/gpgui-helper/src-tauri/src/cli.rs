use base64::prelude::*;
use clap::Parser;
use log::{info, LevelFilter};

use crate::app::App;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", compile_time::date_str!(), ")");
const GP_API_KEY: &[u8; 32] = &[0; 32];

#[derive(Parser)]
#[command(version = VERSION)]
struct Cli {}

impl Cli {
  fn run(&self) -> anyhow::Result<()> {
    #[cfg(debug_assertions)]
    let api_key = GP_API_KEY.to_vec();
    #[cfg(not(debug_assertions))]
    let api_key = self.read_api_key()?;

    let app = App::new(api_key);

    app.run()
  }

  fn read_api_key(&self) -> anyhow::Result<Vec<u8>> {
    let mut api_key = String::new();
    std::io::stdin().read_line(&mut api_key)?;

    let api_key = BASE64_STANDARD.decode(api_key.trim())?;

    Ok(api_key)
  }
}

fn init_logger() {
  env_logger::builder().filter_level(LevelFilter::Info).init();
}

pub fn run() {
  let cli = Cli::parse();

  init_logger();
  info!("gpgui-helper started: {}", VERSION);

  if let Err(e) = cli.run() {
    eprintln!("{}", e);
    std::process::exit(1);
  }
}
