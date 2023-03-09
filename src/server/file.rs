use std::{
    io::{self},
    path::{Path, PathBuf},
    time::SystemTime,
};

use sha2::{Digest, Sha256};
use tempdir::TempDir;
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

use super::msg;

#[cfg(feature = "watch_changes")]
pub use super::watch_changes::watch_edits;

/// A mock that returns an empty stream
#[cfg(not(feature = "watch_changes"))]
pub fn watch_edits(_path: impl AsRef<Path>) -> anyhow::Result<impl futures::Stream<Item = ()>> {
    Ok(tokio_stream::empty())
}

// Tradeoff:
// - trailing newlines don't keep appearing in the web editor unless written there
// - adding a trailing newline on the web will not update the local side
/// Path will never change, can be copied and used elsewhere.
pub struct LocalFile {
    path: PathBuf,
    // deletes directory when dropped
    _tempdir: TempDir,
    /// Last edit time hash is valid for
    last_edit: SystemTime,
    /// hash of the local content, with trailing newline removed
    hash: [u8; 32],
}

// public interface
impl LocalFile {
    pub async fn create(m: &msg::GetTextFromComponent) -> io::Result<Self> {
        let tempdir = TempDir::new("ghost-text")?;
        let mut path = PathBuf::from(tempdir.path());
        path.set_file_name(get_filename(m));

        let mut s = Self {
            path,
            _tempdir: tempdir,
            last_edit: SystemTime::now(),
            hash: [0; 32],
        };

        debug!("Creating file at: {:?}", s.path);
        s.write(m).await?;

        Ok(s)
    }

    pub async fn get_current_contents(&mut self) -> io::Result<String> {
        self.read().await
    }

    pub async fn maybe_update(&mut self, m: &msg::GetTextFromComponent) -> io::Result<bool> {
        if self.is_equivalent(m).await? {
            debug!("Remote copy is equivalent to local, ignoring update");
            return Ok(false);
        }
        debug!("Updating local copy");
        self.write(m).await?;

        Ok(true)
    }
}

impl AsRef<Path> for LocalFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl LocalFile {
    async fn write(&mut self, m: &msg::GetTextFromComponent) -> io::Result<()> {
        let mut f = File::create(&self).await?;
        f.write_all(m.text.as_bytes()).await?;
        f.write_all(&[b'\n']).await?;

        self.update_local_md(&mut f, &m.text).await?;

        Ok(())
    }

    async fn read(&mut self) -> io::Result<String> {
        let mut f = File::open(&self).await?;
        let mut text = String::new();
        f.read_to_string(&mut text).await?;
        if text.ends_with('\n') {
            text.pop();
        }
        self.update_local_md(&mut f, &text).await?;
        Ok(text)
    }

    async fn update_local_md(&mut self, f: &mut File, text: &str) -> io::Result<()> {
        self.last_edit = get_last_modification(f).await?;
        self.hash = calculate_hash(&text);
        Ok(())
    }

    async fn is_equivalent(&self, m: &msg::GetTextFromComponent) -> io::Result<bool> {
        let remote_hash = calculate_hash(&m.text);
        Ok(
            self.last_edit == get_last_modification(&mut File::open(&self).await?).await?
                && remote_hash == self.hash,
        )
    }
}

fn calculate_hash<T: AsRef<[u8]>>(t: &T) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(t);
    s.finalize().try_into().expect("Sha256 output is 32 bytes")
}

async fn get_last_modification(f: &mut File) -> io::Result<SystemTime> {
    f.metadata().await.and_then(|m| m.modified())
}

fn get_filename(msg: &msg::GetTextFromComponent) -> String {
    const BAD_CHARS: &[char] = &[' ', '/', '\\', '\r', '\n', '\t'];

    let extension = determine_file_extension(msg);

    let mut title = msg.title.as_str();

    let file_name = if title.is_empty() {
        String::from("buffer")
    } else {
        if title.len() > 16 {
            if let Some((i, _c)) = title.char_indices().nth(16) {
                title = &title[..i];
            }
        }
        title.replace(BAD_CHARS, "-")
    } + "."
        + extension;

    file_name
}

fn determine_file_extension(msg: &msg::GetTextFromComponent) -> &str {
    use url::{Host, Url};

    const MARKDOWN: &str = "md";
    const PLAINTEXT: &str = "txt";
    const DEFAULT: &str = PLAINTEXT;

    let source_url = match Url::parse(&msg.url) {
        Ok(u) => u,
        Err(e) => {
            debug!("Error parsing source url {:?}: {e}", msg.url);
            return DEFAULT;
        }
    };

    let domain = match source_url.host() {
        Some(Host::Domain(d)) => d,
        _ => return DEFAULT,
    };

    match &domain.split('.').collect::<Vec<_>>()[..] {
        [.., "github", "com"] | [.., "gitlab", "com"] | [.., "codeberg", "org"] => MARKDOWN,
        _ => DEFAULT,
    }
}
