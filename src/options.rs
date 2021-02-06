#[derive(StructOpt, Clone, Debug)]
#[structopt(about)]
pub struct Options {
    /// Port to listen on
    #[structopt(short, long, default_value = "4001")]
    pub port: u16,
    /// Host to bind to
    #[structopt(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Command to run with the received file
    ///
    /// Once the command completes, the contents of the file will be sent to the
    /// browser, the connection closed, and the file deleted.
    #[structopt(short, long, env)]
    pub editor: String,
    /// Allow multiple concurrent instances of editing command
    #[structopt(short, long)]
    pub multi: bool,
}
