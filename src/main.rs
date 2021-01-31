use std::error::Error;
use structopt::StructOpt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init()?;

    let options = ghost_text_file::Options::from_args();

    ghost_text_file::server::run(options).await?;

    Ok(())
}
