#[macro_use]
extern crate serde_derive;

#[macro_use]
extern crate log;
use log::LevelFilter;

use clap::Parser;

mod settings;
use settings::Settings;
mod server;
#[cfg(all(feature = "systemd", target_os = "linux"))]
mod systemd;
mod utils;

fn version() -> &'static str {
    option_env!("CARGO_GIT_VERSION")
        .or(option_env!("CARGO_PKG_VERSION"))
        .unwrap_or("unknown")
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_logger()?;

    let options = Settings::parse();

    server::run(options).await?;

    Ok(())
}

fn init_env_logger() -> anyhow::Result<()> {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .format_timestamp(None)
        // .format_module_path(false)
        .parse_default_env()
        .try_init()?;

    Ok(())
}

#[cfg(not(all(feature = "systemd", target_os = "linux")))]
fn init_logger() -> anyhow::Result<()> {
    init_env_logger()
}

#[cfg(all(feature = "systemd", target_os = "linux"))]
fn init_logger() -> anyhow::Result<()> {
    if systemd::should_init_systemd_logger() {
        systemd::init_systemd_logger()
    } else {
        init_env_logger()
    }
}
