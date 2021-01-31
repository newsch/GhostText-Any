use std::fs;
use std::io::prelude::*;
use std::process::Command;

use tempdir::TempDir;

fn process_title(title: &String) -> String {
    const BAD_CHARS: &[char] = &['/', '\\'];

    let file_name = if title.is_empty() {
        String::from("buffer")
    } else {
        title.replace(BAD_CHARS, "-")
    } + ".txt";

    file_name
}

pub fn run(url: String, title: String, text: String) -> String {
    debug!("New edit {:?} for: {}", title, url);
    let tempdir = TempDir::new("ghost-text").unwrap();
    // TODO: watch for edits to file and change then, overwrite file on incoming edits

    let file_name = process_title(&title);
    let file_path = tempdir.path().join(file_name);

    info!("New edit at: {:?}", file_path);
    {
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.write(&['\n' as u8]).unwrap();
    }

    debug!("Opening editor for {:?}", file_path);
    Command::new("x-terminal-emulator")
        .arg("-e")
        .arg("kak")
        .arg(&file_path)
        .env("GHOST_TEXT_URL", &url)
        .env("GHOST_TEXT_TITLE", &title)
        .spawn()
        .unwrap()
        .wait()
        .unwrap();

    {
        let mut file = fs::File::open(&file_path).unwrap();
        let mut result = String::new();
        file.read_to_string(&mut result).unwrap();
        if result.ends_with("\n") {
            result.pop();
        }
        result
    }
}
