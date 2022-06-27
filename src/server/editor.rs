use std::path::PathBuf;

use anyhow::bail;
use anyhow::Context;
use tokio::process::Command;

use crate::options::Options;
use crate::server::text::utf16_offset_to_utf8_line_col;

use super::msg;

/// Returns on process exit
pub async fn spawn_editor(
    options: &Options,
    file_path: &PathBuf,
    msg: &msg::GetTextFromComponent,
) -> anyhow::Result<()> {
    info!("New session from: {:?}", msg.title);

    // TODO: fix this up
    let file_path = file_path.to_str().expect("file path should be good");

    let (line, col) = msg
        .selections
        .get(0)
        .map(|s| utf16_offset_to_utf8_line_col(s.start, &msg.text))
        .unwrap_or((1, 1));

    let mut pieces =
        shell_words::split(&options.editor).context("Could not parse editor command")?;

    let mut did_replace_file = false;

    for s in pieces.iter_mut().skip(1) {
        replace_in_place(s, "%l", &line.to_string());
        replace_in_place(s, "%c", &col.to_string());
        if replace_in_place(s, "%f", &file_path) {
            did_replace_file = true;
        }
    }

    if pieces.is_empty() {
        bail!("Empty editor command");
    }

    if !did_replace_file {
        pieces.push(file_path.to_string());
    }

    let program = &pieces[0];

    let args = &pieces[1..];

    debug!("Opening editor {:?}", pieces);

    let exit_status = Command::new(program)
        .args(args)
        .env("GHOST_TEXT_URL", &msg.url)
        .env("GHOST_TEXT_TITLE", &msg.title)
        .spawn()?
        .wait()
        .await?;

    if !exit_status.success() {
        error!("Editor process exited with status: {}", exit_status);
    }

    Ok(())
}

fn replace_in_place(source: &mut String, pattern: &str, replacement: &str) -> bool {
    let start = match source.find(pattern) {
        None => return false,
        Some(s) => s,
    };

    source.replace_range(start..(start + pattern.len()), replacement);
    return true;
}
