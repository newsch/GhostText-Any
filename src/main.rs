#[macro_use]
extern crate log;
#[macro_use]
extern crate serde_derive;
#[macro_use]
extern crate structopt;

mod options;
use options::Options;

mod server;
mod ws_messages;

use std::error::Error;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init()?;

    let options = Options::from_args();

    server::run(options).await?;

    Ok(())
}
