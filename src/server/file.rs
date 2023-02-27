use std::{
    fs::File,
    io::{self, Write},
    path::{Path, PathBuf},
};

use tokio::fs;

use super::msg;

#[cfg(feature = "watch_changes")]
pub use super::watch_changes::watch_edits;

/// A mock that returns an empty stream
#[cfg(not(feature = "watch_changes"))]
pub fn watch_edits(_path: &Path) -> anyhow::Result<impl futures::Stream<Item = ()>> {
    Ok(tokio_stream::empty())
}

pub fn get_new_path(dir: &Path, msg: &msg::GetTextFromComponent) -> io::Result<PathBuf> {
    let file_name = get_filename(msg);
    let file_path = dir.join(file_name);
    Ok(file_path)
}

pub fn replace_contents(path: &Path, msg: &msg::GetTextFromComponent) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(msg.text.as_bytes())?;
    if !msg.text.ends_with('\n') {
        file.write_all(&[b'\n'])?;
    }
    Ok(())
}

pub async fn get_current_contents(file_path: &Path) -> io::Result<String> {
    let mut text = fs::read_to_string(file_path).await?;

    if text.ends_with('\n') {
        text.pop();
    }

    Ok(text)
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
