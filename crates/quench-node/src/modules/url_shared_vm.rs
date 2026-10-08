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
  const serialize = (value) => {
    const auth = value.username || value.password
      ? `${value.username}${value.password ? `:${value.password}` : ""}@`
      : "";
    return `${value.protocol}${value.host ? `//${auth}${value.host}` : ""}${value.pathname}${value.search}${value.hash}`;
  };
  const syncSearchParams = (receiver, value) => {
    const current = data(receiver);
    current.search = value;
    current.href = serialize(current);
  };
  class URL {
    constructor(input, base) {
      const value = parse(String(input), base === undefined ? undefined : String(base));
      if (!value) throw new TypeError("Invalid URL");
      state.set(this, value);
    }
    toString() { return data(this).href; }
    toJSON() { return data(this).href; }
    static canParse(input, base) {
      if (arguments.length === 0) {
        throw Object.assign(new TypeError("The \"input\" argument must be specified"), {
          code: "ERR_MISSING_ARGS",
        });
      }
      try {
        parse(String(input), base === undefined ? undefined : String(base));
        return true;
      } catch (_) {
        return false;
      }
    }
    get searchParams() {
      const value = data(this);
      if (!value._searchParams) {
        const params = new globalThis.URLSearchParams(value.search);
        Object.defineProperty(params, "_onchange", {
          configurable: true,
          value: () => syncSearchParams(this, params.toString() ? `?${params.toString()}` : ""),
        });
        value._searchParams = params;
      }
      return value._searchParams;
    }
  }
  for (const field of fields) {
    Object.defineProperty(URL.prototype, field, {
      configurable: true,
      enumerable: true,
      get() { return data(this)[field]; },
      set(value) {
        const current = data(this);
        if (field === "href") {
          const updated = parse(String(value), undefined);
          if (!updated) throw new TypeError("Invalid URL");
          if (current._searchParams) {
            const params = new globalThis.URLSearchParams(updated.search);
            current._searchParams._pairs = params._pairs;
          }
          Object.assign(current, updated);
          return;
        }
        current[field] = String(value);
        if (field === "search") {
          current.search = current.search && current.search !== "?" ? current.search : "";
          if (current._searchParams) {
            current._searchParams._pairs = new globalThis.URLSearchParams(current.search)._pairs;
          }
        }
        if (field === "pathname" && current.pathname === "") current.pathname = "/";
        current.href = serialize(current);
        if (field === "searchParams") throw new TypeError("Cannot set property searchParams of [object URL] which has only a getter");
      },
    });
  }
  Object.defineProperty(URL.prototype, Symbol.toStringTag, { value: "URL" });
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
    let format = context.host_function(crate::host::shared_vm::operation("urlFormat"))?;
    set(context, module, "format", format)?;
    let domain_to_ascii =
        context.host_function(crate::host::shared_vm::operation("domainToASCII"))?;
    set(context, module, "domainToASCII", domain_to_ascii)?;
    let domain_to_unicode =
        context.host_function(crate::host::shared_vm::operation("domainToUnicode"))?;
    set(context, module, "domainToUnicode", domain_to_unicode)?;
    let url_to_http_options =
        context.host_function(crate::host::shared_vm::operation("urlToHttpOptions"))?;
    set(context, module, "urlToHttpOptions", url_to_http_options)?;
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

pub(crate) fn domain_to_ascii(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(input) => context.to_string(input)?,
        None => "undefined".to_owned(),
    };
    let output = url::Host::parse(&input)
        .map(|host| host.to_string())
        .unwrap_or_default();
    Ok(context.string_rooted(&output))
}

pub(crate) fn domain_to_unicode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let input = match args.first().copied() {
        Some(input) => context.to_string(input)?,
        None => "undefined".to_owned(),
    };
    let output = match url::Host::parse(&input) {
        Ok(url::Host::Domain(domain)) => {
            let (unicode, result) = idna::domain_to_unicode(&domain);
            if result.is_ok() { unicode } else { String::new() }
        }
        Ok(host) => host.to_string(),
        Err(_) => String::new(),
    };
    Ok(context.string_rooted(&output))
}

pub(crate) fn url_to_http_options(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Err(invalid_url_to_http_options_argument(context));
    };
    if !context.is_object_rooted(input)? {
        return Err(invalid_url_to_http_options_argument(context));
    }
    let href = url_argument_text(context, input)?;
    let is_url = href.is_some();
    let result = context.null_object_rooted()?;
    for name in ["protocol", "hostname", "hash", "search", "pathname"] {
        let value = get(context, input, name)?;
        set(context, result, name, value)?;
    }
    let pathname = get(context, input, "pathname")?;
    let search = get(context, input, "search")?;
    let pathname = nullish_string(context, pathname)?;
    let search = nullish_string(context, search)?;
    let path = format!("{pathname}{search}");
    let path = context.string_rooted(&path);
    set(context, result, "path", path)?;
    let href = href
        .map(|href| context.string_rooted(&href))
        .unwrap_or(context.undefined());
    set(context, result, "href", href)?;

    let port = get(context, input, "port")?;
    let port_text = context.string_text(port)?;
    if let Some(port_text) = port_text.filter(|port| !port.is_empty()) {
        let number = port_text.parse::<f64>().unwrap_or(f64::NAN);
        let number = context.number(number);
        set(context, result, "port", number)?;
    } else if !is_url {
        let number = context.number(f64::NAN);
        set(context, result, "port", number)?;
    }

    let username_value = get(context, input, "username")?;
    let password_value = get(context, input, "password")?;
    let username = nullish_string(context, username_value)?;
    let password = nullish_string(context, password_value)?;
    if !username.is_empty() || !password.is_empty() {
        let username = percent_decode(&username).unwrap_or(username);
        let password = percent_decode(&password).unwrap_or(password);
        let auth = context.string_rooted(&format!("{username}:{password}"));
        set(context, result, "auth", auth)?;
    }
    Ok(result)
}

fn nullish_string(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        Ok(String::new())
    } else {
        context.to_string(value)
    }
}

fn invalid_url_to_http_options_argument(
    context: &mut NativeContext<'_, NodeHost>,
) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"url\" argument must be of type object",
    )
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
        .ok_or_else(|| invalid_path_argument(context))?;
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
    .ok_or_else(|| invalid_path_argument(context))?
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
        return Err(invalid_url_argument(context));
    };
    let input = url_argument_text(context, input)?.ok_or_else(|| invalid_url_argument(context))?;
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
            &format!(
                "File URL host must be \"localhost\" or empty on {}",
                std::env::consts::OS
            ),
        ));
    }
    if parsed.path().to_ascii_lowercase().contains("%2f")
        || (windows && parsed.path().to_ascii_lowercase().contains("%5c"))
    {
        return Err(coded_url_type_error(
            context,
            "ERR_INVALID_FILE_URL_PATH",
            "File URL path must not include encoded / characters",
        ));
    }
    let path = if windows {
        if cfg!(windows) {
            parsed
                .to_file_path()
                .map_err(|_| invalid_url(context, &input))?
                .to_string_lossy()
                .into_owned()
        } else {
            windows_file_url_path(&parsed).ok_or_else(|| {
                coded_url_type_error(
                    context,
                    "ERR_INVALID_FILE_URL_PATH",
                    "File URL path must be an absolute Windows path",
                )
            })?
        }
    } else {
        percent_decode(parsed.path()).ok_or_else(|| {
            coded_url_type_error(
                context,
                "ERR_INVALID_FILE_URL_PATH",
                "File URL path contains invalid UTF-8",
            )
        })?
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

fn invalid_path_argument(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"path\" argument must be of type string",
    )
}

fn invalid_url_argument(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"url\" argument must be of type string or an instance of URL",
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

pub(crate) fn format(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let Some(input) = args.first().copied() else {
        return Err(invalid_format_argument(context));
    };
    if let Some(input) = context.string_text(input)? {
        let parts = match url_legacy::parse_legacy_parts(&input) {
            Ok(parts) => parts,
            Err(LegacyUrlParseError::Invalid { code, input }) => {
                return Err(coded_type_error(context, &code, &input));
            }
            Err(LegacyUrlParseError::MalformedUri) => return Err(uri_error(context)),
        };
        let mut fields = LegacyFormatFields::default();
        for (name, value) in parts.fields {
            match (name, value) {
                ("slashes", LegacyUrlField::Boolean(value)) => fields.slashes = value,
                (_, LegacyUrlField::Text(value)) => fields.set(name, value),
                _ => {}
            }
        }
        let formatted = format_legacy_fields(fields);
        return Ok(context.string_rooted(&formatted));
    }

    if !context.is_object_rooted(input)? {
        return Err(invalid_format_argument(context));
    }
    if let Some(href) = url_argument_text(context, input)? {
        let formatted = format_whatwg_url(context, &href, args.get(1).copied())?;
        return Ok(context.string_rooted(&formatted));
    }

    let mut fields = LegacyFormatFields::default();
    fields.auth = truthy_property_string(context, input, "auth")?;
    fields.auth_encoded = fields
        .auth
        .as_deref()
        .filter(|auth| !auth.is_empty())
        .map(encode_auth);
    fields.protocol = truthy_property_string(context, input, "protocol")?;
    fields.pathname = truthy_property_string(context, input, "pathname")?;
    fields.hash = truthy_property_string(context, input, "hash")?;
    fields.host = truthy_property_string(context, input, "host")?;
    fields.hostname = truthy_property_string(context, input, "hostname")?;
    fields.port = truthy_property_string(context, input, "port")?;
    let query = get(context, input, "query")?;
    if context.is_object_rooted(query)? {
        let query_module = crate::modules::querystring_shared_vm::module(context)?;
        let stringify = get(context, query_module, "stringify")?;
        let undefined = context.undefined();
        let stringified = context.call_rooted(stringify, undefined, &[query])?;
        fields.query = context.string_text(stringified)?;
    }
    fields.search = truthy_property_string(context, input, "search")?;
    let slashes = get(context, input, "slashes")?;
    fields.slashes = context.truthy_rooted(slashes)?;
    let formatted = format_legacy_fields(fields);
    Ok(context.string_rooted(&formatted))
}

#[derive(Default)]
struct LegacyFormatFields {
    protocol: Option<String>,
    auth: Option<String>,
    auth_encoded: Option<String>,
    host: Option<String>,
    hostname: Option<String>,
    port: Option<String>,
    pathname: Option<String>,
    search: Option<String>,
    hash: Option<String>,
    query: Option<String>,
    slashes: bool,
}

impl LegacyFormatFields {
    fn set(&mut self, name: &str, value: String) {
        match name {
            "protocol" => self.protocol = Some(value),
            "auth" => self.auth = Some(value),
            "host" => self.host = Some(value),
            "hostname" => self.hostname = Some(value),
            "port" => self.port = Some(value),
            "pathname" => self.pathname = Some(value),
            "search" => self.search = Some(value),
            "hash" => self.hash = Some(value),
            _ => {}
        }
    }
}

fn format_legacy_fields(fields: LegacyFormatFields) -> String {
    let mut protocol = fields.protocol.unwrap_or_default();
    if !protocol.is_empty() && !protocol.ends_with(':') {
        protocol.push(':');
    }
    let mut host = fields.host.unwrap_or_default();
    if host.is_empty() {
        if let Some(hostname) = fields.hostname {
            host = if hostname.contains(':') && !hostname.starts_with('[') {
                format!("[{hostname}]")
            } else {
                hostname
            };
            if let Some(port) = fields.port {
                if !port.is_empty() {
                    host.push(':');
                    host.push_str(&port);
                }
            }
        }
    }
    if let Some(auth) = fields.auth_encoded {
        host = format!("{auth}@{host}");
    } else if let Some(auth) = fields.auth.filter(|auth| !auth.is_empty()) {
        host = format!("{}@{host}", encode_auth(&auth));
    }

    let mut pathname = fields.pathname.unwrap_or_default();
    pathname = pathname.replace('#', "%23").replace('?', "%3F");
    let mut search = fields
        .search
        .filter(|search| !search.is_empty())
        .or_else(|| fields.query.filter(|query| !query.is_empty()).map(|query| format!("?{query}")))
        .unwrap_or_default();
    if search.contains('#') {
        search = search.replace('#', "%23");
    }
    let mut hash = fields.hash.unwrap_or_default();
    if !hash.is_empty() && !hash.starts_with('#') {
        hash.insert(0, '#');
    }

    if fields.slashes || is_slashed_protocol(&protocol) {
        if fields.slashes || !host.is_empty() {
            if !pathname.is_empty() && !pathname.starts_with('/') {
                pathname.insert(0, '/');
            }
            host.insert_str(0, "//");
        } else if protocol.starts_with("file") {
            host.push_str("//");
        }
    }
    if !search.is_empty() && !search.starts_with('?') {
        search.insert(0, '?');
    }
    format!("{protocol}{host}{pathname}{search}{hash}")
}

fn is_slashed_protocol(protocol: &str) -> bool {
    matches!(protocol, "http:" | "https:" | "ftp:" | "gopher:" | "file:" | "ws:" | "wss:")
}

fn encode_auth(auth: &str) -> String {
    let mut encoded = String::with_capacity(auth.len());
    for byte in auth.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'():".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(b"0123456789ABCDEF"[(byte >> 4) as usize]));
            encoded.push(char::from(b"0123456789ABCDEF"[(byte & 0x0f) as usize]));
        }
    }
    encoded
}

fn format_whatwg_url(
    context: &mut NativeContext<'_, NodeHost>,
    href: &str,
    options: Option<RootId>,
) -> Result<String, RootedError> {
    let mut formatted = href.to_owned();
    let Some(options) = options else {
        return Ok(formatted);
    };
    if context
        .rooted_value(options)
        .is_some_and(|value| value.is_null() || value.is_undefined())
        || !context.truthy_rooted(options)?
    {
        return Ok(formatted);
    }
    if !context.is_object_rooted(options)? {
        return Err(invalid_format_options(context));
    }
    if !option_truthy(context, options, "fragment")? {
        if let Some(index) = formatted.find('#') {
            formatted.truncate(index);
        }
    }
    let unicode = option_truthy(context, options, "unicode")?;
    if unicode {
        formatted = format_unicode_hostname(&formatted);
    }
    if !option_truthy(context, options, "search")? {
        let before_hash = formatted.find('#').unwrap_or(formatted.len());
        if let Some(index) = formatted[..before_hash].find('?') {
            formatted.replace_range(index..before_hash, "");
        }
    }
    if !option_truthy(context, options, "auth")? {
        if let Some(authority_start) = formatted.find("//").map(|index| index + 2) {
            let authority_end = formatted[authority_start..]
                .find(['/', '?', '#'])
                .map(|index| authority_start + index)
                .unwrap_or(formatted.len());
            if let Some(auth_end) = formatted[authority_start..authority_end].rfind('@') {
                formatted.replace_range(authority_start..authority_start + auth_end + 1, "");
            }
        }
    }
    Ok(formatted)
}

fn format_unicode_hostname(href: &str) -> String {
    let Some(scheme_end) = href.find("://") else {
        return href.to_owned();
    };
    let authority_start = scheme_end + 3;
    let authority_end = href[authority_start..]
        .find(['/', '?', '#'])
        .map(|index| authority_start + index)
        .unwrap_or(href.len());
    let authority = &href[authority_start..authority_end];
    let host_start = authority
        .rfind('@')
        .map(|index| authority_start + index + 1)
        .unwrap_or(authority_start);
    let host_end = if href[host_start..authority_end].starts_with('[') {
        href[host_start..authority_end]
            .find(']')
            .map(|index| host_start + index + 1)
            .unwrap_or(authority_end)
    } else {
        href[host_start..authority_end]
            .find(':')
            .map(|index| host_start + index)
            .unwrap_or(authority_end)
    };
    let hostname = &href[host_start..host_end];
    let (unicode, result) = idna::domain_to_unicode(hostname);
    if result.is_err() || unicode == hostname {
        return href.to_owned();
    }
    let mut formatted = href.to_owned();
    formatted.replace_range(host_start..host_end, &unicode);
    formatted
}

fn option_truthy(
    context: &mut NativeContext<'_, NodeHost>,
    options: RootId,
    name: &str,
) -> Result<bool, RootedError> {
    let value = get(context, options, name)?;
    if context
        .rooted_value(value)
        .is_some_and(|value| value.is_null() || value.is_undefined())
    {
        return Ok(true);
    }
    context.truthy_rooted(value)
}

fn truthy_property_string(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<Option<String>, RootedError> {
    let value = get(context, object, name)?;
    if context.truthy_rooted(value)? {
        Ok(Some(context.to_string(value)?))
    } else {
        Ok(None)
    }
}

fn invalid_format_argument(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(
        context,
        "ERR_INVALID_ARG_TYPE",
        "The \"urlObject\" argument must be of type object or string",
    )
}

fn invalid_format_options(context: &mut NativeContext<'_, NodeHost>) -> RootedError {
    coded_url_type_error(context, "ERR_INVALID_ARG_TYPE", "The \"options\" argument must be of type object")
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
