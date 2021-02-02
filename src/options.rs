#[derive(StructOpt, Clone, Debug)]
pub struct Options {
    /// Port to listen on
    #[structopt(short, long, default_value = "4001")]
    pub port: u16,
    /// Command to run with the received file
    #[structopt(short, long, env)]
    pub editor: String,
    /// Allow many concurrent instances of editing command
    #[structopt(short, long)]
    pub many: bool,
}
