fn path_arg(arguments: &[Value], index: usize) -> Result<&str, VmError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value),
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "path must be a string",
        ))),
    }
}

fn path_value(arguments: &[Value], index: usize) -> Result<String, VmError> {
    match arguments.get(index) {
        Some(Value::String(value)) => Ok(value.clone()),
        Some(Value::Object(_)) => {
            let href =
                quench_runtime::execute::get_property_result(arguments.get(index).unwrap(), "href")
                    .ok()
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
                    .ok_or_else(|| {
                        VmError::Thrown(fs_error(
                            "ERR_INVALID_ARG_TYPE",
                            "path must be a string or URL",
                        ))
                    })?;
            Ok(href.strip_prefix("file://").unwrap_or(&href).to_owned())
        }
        _ => Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "path must be a string or URL",
        ))),
    }
}

fn encode_file_path(path: &str) -> String {
    let mut encoded = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"-._/&=:;".contains(byte) {
            encoded.push(*byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn decode_file_url(value: &str) -> String {
    value
        .strip_prefix("file://")
        .unwrap_or(value)
        .replace("%20", " ")
        .replace("%23", "#")
        .replace("%5C", "\\")
        .replace("%5c", "\\")
}

fn path_win_basename(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?.trim_end_matches(['\\', '/']);
    let mut value = value
        .rsplit(['\\', '/'])
        .next()
        .unwrap_or(value)
        .to_string();
    if let Some(suffix) = arguments.get(1) {
        let Value::String(suffix) = suffix else {
            return Err(VmError::Thrown(fs_error(
                "ERR_INVALID_ARG_TYPE",
                "suffix must be a string",
            )));
        };
        if value.ends_with(suffix) {
            value.truncate(value.len() - suffix.len());
        }
    }
    Ok(Value::String(value.into()))
}

fn path_normalize(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let separator = if win32 { '\\' } else { '/' };
    let value = if win32 {
        value.replace('/', "\\")
    } else {
        value.replace('\\', "/")
    };
    let absolute =
        value.starts_with(separator) || (win32 && value.len() > 2 && value.as_bytes()[1] == b':');
    let mut parts = Vec::new();
    for part in value.split(separator) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            value => parts.push(value),
        }
    }
    let mut result = parts.join(&separator.to_string());
    if absolute && !(win32 && result.len() > 1 && result.as_bytes()[1] == b':') {
        result = format!("{separator}{result}");
    }
    if result.is_empty() {
        result = ".".into();
    }
    Ok(Value::String(result.into()))
}

fn path_parse(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let separator = if win32 && value.starts_with('/') {
        '/'
    } else if win32 {
        '\\'
    } else {
        '/'
    };
    let normalized = if win32 && separator == '\\' {
        value.replace('/', "\\")
    } else {
        value.to_owned()
    };
    let root = if win32
        && normalized.len() >= 3
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'\\'
    {
        &normalized[..3]
    } else if win32 && normalized.len() == 2 && normalized.as_bytes()[1] == b':' {
        &normalized[..2]
    } else if win32 && normalized.starts_with("\\\\") {
        let mut parts = normalized.split('\\').filter(|part| !part.is_empty());
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        return path_parse_windows_with_root(&normalized, &format!("\\\\{server}\\{share}\\"));
    } else if win32 && (normalized.starts_with('\\') || normalized.starts_with('/')) {
        if separator == '/' {
            "/"
        } else {
            "\\"
        }
    } else if !win32 && value.starts_with('/') {
        "/"
    } else {
        ""
    };
    let trimmed = normalized.trim_end_matches(separator);
    let (dir, base) = if win32 && normalized.len() == 2 && normalized.as_bytes()[1] == b':' {
        (root, "")
    } else if win32
        && normalized.len() == 3
        && normalized.as_bytes()[1] == b':'
        && normalized.as_bytes()[2] == b'\\'
    {
        (root, "")
    } else if trimmed.is_empty() && !root.is_empty() {
        (root, "")
    } else {
        trimmed
            .rsplit_once(separator)
            .map_or((root, trimmed), |(dir, base)| (dir, base))
    };
    let dir_with_extra_separator = if win32 {
        normalized.rsplit_once(separator).and_then(|(prefix, _)| {
            prefix
                .ends_with(separator)
                .then(|| format!("{dir}{separator}"))
        })
    } else {
        None
    };
    let dir = dir_with_extra_separator.as_deref().unwrap_or(dir);
    let (name, ext) = base
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or((base, ""), |index| (&base[..index], &base[index..]));
    Ok(Value::object(vec![
        ("root".into(), Value::String(root.to_string().into())),
        ("dir".into(), Value::String(dir.to_string().into())),
        ("base".into(), Value::String(base.to_string().into())),
        ("ext".into(), Value::String(ext.to_string().into())),
        ("name".into(), Value::String(name.to_string().into())),
    ]))
}

fn path_parse_windows_with_root(value: &str, root: &str) -> Result<Value, VmError> {
    let trimmed = value.trim_end_matches('\\');
    let (dir, base) = trimmed
        .rsplit_once('\\')
        .map_or((root, trimmed), |(dir, base)| (dir, base));
    let (name, ext) = base
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or((base, ""), |index| (&base[..index], &base[index..]));
    Ok(Value::object(vec![
        ("root".into(), Value::String(root.to_owned().into())),
        ("dir".into(), Value::String(dir.to_owned().into())),
        ("base".into(), Value::String(base.to_owned().into())),
        ("ext".into(), Value::String(ext.to_owned().into())),
        ("name".into(), Value::String(name.to_owned().into())),
    ]))
}

fn path_format(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let Some(Value::Object(object)) = arguments.first() else {
        return Err(VmError::Thrown(fs_error(
            "ERR_INVALID_ARG_TYPE",
            "path object must be an object",
        )));
    };
    let get = |name| {
        quench_runtime::execute::get_property_result(&Value::Object(object.clone()), name).ok()
    };
    let string_prop = |name| {
        get(name)
            .and_then(|value| match value {
                Value::String(value) => Some(value.to_string()),
                _ => None,
            })
            .unwrap_or_default()
    };
    let dir = {
        let value = string_prop("dir");
        if value.is_empty() {
            string_prop("root")
        } else {
            value
        }
    };
    let base = get("base")
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            let name = string_prop("name");
            let ext = string_prop("ext");
            let ext = if ext.is_empty() || ext.starts_with('.') {
                ext
            } else {
                format!(".{ext}")
            };
            format!("{name}{ext}")
        });
    let separator = if win32 { '\\' } else { '/' };
    let output = if dir.is_empty() {
        base
    } else if win32 && dir.ends_with(':') {
        format!("{dir}{base}")
    } else {
        format!(
            "{}{}{}",
            dir.strip_suffix(separator).unwrap_or(dir.as_str()),
            separator,
            base
        )
    };
    Ok(Value::String(output.into()))
}

fn path_relative(arguments: &[Value]) -> Result<Value, VmError> {
    let from = path_arg(arguments, 0)?;
    let to = path_arg(arguments, 1)?;
    if from.contains('\\') || to.contains('\\') {
        let from = from.replace('/', "\\");
        let to = to.replace('/', "\\");
        let from = from
            .split('\\')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let to = to
            .split('\\')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let common = from
            .iter()
            .zip(&to)
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count();
        let mut result = vec![".."; from.len().saturating_sub(common)];
        result.extend(to[common..].iter().copied());
        return Ok(Value::String(result.join("\\")));
    }
    let from = from
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();
    let to = to
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut result = vec![".."; from.len().saturating_sub(common)];
    result.extend(to[common..].iter().copied());
    Ok(Value::String(result.join("/")))
}

fn path_join(arguments: &[Value]) -> Result<Value, VmError> {
    let mut path = PathBuf::new();
    for argument in arguments {
        path.push(path_arg(std::slice::from_ref(argument), 0)?);
    }
    let joined = Value::String(path.to_string_lossy().into_owned().into());
    path_normalize(&[joined], false)
}

fn path_extname(arguments: &[Value]) -> Result<Value, VmError> {
    let path = path_arg(arguments, 0)?;
    let path = path.trim_end_matches(['/', '\\']);
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if base == "." || base == ".." {
        return Ok(Value::String("".into()));
    }
    let Some(dot) = base.rfind('.') else {
        return Ok(Value::String("".into()));
    };
    if dot == 0 && !base[1..].contains('.') {
        return Ok(Value::String("".into()));
    }
    Ok(Value::String(base[dot..].to_owned().into()))
}

fn path_dirname(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let win32 = value.contains('\\') || (value.len() >= 2 && value.as_bytes()[1] == b':');
    let separator = if win32 { '\\' } else { '/' };
    if win32 && value.len() == 3 && value.as_bytes()[1] == b':' && value.as_bytes()[2] == b'\\' {
        return Ok(Value::String(value.into()));
    }
    let value = value.trim_end_matches(['/', '\\']);
    let dirname = match value.rfind(separator) {
        Some(0) => {
            if win32 {
                "\\"
            } else {
                "/"
            }
        }
        Some(index) => &value[..index],
        None => {
            if win32 && value.len() == 2 && value.as_bytes()[1] == b':' {
                value
            } else {
                "."
            }
        }
    };
    Ok(Value::String(dirname.into()))
}

fn path_is_absolute(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    Ok(Value::Boolean(
        value.starts_with('/') || (value.len() > 2 && value.as_bytes()[1] == b':'),
    ))
}

fn path_is_absolute_win(arguments: &[Value]) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    Ok(Value::Boolean(
        value.starts_with(['/', '\\'])
            || (value.len() > 2
                && value.as_bytes()[1] == b':'
                && matches!(value.as_bytes()[2], b'/' | b'\\')),
    ))
}

fn path_matches_glob(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let value = path_arg(arguments, 0)?;
    let pattern = path_arg(arguments, 1)?;
    let value = if win32 {
        value.replace('\\', "/")
    } else {
        value.to_owned()
    };
    let pattern = if win32 {
        pattern.replace('\\', "/")
    } else {
        pattern.to_owned()
    };
    let matched = if let Some(prefix) = pattern.strip_suffix("/**") {
        value == prefix || value.starts_with(&format!("{prefix}/"))
    } else if let Some(suffix) = pattern.strip_prefix("*.") {
        value.ends_with(&format!(".{suffix}"))
    } else {
        value == pattern
    };
    Ok(Value::Boolean(matched))
}

fn path_resolve(arguments: &[Value], win32: bool) -> Result<Value, VmError> {
    let mut result = String::new();
    for argument in arguments {
        let value = path_arg(std::slice::from_ref(argument), 0)?;
        if win32 {
            result = format!(
                "{}\\{}",
                value.trim_end_matches(['/', '\\']),
                result.trim_start_matches(['/', '\\'])
            );
        } else {
            result = format!(
                "{}/{}",
                value.trim_end_matches('/'),
                result.trim_start_matches('/')
            );
        }
    }
    if result.is_empty() {
        result = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .into_owned();
    }
    Ok(Value::String(result.into()))
}

fn path_win_to_namespaced(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(value) = arguments.first() else {
        return Ok(Value::Undefined);
    };
    let Value::String(value) = value else {
        return Ok(value.clone());
    };
    let value = value.replace('/', "\\");
    if value.starts_with("\\\\") {
        Ok(Value::String(format!(
            "\\\\?\\UNC\\{}\\",
            value.trim_start_matches("\\\\")
        )))
    } else if value.len() > 2 && value.as_bytes()[1] == b':' {
        Ok(Value::String(format!("\\\\?\\{}", value)))
    } else {
        Ok(Value::String(value))
    }
}
