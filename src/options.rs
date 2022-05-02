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
    /// Shutdown after <SECONDS> with no connections
    #[structopt(short, long, name = "SECONDS")]
    pub idle_timeout: Option<u64>,
    /// Serve on a listening socket passed by systemd
    ///
    /// If the socket cannot be found or used a failure will be returned.
    /// The `--port` flag must match what systemd is listening on in order to
    /// send a correct ghosttext websocket redirect message.
    /// This expects a socket configured with `Accept=no` and
    /// `ListenStream=<PORT>`.
    /// See `systemd.socket(5)`, `sd_listen_fds(3)`.
    #[structopt(long)]
    pub from_systemd: bool,
}
