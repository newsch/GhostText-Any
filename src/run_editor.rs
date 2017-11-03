use std::io::prelude::*;
use std::fs;
use std::process::Command;

use tempdir::TempDir;

pub fn run(url: String, title: String, text: String) -> String {
    let tempdir = TempDir::new("ghost-text").unwrap();

    let file_path = tempdir.path().join("buffer.txt");
    {
        let mut file = fs::File::create(&file_path).unwrap();
        file.write_all(text.as_bytes()).unwrap();
        file.write(&['\n' as u8]).unwrap();
    }

    debug!("Opening editor for {:?}", file_path);
    Command::new("nvim-qt")
        .arg("--nofork")
        .arg("--")
        .arg(&file_path)
        .env("GHOST_TEXT_URL", &url)
        .env("GHOST_TEXT_TITLE", &title)
        .spawn().unwrap().wait().unwrap();

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
