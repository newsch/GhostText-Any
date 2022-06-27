use log::LevelFilter;

#[macro_use]
extern crate serde_derive;

use clap::Parser;

mod settings;
use settings::Settings;
mod server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .format_timestamp(None)
        // .format_module_path(false)
        .parse_default_env()
        .try_init()?;

    let options = Settings::parse();

    server::run(options).await?;

    Ok(())
}
