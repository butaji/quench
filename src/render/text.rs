//! Text utilities for rendering

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Calculate the visible width of text, stripping ANSI codes
pub fn ansi_width(text: &str) -> usize {
    // Quick path: no ANSI escapes - use unicode-width directly
    if !text.as_bytes().contains(&0x1B) {
        return text.width();
    }

    let bytes = text.as_bytes();
    let mut width = 0;
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1B {
            // ANSI escape sequence - skip it
            let mut seq_len = 1;
            while i + seq_len < bytes.len()
                && !b"mHABCDdefghijklmnopqrstuvwxyz@ABCDEFGHIJKLMNOPQRSTUVWXYZ"
                    .contains(&bytes[i + seq_len])
            {
                seq_len += 1;
            }
            if i + seq_len < bytes.len() {
                seq_len += 1;
            }
            i += seq_len;
        } else if bytes[i] >= 0xC0 {
            // Multi-byte UTF-8 - decode char and get its width
            if let Some(ch) = text[i..].chars().next() {
                width += UnicodeWidthChar::width(ch).unwrap_or(1);
                i += ch.len_utf8();
            } else {
                width += 1;
                i += 1;
            }
        } else {
            width += 1;
            i += 1;
        }
    }

    width
}

/// Get ANSI sequence length
fn get_ansi_seq_len(bytes: &[u8], i: usize) -> usize {
    let mut seq_len = 1;
    while i + seq_len < bytes.len()
        && !b"mHABCDdefghijklmnopqrstuvwxyz@ABCDEFGHIJKLMNOPQRSTUVWXYZ"
            .contains(&bytes[i + seq_len])
    {
        seq_len += 1;
    }
    seq_len + 1
}

/// Get UTF-8 char width and byte length
fn get_char_info(bytes: &[u8], i: usize) -> (usize, usize) {
    use unicode_width::UnicodeWidthChar;
    // We need to decode from the original string, but bytes is indexed.
    // For non-ASCII, decode the UTF-8 sequence.
    if bytes[i] < 0x80 {
        return (1, 1);
    }
    // Decode UTF-8 char starting at byte i
    let s = std::str::from_utf8(&bytes[i..]).unwrap_or("");
    if let Some(ch) = s.chars().next() {
        (UnicodeWidthChar::width(ch).unwrap_or(1), ch.len_utf8())
    } else {
        (1, 1)
    }
}

/// Truncate text to fit within max_width characters
pub fn truncate_text(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let visible_width = ansi_width(text);
    if visible_width <= max_width {
        return text.to_string();
    }

    let ellipsis = "…";
    let ellipsis_width = ansi_width(ellipsis);
    if max_width < ellipsis_width {
        return String::new();
    }

    let available = max_width - ellipsis_width;
    let (result, _) = append_chars_up_to_width(text, available);
    result + ellipsis
}

fn append_chars_up_to_width(text: &str, max_width: usize) -> (String, usize) {
    let mut result = String::new();
    let mut current_width = 0;
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() && current_width < max_width {
        let (char_width, char_len) = if bytes[i] == 0x1B {
            (0, get_ansi_seq_len(bytes, i))
        } else {
            get_char_info(bytes, i)
        };

        if current_width + char_width <= max_width {
            result.push_str(&text[i..i + char_len]);
            current_width += char_width;
        }
        i += char_len;
    }

    (result, current_width)
}
