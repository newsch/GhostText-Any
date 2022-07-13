use std::{env, os::unix};

use log::{debug, LevelFilter, Log, Metadata, Record};
use systemd_journal_logger::{connected_to_journal, JournalLog};
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

struct SystemdEnvLogger {
    filter: env_logger::filter::Filter,
    inner: JournalLog<&'static str, &'static str>,
}

impl SystemdEnvLogger {
    fn new() -> Self {
        use env_logger::filter::Builder;

        let filter = Builder::new()
            .filter_level(LevelFilter::Info)
            .parse(&env::var(env_logger::DEFAULT_FILTER_ENV).unwrap_or_default())
            .build();

        let inner = JournalLog::with_extra_fields(vec![("VERSION", crate::version())]);

        log::set_max_level(filter.filter());

        Self { filter, inner }
    }
}

impl Log for SystemdEnvLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.filter.matches(record) {
            self.inner.log(record);
        }
    }

    fn flush(&self) {
        self.inner.flush();
    }
}

pub fn should_init_systemd_logger() -> bool {
    connected_to_journal()
}

pub fn init_systemd_logger() -> anyhow::Result<()> {
    log::set_boxed_logger(Box::new(SystemdEnvLogger::new()))?;

    Ok(())
}
