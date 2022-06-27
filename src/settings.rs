use clap::Parser;

#[derive(Parser, Clone, Debug)]
#[clap(about)]
pub struct Settings {
    /// Port to listen on
    #[clap(short, long, default_value = "4001")]
    pub port: u16,
    /// Host to bind to
    #[clap(long, default_value = "127.0.0.1")]
    pub host: String,
    /// Command to run with the received file
    ///
    /// Defaults to the value of $EDITOR.
    ///
    /// Once the command completes, the contents of the file will be sent to the
    /// browser, the connection closed, and the file deleted.
    ///
    /// If %f is present in the command, it will be replaced with the filename,
    /// otherwise the filename will be appended to the command.
    /// If %l or %c are present in the command, they will be replaced with the
    /// line and column, respectively, of the browser's cursor.
    #[clap(short, long, env)]
    pub editor: String,
    /// Allow multiple concurrent instances of editing command
    #[clap(short, long)]
    pub multi: bool,
    /// Shutdown after <SECONDS> with no connections
    #[clap(short, long, name = "SECONDS")]
    pub idle_timeout: Option<u64>,
    /// Serve on a listening socket passed by systemd
    ///
    /// If the socket cannot be found or used a failure will be returned.
    /// The `--port` flag must match what systemd is listening on in order to
    /// send a correct ghosttext websocket redirect message.
    /// This expects a socket configured with `Accept=no` and
    /// `ListenStream=<PORT>`.
    /// See `systemd.socket(5)`, `sd_listen_fds(3)`.
    #[clap(long)]
    pub from_systemd: bool,
}
