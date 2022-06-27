use std::{env, os::unix};

use log::debug;
use tokio_stream::wrappers::UnixListenerStream;

/// Try to get a listener socket passed by systemd.
///
/// This function should only be called once.
pub fn try_get_socket() -> anyhow::Result<UnixListenerStream> {
    const START_FD: usize = 3; // SD_LISTEN_FDS_START, see sd_listen_fds(3)
    const LISTEN_PID: &str = "LISTEN_PID";
    const LISTEN_FD_NAMES: &str = "LISTEN_FD_NAMES";
    const LISTEN_FDS: &str = "LISTEN_FDS";

    debug!("LISTEN_PID={:?}", env::var_os(LISTEN_PID));
    debug!("LISTEN_FD_NAMES={:?}", env::var_os(LISTEN_FD_NAMES));
    debug!("LISTEN_FDS={:?}", env::var_os(LISTEN_FDS));

    let num_fds: usize = env::var(LISTEN_FDS)?.parse()?;
    if num_fds > 1 {
        anyhow::bail!("More than one systemd socket file descriptor present");
    } else if num_fds == 0 {
        anyhow::bail!("No systemd socket file descriptors present");
    }

    // only one socket
    let fd = START_FD as unix::io::RawFd;

    // turn fd into std UnixListener
    // Safety: only called once, environment variables are removed to prevent reuse
    use unix::io::FromRawFd;
    let listener = unsafe { unix::net::UnixListener::from_raw_fd(fd) };
    listener.set_nonblocking(true)?;

    // convert to tokio UnixListenerStream
    let listener = tokio::net::UnixListener::from_std(listener)?;
    let listener_stream = UnixListenerStream::new(listener);

    // Remove environment variables to prevent reuse
    env::remove_var(LISTEN_PID);
    env::remove_var(LISTEN_FD_NAMES);
    env::remove_var(LISTEN_FDS);

    Ok(listener_stream)
}
