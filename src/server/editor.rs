use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use log::{debug, error, info};
use tokio::process::Command;

use super::msg;
use super::text::utf16_offset_to_utf8_line_col;
use super::Settings;

/// Returns on process exit
pub async fn spawn_editor(
    options: &Settings,
    file_path: &Path,
    msg: &msg::GetTextFromComponent,
) -> anyhow::Result<()> {
    info!("New session from: {:?}", msg.title);

    let file_path = file_path
        .to_str()
        .expect("Internally created file paths should be safe UTF-8");

    let (line, col) = msg
        .selections
        .get(0)
        .map(|s| utf16_offset_to_utf8_line_col(s.start, &msg.text))
        .unwrap_or((1, 1));

    let mut pieces =
        shell_words::split(&options.editor).context("Could not parse editor command")?;

    if pieces.is_empty() {
        bail!("Empty editor command");
    }

    perform_substitutions(&mut pieces, file_path, line, col);

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

/// Add filename, cursor line, and cursor column to the command
fn perform_substitutions(command: &mut Vec<String>, file_path: &str, line: usize, col: usize) {
    const FILE: &str = "%f";
    const LINE: &str = "%l";
    const COLUMN: &str = "%c";

    if command
        .iter()
        .skip(1)
        .any(|s| s.contains(FILE) || s.contains(LINE) || s.contains(COLUMN))
    {
        for s in command.iter_mut().skip(1) {
            replace_in_place(s, FILE, file_path);
            replace_in_place(s, LINE, &line.to_string());
            replace_in_place(s, COLUMN, &col.to_string());
        }
        return;
    }

    let editor = &command[command.len() - 1];
    if let Some(mut additions) = format_known_editors(editor, file_path, line, col) {
        debug!("Recognized editor {editor:?}: adding {additions:?}");
        command.append(&mut additions);
        return;
    }

    command.push(file_path.to_string());
}

fn replace_in_place(source: &mut String, pattern: &str, replacement: &str) -> bool {
    let start = match source.find(pattern) {
        None => return false,
        Some(s) => s,
    };

    source.replace_range(start..(start + pattern.len()), replacement);
    true
}

/// Format filepath, cursor position, and other flags for known editors.
///
/// Based on fish-shell's edit_command_buffer function, see <https://github.com/fish-shell/fish-shell/blob/3.5.0/share/functions/edit_command_buffer.fish#L45=>.
fn format_known_editors(editor: &str, file: &str, line: usize, col: usize) -> Option<Vec<String>> {
    use std::format as f;
    Some(match editor {
        "vi" | "vim" | "nvim" => vec![f!("+{line}"), f!("+norm! {col}|"), file.to_string()],
        "emacs" | "emacsclient" | "gedit" | "kak" => vec![f!("+{line}:{col}"), file.to_string()],
        "nano" => vec![f!("+{line},{col}"), file.to_string()],
        "joe" | "ee" => vec![f!("+{line}"), file.to_string()],
        "code" | "code-oss" => vec![
            "--goto".to_string(),
            f!("{file}:{line}:{col}"),
            "--wait".to_string(),
        ],
        "subl" => vec![f!("{file}:{line}:{col}"), "--wait".to_string()],
        "micro" => vec![file.to_string(), f!("+{line}:{col}")],
        _ => return None,
    })
}
