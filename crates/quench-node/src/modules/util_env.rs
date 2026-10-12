//! Shared dotenv parsing for util.parseEnv and process.loadEnvFile.
use std::collections::BTreeMap;

fn trim(content: &str) -> &str {
    content.trim_matches([' ', '\t', '\n'])
}

pub(crate) fn parse_env_file(input: &str) -> BTreeMap<String, String> {
    let normalized = input.replace('\r', "");
    let mut content = trim(&normalized);
    let mut values = BTreeMap::new();
    while !content.is_empty() {
        if content.starts_with(['\n', '#']) {
            content = content.find('\n').map_or("", |index| &content[index + 1..]);
            continue;
        }
        let Some(separator) = content.find(['=', '\n']) else {
            break;
        };
        if content.as_bytes()[separator] == b'\n' {
            content = trim(&content[separator + 1..]);
            continue;
        }
        let mut key = trim(&content[..separator]);
        content = &content[separator + 1..];
        if content.is_empty() || content.starts_with('\n') {
            values.insert(key.to_owned(), String::new());
            continue;
        }
        content = trim(content);
        if key.is_empty() {
            continue;
        }
        if let Some(exported) = key.strip_prefix("export ") {
            key = trim(exported);
        }
        if content.is_empty() {
            values.insert(key.to_owned(), String::new());
            break;
        }
        let quote = content.as_bytes()[0];
        if matches!(quote, b'\'' | b'"' | b'`') {
            if let Some(closing) = content[1..].find(quote as char).map(|index| index + 1) {
                let value = &content[1..closing];
                values.insert(
                    key.to_owned(),
                    if quote == b'"' {
                        value.replace("\\n", "\n")
                    } else {
                        value.to_owned()
                    },
                );
                content = content[closing + 1..]
                    .find('\n')
                    .map_or("", |index| &content[closing + index + 2..]);
                continue;
            }
            let newline = content.find('\n').unwrap_or(content.len());
            values.insert(key.to_owned(), content[..newline].to_owned());
            content = content.get(newline + 1..).unwrap_or("");
        } else {
            let newline = content.find('\n').unwrap_or(content.len());
            let line = &content[..newline];
            let value = line.split_once('#').map_or(line, |(value, _)| value);
            values.insert(key.to_owned(), trim(value).to_owned());
            content = content.get(newline + 1..).unwrap_or("");
        }
        content = trim(content);
    }
    values
}
