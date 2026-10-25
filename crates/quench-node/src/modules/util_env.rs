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

/// Apply variables from startup `--env-file` options before Node's process
/// surface is initialized. Existing host variables keep precedence, while
/// later files override values from earlier files.
pub(crate) fn load_startup_env_files(
    node_options: Option<&str>,
    arguments: &[String],
    cwd: &std::path::Path,
) {
    let mut options = node_options
        .and_then(|value| crate::modules::cli_options::parse_node_options(value).ok())
        .unwrap_or_default();
    options.extend(arguments.iter().cloned());

    let inherited: std::collections::HashSet<String> = std::env::vars_os()
        .filter_map(|(key, _)| key.into_string().ok())
        .collect();
    let mut loaded = BTreeMap::new();
    let mut index = 0;
    while index < options.len() {
        let argument = &options[index];
        if argument == "--" {
            break;
        }
        let mut path = None;
        if let Some(value) = argument.strip_prefix("--env-file=") {
            path = Some(value.to_owned());
        } else if let Some(value) = argument.strip_prefix("--env-file-if-exists=") {
            path = Some(value.to_owned());
        } else if argument == "--env-file" || argument == "--env-file-if-exists" {
            if let Some(value) = options.get(index + 1) {
                path = Some(value.clone());
                index += 1;
            }
        }
        if let Some(path) = path {
            let requested = std::path::Path::new(&path);
            let path = if requested.is_absolute() {
                requested.to_path_buf()
            } else {
                cwd.join(requested)
            };
            if let Ok(contents) = std::fs::read_to_string(path) {
                loaded.extend(parse_env_file(&contents));
            }
        }
        index += 1;
    }

    for (key, value) in loaded {
        if inherited.contains(&key) {
            continue;
        }
        // SAFETY: startup happens synchronously before guest code observes
        // process.env or starts child processes.
        unsafe { std::env::set_var(key, value) };
    }
}
