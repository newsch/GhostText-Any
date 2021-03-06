#[macro_use]
extern crate log;
use log::LevelFilter;

#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate structopt;

mod options;
use options::Options;

mod server;
mod ws_messages;

use structopt::StructOpt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::builder()
        .filter(Some("gtany"), LevelFilter::Info)
        .format_timestamp(None)
        .format_module_path(false)
        .try_init()?;

    let options = Options::from_args();

    server::run(options).await?;

    Ok(())
}
