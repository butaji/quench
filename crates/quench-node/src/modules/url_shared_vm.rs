//! Shared-VM URL constructors and Node's legacy URL parser.

use crate::host::NodeHost;
use crate::modules::url_legacy::{self, LegacyUrlField, LegacyUrlParseError};
use quench_runtime::{NativeContext, RootId, RootedError};
use std::path::{Component, Path, PathBuf};

const URL_CONSTRUCTOR_SOURCE: &str = "(class Url {})";
const WHATWG_URL_FACTORY: &str = quench_js_check::checked_js!(
    r#"(parse) => {
  const state = new WeakMap();
  const fields = [
    "href", "origin", "protocol", "username", "password", "host",
    "hostname", "port", "pathname", "search", "hash",
  ];
  const data = (receiver) => {
    const value = state.get(receiver);
    if (value === undefined) throw new TypeError("Illegal invocation");
    return value;
  };
  class URL {
    constructor(input, base) {
      state.set(this, parse(String(input), base === undefined ? undefined : String(base)));
    }
    toString() { return data(this).href; }
    toJSON() { return data(this).href; }
  }
  for (const field of fields) {
    Object.defineProperty(URL.prototype, field, {
      configurable: true,
      enumerable: true,
      get() { return data(this)[field]; },
    });
  }
  return URL;
}"#
);

const WHATWG_PARSE_MARKER: &str = "\0quench:node:url:whatwg-parse";

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let constructor = url_constructor(context)?;
    let legacy_constructor =
        context.evaluate_script_rooted(URL_CONSTRUCTOR_SOURCE, "node:url/Url")?;
    let module = context.object_rooted()?;
    let parse = context.host_function_with_data(
        crate::host::shared_vm::operation("urlParse"),
        legacy_constructor,
    )?;
    set(context, module, "parse", parse)?;
    set(context, module, "URL", constructor)?;
    set(context, module, "Url", legacy_constructor)?;
    let path_to_file_url = context.host_function(
        crate::host::shared_vm::operation("pathToFileURL"),
    )?;
    set(context, module, "pathToFileURL", path_to_file_url)?;
    let file_url_to_path = context.host_function(
        crate::host::shared_vm::operation("fileURLToPath"),
    )?;
    set(context, module, "fileURLToPath", file_url_to_path)?;
    Ok(module)
}

pub(crate) fn path_to_file_url(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let path = args
        .first()
        .copied()
        .map(|root| context.string_text(root))
        .transpose()?
        .flatten()
        .ok_or_else(|| invalid_argument(context, "path"))?;
    let windows = windows_option(context, args)?;
    let href = if windows {
        windows_path_to_url(&path)
    } else {
        let path = PathBuf::from(path);
        let absolute = if path.is_absolute() {
            path
        } else {
            std::env::current_dir()
                .map_err(|error| RootedError::host(error.to_string()))?
                .join(path)
        };
        url::Url::from_file_path(normalize_path(&absolute)).ok()
    }
    .ok_or_else(|| invalid_argument(context, "path"))?
    .to_string();
    let href = context.string_rooted(&href);
    let constructor = url_constructor(context)?;
    context.construct_rooted(constructor, constructor, &[href])
}

pub(crate) fn file_url_to_path(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Err(invalid_argument(context, "url"));
    };
    let input = url_argument_text(context, input)?.ok_or_else(|| invalid_argument(context, "url"))?;
    let windows = windows_option(context, args)?;
    let parsed = url::Url::parse(&input).map_err(|_| invalid_url(context, &input))?;
    if parsed.scheme() != "file" {
        return Err(coded_url_type_error(
            context,
            "ERR_INVALID_URL_SCHEME",
            "The URL must be of scheme file",
        ));
    }
    if !windows && parsed
        .host_str()
        .is_some_and(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"))
    {
        return Err(coded_url_type_error(
            context,
            "ERR_INVALID_FILE_URL_HOST",
            "File URL host must be \"localhost\" or empty on this platform",
        ));
    }
    if parsed.path().to_ascii_lowercase().contains("%2f")
        || (windows && parsed.path().to_ascii_lowercase().contains("%5c"))
    {
        return Err(coded_url_type_error(
            context,
            "ERR_INVALID_FILE_URL_PATH",
            "File URL path must not include encoded path separators",
        ));
    }
    let path = if windows && !cfg!(windows) {
        windows_file_url_path(&parsed).ok_or_else(|| {
            coded_url_type_error(
                context,
                "ERR_INVALID_FILE_URL_PATH",
                "File URL path must be an absolute Windows path",
            )
        })?
    } else {
        parsed
            .to_file_path()
            .map_err(|_| invalid_url(context, &input))?
            .to_string_lossy()
            .into_owned()
    };
    Ok(context.string_rooted(&path))
}

fn windows_option(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<bool, RootedError> {
    let Some(options) = args.get(1).copied() else {
        return Ok(cfg!(windows));
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(cfg!(windows));
    }
    let key = context.string_rooted("windows");
    let windows = context.get_property_rooted(options, key)?;
    if context
        .rooted_value(windows)
        .is_some_and(|value| value.is_undefined())
    {
        Ok(cfg!(windows))
    } else {
        context.truthy_rooted(windows)
    }
}

fn windows_path_to_url(path: &str) -> Option<url::Url> {
    let path = path.replace('\\', "/");
    if let Some(unc) = path.strip_prefix("//") {
        let (host, path) = unc.split_once('/')?;
        if host.is_empty() || path.is_empty() {
            return None;
        }
        let mut url = url::Url::parse(&format!("file://{host}/")).ok()?;
        push_url_segments(&mut url, path, path.ends_with('/'))?;
        return Some(url);
    }

    if path.len() >= 2 && path.as_bytes()[0].is_ascii_alphabetic() && path.as_bytes()[1] == b':' {
        let drive = &path[..2];
        let mut rest = path[2..].to_owned();
        if !rest.starts_with('/') {
            let cwd = std::env::current_dir().ok()?.to_string_lossy().replace('\\', "/");
            rest = format!("/{}/{}", cwd.trim_start_matches('/'), rest);
        }
        let mut url = url::Url::parse("file:///").ok()?;
        {
            let mut segments = url.path_segments_mut().ok()?;
            segments.push(drive);
            for segment in rest.trim_start_matches('/').split('/') {
                if !segment.is_empty() {
                    segments.push(segment);
                }
            }
            if rest.ends_with('/') {
                segments.push("");
            }
        }
        return Some(url);
    }

    let path = if path.starts_with('/') {
        PathBuf::from(path)
    } else {
        let cwd = std::env::current_dir().ok()?;
        cwd.join(path)
    };
    url::Url::from_file_path(normalize_path(&path)).ok()
}

fn push_url_segments(url: &mut url::Url, path: &str, trailing_slash: bool) -> Option<()> {
    let mut segments = url.path_segments_mut().ok()?;
    for segment in path.split('/') {
        if !segment.is_empty() {
            segments.push(segment);
        }
    }
    if trailing_slash {
        segments.push("");
    }
    Some(())
}

fn windows_file_url_path(url: &url::Url) -> Option<String> {
    let path = percent_decode(url.path())?;
    if let Some(host) = url
        .host_str()
        .filter(|host| !host.is_empty() && !host.eq_ignore_ascii_case("localhost"))
    {
        let share_path = path.trim_start_matches('/').replace('/', "\\");
        return Some(format!("\\\\{host}\\{share_path}"));
    }
    let drive_path = path.strip_prefix('/')?;
    if drive_path.len() < 2
        || !drive_path.as_bytes()[0].is_ascii_alphabetic()
        || drive_path.as_bytes()[1] != b':'
    {
        return None;
    }
    Some(drive_path.replace('/', "\\"))
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_value(*bytes.get(index + 1)?)?;
            let low = hex_value(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            component => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn url_argument_text(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<Option<String>, RootedError> {
    if let Some(text) = context.string_text(value)? {
        return Ok(Some(text));
    }
    if !context.is_object_rooted(value)? {
        return Ok(None);
    }
    let global = context.global_root()?;
    let constructor = get(context, global, "URL")?;
    let prototype = get(context, constructor, "prototype")?;
    let to_string = get(context, prototype, "toString")?;
    match context.call_rooted(to_string, value, &[]) {
        Ok(url) => context.string_text(url),
        Err(error) => {
            if let Some(exception) = error.exception {
                context.release_root(exception);
            }
            Ok(None)
        }
    }
}

fn invalid_argument(context: &mut NativeContext<'_, NodeHost>, name: &str) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        &format!("The \"{name}\" argument must be of type string or an instance of URL"),
    )
}

fn coded_url_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    code: &str,
    message: &str,
) -> RootedError {
    let exception = match context.type_error_rooted(message) {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    let code = context.string_rooted(code);
    if let Err(error) = set(context, exception, "code", code) {
        return error;
    }
    context.throw(exception)
}

/// Install the realm's one URL constructor as the global; `node:url` reads the
/// same rooted constructor from host state.
pub(crate) fn install_global(context: &mut NativeContext<'_, NodeHost>) -> Result<(), RootedError> {
    let constructor = url_constructor(context)?;
    let install = context.evaluate_script_rooted(
        "(value) => Object.defineProperty(globalThis, 'URL', { value, writable: true, configurable: true })",
        "node:url/install-global.js",
    )?;
    let undefined = context.undefined();
    context.call_rooted(install, undefined, &[constructor])?;
    Ok(())
}

fn url_constructor(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let roots = context.host_mut().shared_state();
    if let Some(constructor) = roots.borrow().url_constructor {
        return Ok(constructor);
    }

    let parser_data = context.string_rooted(WHATWG_PARSE_MARKER);
    let parser = context
        .host_function_with_data(crate::host::shared_vm::operation("urlParse"), parser_data)?;
    let factory = context.evaluate_script_rooted(WHATWG_URL_FACTORY, "node:url/whatwg.js")?;
    let undefined = context.undefined();
    let constructor = context.call_rooted(factory, undefined, &[parser])?;
    let retained = context.retain(constructor)?;
    roots.borrow_mut().url_constructor = Some(retained);
    Ok(constructor)
}

pub(crate) fn parse(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    if context.string_text(data)?.as_deref() == Some(WHATWG_PARSE_MARKER) {
        return parse_whatwg(context, args);
    }

    let Some(input) = args.first().copied() else {
        return Err(type_error(
            context,
            "The \"url\" argument must be of type string",
        ));
    };
    let Some(input) = context.string_text(input)? else {
        return Err(type_error(
            context,
            "The \"url\" argument must be of type string",
        ));
    };
    let parsed = url_legacy::parse_legacy_parts(&input).map_err(|error| match error {
        LegacyUrlParseError::Invalid { code, input } => coded_type_error(context, &code, &input),
        LegacyUrlParseError::MalformedUri => uri_error(context),
    })?;
    let query_requested = args
        .get(1)
        .copied()
        .map(|argument| context.truthy_rooted(argument))
        .transpose()?
        .unwrap_or(false);
    let query_entries = if query_requested && parsed.query_object {
        Some(query_object(
            context,
            parsed.query.as_deref().unwrap_or_default(),
        )?)
    } else {
        None
    };
    let constructor = context.host_function_data()?;
    let instance = context.construct_rooted(constructor, constructor, &[])?;
    for (name, value) in parsed.fields {
        let value = if name == "query" {
            query_entries.unwrap_or_else(|| legacy_value(context, value))
        } else {
            legacy_value(context, value)
        };
        set(context, instance, name, value)?;
    }
    Ok(instance)
}

fn parse_whatwg(
    context: &mut NativeContext<'_, NodeHost>,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = args
        .first()
        .copied()
        .map(|root| context.to_string(root))
        .transpose()?
        .unwrap_or_else(|| "undefined".to_owned());
    let base = args
        .get(1)
        .copied()
        .filter(|root| {
            context
                .rooted_value(*root)
                .is_some_and(|value| !value.is_undefined())
        })
        .map(|root| context.to_string(root))
        .transpose()?;

    let parsed = crate::modules::url_whatwg_data::Parsed::parse_strict(&input, base.as_deref())
        .map_err(|_| invalid_url(context, &input))?;
    let fields = [
        "href", "origin", "protocol", "username", "password", "host", "hostname", "port",
        "pathname", "search", "hash",
    ];
    let object = context.object_rooted()?;
    for name in fields {
        let value = parsed.get(name);
        set_text(context, object, name, &value)?;
    }
    Ok(object)
}

fn query_object(
    context: &mut NativeContext<'_, NodeHost>,
    query: &str,
) -> Result<RootId, RootedError> {
    let object = context.null_object_rooted()?;
    for (key, values) in crate::modules::querystring_data::parse_default_entries(query) {
        let value = if values.len() == 1 {
            context.string_rooted(&values[0])
        } else {
            let values = values
                .iter()
                .map(|value| context.string_rooted(value))
                .collect::<Vec<_>>();
            context.array_rooted(&values)?
        };
        set(context, object, &key, value)?;
    }
    Ok(object)
}

fn legacy_value(context: &mut NativeContext<'_, NodeHost>, value: LegacyUrlField) -> RootId {
    match value {
        LegacyUrlField::Null => context.null(),
        LegacyUrlField::Boolean(value) => context.boolean(value),
        LegacyUrlField::Text(value) => context.string_rooted(&value),
    }
}

fn type_error(context: &mut NativeContext<'_, NodeHost>, message: &str) -> RootedError {
    match context.type_error_rooted(message) {
        Ok(error) => context.throw(error),
        Err(error) => error,
    }
}

fn coded_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    code: &str,
    input: &str,
) -> RootedError {
    let exception = match context.type_error_rooted("Invalid URL") {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    let code = context.string_rooted(code);
    let input = context.string_rooted(input);
    if let Err(error) = set(context, exception, "code", code) {
        return error;
    }
    if let Err(error) = set(context, exception, "input", input) {
        return error;
    }
    context.throw(exception)
}

fn invalid_url(context: &mut NativeContext<'_, NodeHost>, input: &str) -> RootedError {
    coded_type_error(context, "ERR_INVALID_URL", input)
}

fn uri_error(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    let exception = match context.error_rooted("URI malformed") {
        Ok(exception) => exception,
        Err(error) => return error,
    };
    let name = context.string_rooted("URIError");
    let code = context.string_rooted("ERR_INVALID_URI");
    if let Err(error) = set(context, exception, "name", name) {
        return error;
    }
    if let Err(error) = set(context, exception, "code", code) {
        return error;
    }
    context.throw(exception)
}

fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.set_property_rooted(object, key, value, object)?;
    Ok(())
}

fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}

fn set_text(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: &str,
) -> Result<(), RootedError> {
    let value = context.string_rooted(value);
    set(context, object, name, value)
}
