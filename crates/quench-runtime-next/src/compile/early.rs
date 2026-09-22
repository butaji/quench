pub(super) fn strict_arguments_early_error(source: &str) -> bool {
    let masked = mask_literals_and_comments(source);
    let bytes = masked.as_bytes();
    let mut index = 0;
    while index + 9 <= bytes.len() {
        if bytes[index..].starts_with(b"arguments")
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
            && (index + 9 == bytes.len() || !bytes[index + 9].is_ascii_alphanumeric())
        {
            let mut cursor = index + 9;
            while matches!(bytes.get(cursor), Some(b' ' | b'\t' | b'\n' | b'\r')) {
                cursor += 1;
            }
            if matches!(
                bytes.get(cursor),
                Some(b'=') | Some(b'+') | Some(b'-') | Some(b'*') | Some(b'/')
            ) || source[index.saturating_sub(7)..index].contains("delete")
            {
                return true;
            }
        }
        index += 1;
    }
    false
}

fn mask_literals_and_comments(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut masked = bytes.to_vec();
    let mut quote = None;
    let mut index = 0;
    while index < bytes.len() {
        if let Some(delimiter) = quote {
            if bytes[index] == b'\\' && index + 1 < bytes.len() {
                masked[index] = b' ';
                masked[index + 1] = b' ';
                index += 2;
                continue;
            }
            if bytes[index] == delimiter {
                quote = None;
            } else if !matches!(bytes[index], b'\n' | b'\r') {
                masked[index] = b' ';
            }
            index += 1;
            continue;
        }
        if matches!(bytes[index], b'\'' | b'"' | b'`') {
            quote = Some(bytes[index]);
            masked[index] = b' ';
            index += 1;
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'/') {
            masked[index] = b' ';
            masked[index + 1] = b' ';
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                masked[index] = b' ';
                index += 1;
            }
            continue;
        }
        if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
            masked[index] = b' ';
            masked[index + 1] = b' ';
            index += 2;
            while index + 1 < bytes.len() {
                if bytes[index] == b'*' && bytes[index + 1] == b'/' {
                    masked[index] = b' ';
                    masked[index + 1] = b' ';
                    index += 2;
                    break;
                }
                if !matches!(bytes[index], b'\n' | b'\r') {
                    masked[index] = b' ';
                }
                index += 1;
            }
            continue;
        }
        index += 1;
    }
    String::from_utf8(masked).unwrap_or_default()
}
