/// Convert the browser's 0-based UTF-16 offset to 1-based UTF-8 line/col cursor coordinates
pub fn utf16_offset_to_utf8_line_col(offset: usize, text: &str) -> (usize, usize) {
    // - the ascii range (`0x00` - `0x7F`) counts the same (1:1)
    // - `0x0000` - `0xD7FF` is 1 code unit in UTF-16, but can be 1-3 in UTF-8
    // - `0x10000` - `0x10FFFF` is 2 code units in UTF-16, but 4 in UTF-8
    // See <https://en.wikipedia.org/wiki/UTF-8> and <https://en.wikipedia.org/wiki/UTF-16>
    // TODO: don't land in middle of graphemes?

    let mut line = 1;
    let mut utf8_col = 1;
    let mut utf16_offset = 0;

    for c in text.chars() {
        // Returns the index of the previous char if byte index points to the middle of a code point.
        utf16_offset += c.len_utf16();
        if utf16_offset > offset {
            break;
        }

        if c == '\n' { // TODO: any unicode line break?
            line += 1;
            utf8_col = 1;
        } else {
            utf8_col += c.len_utf8();
        }
    }

    (line, utf8_col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    #[test_case("asdf hjkl", 0 => (1, 1)               ; "at text beginning")]
    //           ^here
    #[test_case("asdf hjkl", 10 => (1, 10)             ; "at text end")]
    //                    ^here
    #[test_case("asdf hjkl", 4 => (1, 5)               ; "in text")]
    //               ^here
    #[test_case("asdf hjkl", 12 => (1, 10)             ; "past end of text")]
    //                      ^here
    #[test_case("asdf\nhjkl\nzxcv", 7 => (2, 3)        ; "on another line")]
    //                   ^here
    #[test_case("asdf 🇺🇸", 5 => (1, 6)                 ; "at beginning of surrogate pair")]
    //                ^here
    #[test_case("asdf 🇺🇸", 9 => (1, 14)                ; "at end of surrogate pair")]
    //                  ^here
    #[test_case("asdf 🇺🇸", 6 => (1, 6)                 ; "in middle of surrogate pair")]
    //                ^here
    #[test_case("àsdf hjkl", 1 => (1, 3)               ; "right after 2-byte UTF-8 sequence")]
    //            ^here
    #[test_case("àsdf hjkl", 4 => (1, 6)               ; "after 2-byte UTF-8 sequence")]
    //               ^here
    #[test_case("linear A: 𐘗 (U+10617)", 10 => (1, 11) ; "right before UTF-16 surrogate pair")]
    //                     ^here
    #[test_case("linear A: 𐘗 (U+10617)", 11 => (1, 11) ; "in middle of UTF-16 surrogate pair")]
    //                     ^here
    #[test_case("linear A: 𐘗 (U+10617)", 12 => (1, 15) ; "right after UTF-16 surrogate pair")]
    //                      ^here
    #[test_case("linear A: 𐘗 (U+10617)", 14 => (1, 17) ; "after UTF-16 surrogate pair")]
    //                        ^here
    fn offset_conversions(text: &str, offset: usize) -> (usize, usize) {
        utf16_offset_to_utf8_line_col(offset, text)
    }
}
