#[derive(StructOpt)]
pub struct Options {
    #[structopt(short = "p", long = "port", help = "The port to listen to", default_value = "4001")]
    pub port: u16,
}
