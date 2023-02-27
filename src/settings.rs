use clap::Parser;

#[derive(Parser, Clone, Debug)]
#[clap(author, about)]
#[clap(version = crate::version())]
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
    /// If %f, %l, or %c are present in the command, they will be replaced with
    /// the filename, cursor line, and cursor column, respectively. If none are
    /// present, the filename will be appended to the command.
    #[clap(short, long, env)]
    pub editor: String,
    /// Allow multiple concurrent instances of editing command
    #[clap(short, long)]
    pub multi: bool,
    /// Shutdown after <SECONDS> with no connections
    #[clap(short, long, name = "SECONDS")]
    pub idle_timeout: Option<u64>,
    /// Buffer incoming changes for <MILLISECONDS> before updating the local file.
    ///
    /// May conflict with $EDITOR's internal debouncing. Set to 0 to disable.
    #[clap(long, name = "MILLIS", default_value = "500")]
    pub delay: u64,
    /// Serve on a listening socket passed by systemd
    ///
    /// If the socket cannot be found or used a failure will be returned.
    /// The `--port` flag must match what systemd is listening on in order to
    /// send a correct ghosttext websocket redirect message.
    ///
    /// This expects a socket configured with `Accept=no` and
    /// `ListenStream=<PORT>`.
    /// See `systemd.socket(5)`, `sd_listen_fds(3)`.
    #[clap(long)]
    #[cfg(all(feature = "systemd", target_os = "linux"))]
    pub from_systemd: bool,
}
