use log::trace;

/// Convert the browser's 0-based UTF-16 offset to 1-based UTF-8 line/col cursor coordinates
pub fn utf16_offset_to_utf8_line_col(offset: usize, text: &str) -> (usize, usize) {
    // re-encode text as UTF-16
    let chars = text.encode_utf16();

    let mut line = 1;
    let mut utf16_col = 1;

    let mut last_line = Vec::new();

    for (i, c) in chars.enumerate() {
        if i < offset {
            if c == '\n' as u16 {
                line += 1;
                utf16_col = 1;
                last_line.clear();
            } else {
                utf16_col += 1;
                last_line.push(c);
            }
        } else {
            // continue reading line to buffer
            if c == '\n' as u16 {
                break;
            }
            last_line.push(c);
        }
    }

    trace!("last line: {:?}", last_line);

    // don't need to compute anything else if at beginning of last line
    if utf16_col == 1 {
        trace!("cursor at beginning of line");
        let col = 1;
        trace!("col: {}", col);
        return (line, col);
    }

    // substring of last line to cursor
    // includes all characters BEFORE browser-style cursor (pipe)
    // does not include character highlighted by terminal-style cursor (block)
    let to_cursor = &last_line[..utf16_col - 1];
    trace!("to_cursor: {:?}", to_cursor);

    // if substring to cursor is valid, it's length is the byte before the one we want the cursor to land on
    if let Ok(to_cursor) = String::from_utf16(to_cursor) {
        trace!("using valid cursor position");
        let col = to_cursor.len() + 1;
        trace!("col: {}", col);
        return (line, col);
    }

    // if the substring is invalid, we're in the middle of a bad byte
    // try to backtrack to last good byte
    trace!("cursor on bad byte");

    let decode_results: Vec<_> = std::char::decode_utf16(to_cursor.to_owned())
        .map(|r| r.map_err(|e| e.unpaired_surrogate()))
        .collect();
    trace!("decode results: {:?}", decode_results);

    // trim trailing bad bytes
    let trailing_err = decode_results
        .iter()
        .rposition(|r| r.is_ok())
        .map(|i| i + 1) // convert to 1-based
        .unwrap_or(decode_results.len());
    let decode_results = &decode_results[..trailing_err];
    trace!("cleaned decode results: {:?}", decode_results);

    let col = decode_results
        .iter()
        .map(|r| match r {
            Ok(c) => c.len_utf8(),
            // FIXME: see if this is really right for UTF16...
            Err(w) if *w < 127 => 1,
            Err(_) => 2,
        })
        .sum::<usize>()
        + 1;

    trace!("col: {}", col);

    (line, col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("asdf hjkl", 0 => (1, 1)   ; "at text beginning")]
    //           ^here
    #[test_case("asdf hjkl", 10 => (1, 10) ; "at text end")]
    //                    ^here
    #[test_case("asdf hjkl", 4 => (1, 5)   ; "in text")]
    //               ^here
    #[test_case("asdf hjkl", 12 => (1, 10) ; "past end of text")]
    //                      ^here
    #[test_case("asdf\nhjkl\nzxcv", 7 => (2, 3)  ; "on another line")]
    //                   ^here
    #[test_case("asdf 🇺🇸", 5 => (1, 6)     ; "at beginning of surrogate pair")]
    //                ^here
    #[test_case("asdf 🇺🇸", 9 => (1, 14)    ; "at end of surrogate pair")]
    //                  ^here
    #[test_case("asdf 🇺🇸", 6 => (1, 6)     ; "in middle of surrogate pair")]
    //                ^here
    #[test_log::test]
    fn offset_conversions(text: &str, offset: usize) -> (usize, usize) {
        utf16_offset_to_utf8_line_col(offset, text)
    }
}
