#[derive(StructOpt, Clone, Debug)]
pub struct Options {
    /// Port to listen on
    #[structopt(short, long, default_value = "4001")]
    pub port: u16,
    #[structopt(short, long, env)]
    pub editor: String,
}
