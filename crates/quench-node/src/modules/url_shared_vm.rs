//! `node:url` (lib/url.js) and the `internal/url` namespace host code converts file URLs
//! through: the WHATWG classes (`url_whatwg_shared_vm`), the legacy API
//! (`url_legacy_shared_vm`) and the file URL conversions.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The `url` exports beyond the classes, in Node's order: (name, operation, length).
const LEGACY_EXPORTS: &[(&str, &str, &str, f64)] = &[
    ("parse", "urlParse", "urlLegacyParse", 3.0),
    ("resolve", "urlResolve", "urlLegacyResolve", 2.0),
    ("resolveObject", "urlResolveObject", "urlLegacyResolveObject", 2.0),
    ("format", "urlFormat", "urlLegacyFormat", 2.0),
];
const CONVERSIONS: &[(&str, &str, f64)] = &[
    ("domainToASCII", "urlDomainToASCII", 1.0),
    ("domainToUnicode", "urlDomainToUnicode", 1.0),
    ("pathToFileURL", "urlPathToFileURL", 2.0),
    ("fileURLToPath", "fileURLToPath", 1.0),
    ("fileURLToPathBuffer", "fileURLToPathBuffer", 1.0),
    ("urlToHttpOptions", "urlToHttpOptions", 1.0),
];
/// `internal/url` helpers beyond the public conversions.
const INTERNALS: &[(&str, &str, f64)] = &[("isURL", "urlIsURL", 1.0), ("toPathIfFileURL", "toPathIfFileURL", 1.0)];

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

fn export(c: &mut Context<'_>, object: RootId, export: &str, name: &str, operation_name: &str, length: f64) -> Result<(), RootedError> {
    let function = c.host_function_named(operation(operation_name), name)?;
    let key = c.string_rooted("length");
    let length = c.number(length);
    c.define_data_property_rooted(function, key, length, false, false, true)?;
    put(c, object, export, function)
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

/// `node:url`.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    let url = super::url_legacy_shared_vm::class(c)?;
    put(c, module, "Url", url)?;
    for &(name, function_name, operation_name, length) in LEGACY_EXPORTS {
        export(c, module, name, function_name, operation_name, length)?;
    }
    let (url, params) = super::url_whatwg_shared_vm::classes(c)?;
    put(c, module, "URL", url)?;
    put(c, module, "URLSearchParams", params)?;
    for &(name, operation_name, length) in CONVERSIONS {
        export(c, module, name, name, operation_name, length)?;
    }
    Ok(module)
}

/// The `internal/url` namespace: the classes, the conversions and `isURL`.
pub(crate) fn internal_module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = c.object_rooted()?;
    let (url, params) = super::url_whatwg_shared_vm::classes(c)?;
    put(c, module, "URL", url)?;
    put(c, module, "URLSearchParams", params)?;
    for &(name, operation_name, length) in CONVERSIONS.iter().chain(INTERNALS) {
        export(c, module, name, name, operation_name, length)?;
    }
    Ok(module)
}

/// `isURL`: Node's URL-like shape test (`href` and `protocol`, no `auth` or `path`).
pub(crate) fn is_url_like(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        if c.string_text(value)?.is_some() && !strings_have_href(c)? {
            return Ok(false);
        }
        if type_of(c, value).is_nullish() {
            return Ok(false);
        }
    }
    let href = get(c, value, "href")?;
    if !c.truthy_rooted(href)? {
        return Ok(false);
    }
    let protocol = get(c, value, "protocol")?;
    if !c.truthy_rooted(protocol)? {
        return Ok(false);
    }
    let auth = get(c, value, "auth")?;
    let path = get(c, value, "path")?;
    Ok(type_of(c, auth) == ValueType::Undefined && type_of(c, path) == ValueType::Undefined)
}

pub(crate) fn is_url_operation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let result = is_url_like(c, value)?;
    Ok(c.boolean(result))
}

/// Whether a string primitive finds an `href` property (its prototype chain is the realm's
/// `String.prototype` and `Object.prototype`); [[HasProperty]] on them runs no script.
fn strings_have_href(c: &mut Context<'_>) -> Result<bool, RootedError> {
    // The intrinsic `String.prototype`, read once: every path argument asks.
    const CACHE_KEY: &str = "\0url:String.prototype";
    let shared = c.host_mut().shared_state();
    let cached = shared.borrow().builtin_cache.get(CACHE_KEY).copied();
    let prototype = match cached {
        Some(prototype) => prototype,
        None => {
            let prototype = c.with_startup_intrinsics(|c| {
                let global = c.global_root()?;
                let constructor = get(c, global, "String")?;
                get(c, constructor, "prototype")
            })?;
            let retained = c.retain(prototype)?;
            shared.borrow_mut().builtin_cache.insert(CACHE_KEY.to_owned(), retained);
            retained
        }
    };
    let key = c.string_rooted("href");
    c.has_property_rooted(prototype, key)
}

pub(crate) fn to_path_if_file_url(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    if !is_url_like(c, value)? {
        return Ok(value);
    }
    file_url_to_path(c, this, &[value])
}

fn coded_type_error(c: &mut Context<'_>, message: &str, code: &'static str) -> Result<RootedError, RootedError> {
    let error = c.type_error_rooted(message)?;
    super::node_errors_shared_vm::decorate_type_error(c, error, code)?;
    Ok(c.throw(error))
}

/// `options.windows`, or the host platform's convention.
fn windows_option(c: &mut Context<'_>, options: RootId) -> Result<bool, RootedError> {
    if c.is_object_rooted(options)? {
        let windows = get(c, options, "windows")?;
        if !type_of(c, windows).is_nullish() {
            return c.truthy_rooted(windows);
        }
    }
    Ok(cfg!(windows))
}

/// `fileURLToPath`'s path text: the URL's file path in the chosen convention.
fn file_url_path(c: &mut Context<'_>, args: &[RootId]) -> Result<String, RootedError> {
    let input = argument(c, args, 0);
    let options = argument(c, args, 1);
    let windows = windows_option(c, options)?;
    let url = if let Some(text) = c.string_text(input)? {
        match url::Url::parse(&text) {
            Ok(url) => url,
            Err(_) => return Err(super::url_whatwg_shared_vm::invalid_url(c, &text, None)?),
        }
    } else if is_url_like(c, input)? {
        let href = get(c, input, "href")?;
        let href = c.to_string(href)?;
        match url::Url::parse(&href) {
            Ok(url) => url,
            Err(_) => return Err(super::url_whatwg_shared_vm::invalid_url(c, &href, None)?),
        }
    } else {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, input, "path", "of type string or an instance of URL")?);
    };
    if url.scheme() != "file" {
        return Err(coded_type_error(c, "The URL must be of scheme file", "ERR_INVALID_URL_SCHEME")?);
    }
    let hostname = url::quirks::hostname(&url).to_owned();
    let pathname = url::quirks::pathname(&url).to_owned();
    let encoded_separator = |pathname: &str, windows: bool| {
        pathname.as_bytes().windows(3).any(|window| {
            window[0] == b'%' && ((window[1] == b'2' && window[2] | 0x20 == b'f') || (windows && window[1] == b'5' && window[2] | 0x20 == b'c'))
        })
    };
    if windows {
        if encoded_separator(&pathname, true) {
            return Err(coded_type_error(c, "File URL path must not include encoded \\ or / characters", "ERR_INVALID_FILE_URL_PATH")?);
        }
        let path = percent_decode(c, &pathname.replace('/', "\\"))?;
        if !hostname.is_empty() {
            return Ok(format!("\\\\{}{path}", idna::domain_to_unicode(&hostname).0));
        }
        let bytes = path.as_bytes();
        let letter = bytes.get(1).map(|byte| byte | 0x20);
        if !letter.is_some_and(|letter| letter.is_ascii_lowercase()) || bytes.get(2) != Some(&b':') {
            return Err(coded_type_error(c, "File URL path must be absolute", "ERR_INVALID_FILE_URL_PATH")?);
        }
        return Ok(path[1..].to_owned());
    }
    if !hostname.is_empty() {
        let platform = super::process_state::platform();
        return Err(coded_type_error(c, &format!("File URL host must be \"localhost\" or empty on {platform}"), "ERR_INVALID_FILE_URL_HOST")?);
    }
    if encoded_separator(&pathname, false) {
        return Err(coded_type_error(c, "File URL path must not include encoded / characters", "ERR_INVALID_FILE_URL_PATH")?);
    }
    percent_decode(c, &pathname)
}

/// `decodeURIComponent`.
fn percent_decode(c: &mut Context<'_>, text: &str) -> Result<String, RootedError> {
    let global = c.global_root()?;
    let decode = get(c, global, "decodeURIComponent")?;
    let text = c.string_rooted(text);
    let undefined = c.undefined();
    let decoded = c.call_rooted(decode, undefined, &[text])?;
    c.to_string(decoded)
}

/// `url.fileURLToPath(url, options)`.
pub(crate) fn file_url_to_path(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = file_url_path(c, args)?;
    Ok(c.string_rooted(&path))
}

/// `url.fileURLToPathBuffer(url, options)`: the path's bytes.
pub(crate) fn file_url_to_path_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = file_url_path(c, args)?;
    super::buffer_shared_vm::from_bytes(c, path.as_bytes())
}

/// `url.pathToFileURL(path, options)`.
pub(crate) fn path_to_file_url(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = argument(c, args, 0);
    let Some(path) = c.string_text(input)? else {
        return Err(super::util_shared_vm::argument_type_error(c, input, "path", "string")?);
    };
    let options = argument(c, args, 1);
    let windows = windows_option(c, options)?;
    let flavor = if windows { super::path_shared_vm::win32(c)? } else { super::path_shared_vm::posix(c)? };
    let unc = windows && path.starts_with("\\\\");
    let mut resolved = if unc {
        path.clone()
    } else {
        let resolve = get(c, flavor, "resolve")?;
        let resolved = c.call_rooted(resolve, flavor, &[input])?;
        c.to_string(resolved)?
    };
    let undefined = c.undefined();
    if windows && resolved.starts_with("\\\\") {
        const EXTENDED_UNC_PREFIX: &str = "\\\\?\\UNC\\";
        const UNC_PREFIX: &str = "\\\\";
        let prefix = if resolved.starts_with(EXTENDED_UNC_PREFIX) { EXTENDED_UNC_PREFIX.len() } else { UNC_PREFIX.len() };
        let Some(end) = resolved[prefix..].find('\\').map(|end| end + prefix) else {
            let value = c.string_rooted(&resolved);
            return Err(super::util_shared_vm::argument_value_error(c, value, "path", "Missing UNC resource path")?);
        };
        if end == UNC_PREFIX.len() {
            let value = c.string_rooted(&resolved);
            return Err(super::util_shared_vm::argument_value_error(c, value, "path", "Empty UNC servername")?);
        }
        let hostname = c.string_rooted(&resolved[prefix..end]);
        let rest = c.string_rooted(&resolved[end..]);
        let yes = c.boolean(true);
        let binding = super::url_binding_shared_vm::binding(c)?;
        let convert = get(c, binding, "pathToFileURL")?;
        let href = c.call_rooted(convert, undefined, &[rest, yes, hostname])?;
        let href = c.to_string(href)?;
        return whatwg_url(c, &href);
    }
    let separator = if windows { '\\' } else { '/' };
    let last = path.chars().last();
    if (last == Some('/') || (windows && last == Some('\\'))) && !resolved.ends_with(separator) {
        resolved.push('/');
    }
    let resolved = c.string_rooted(&resolved);
    let windows = c.boolean(windows);
    let binding = super::url_binding_shared_vm::binding(c)?;
    let convert = get(c, binding, "pathToFileURL")?;
    let href = c.call_rooted(convert, undefined, &[resolved, windows])?;
    let href = c.to_string(href)?;
    whatwg_url(c, &href)
}

fn whatwg_url(c: &mut Context<'_>, href: &str) -> Result<RootId, RootedError> {
    match url::Url::parse(href) {
        Ok(url) => super::url_whatwg_shared_vm::url_object(c, &url),
        Err(_) => Err(super::url_whatwg_shared_vm::invalid_url(c, href, None)?),
    }
}

/// `url.urlToHttpOptions(url)`.
pub(crate) fn url_to_http_options(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let url = argument(c, args, 0);
    if !c.is_object_rooted(url)? {
        return Err(super::util_shared_vm::argument_type_error(c, url, "url", "Object")?);
    }
    let options = c.null_object_rooted()?;
    for key in c.enumerable_own_keys_rooted(url, true, true)? {
        let value = c.get_property_rooted(url, key)?;
        c.define_data_property_rooted(options, key, value, true, true, true)?;
    }
    let mut fields = Vec::new();
    for name in ["protocol", "hostname", "hash", "search", "pathname", "href", "port", "username", "password"] {
        fields.push(get(c, url, name)?);
    }
    let [protocol, hostname, hash, search, pathname, href, port, username, password] = fields[..] else {
        return Err(RootedError::host("URL fields read"));
    };
    put(c, options, "protocol", protocol)?;
    let hostname = match c.string_text(hostname)? {
        Some(text) if text.starts_with('[') => {
            let end = text.len().saturating_sub(1).max(1);
            c.string_rooted(&text[1..end])
        }
        _ => hostname,
    };
    put(c, options, "hostname", hostname)?;
    put(c, options, "hash", hash)?;
    put(c, options, "search", search)?;
    put(c, options, "pathname", pathname)?;
    let path_text = |c: &mut Context<'_>, value: RootId| -> Result<String, RootedError> {
        if c.truthy_rooted(value)? { c.to_string(value) } else { Ok(String::new()) }
    };
    let path = format!("{}{}", path_text(c, pathname)?, path_text(c, search)?);
    let path = c.string_rooted(&path);
    put(c, options, "path", path)?;
    put(c, options, "href", href)?;
    if c.string_text(port)?.as_deref() != Some("") {
        let number = c.to_number(port)?;
        let number = c.number(number);
        put(c, options, "port", number)?;
    }
    if c.truthy_rooted(username)? || c.truthy_rooted(password)? {
        let username = c.to_string(username)?;
        let password = c.to_string(password)?;
        let auth = format!("{}:{}", percent_decode(c, &username)?, percent_decode(c, &password)?);
        let auth = c.string_rooted(&auth);
        put(c, options, "auth", auth)?;
    }
    Ok(options)
}

/// `url.domainToASCII(domain)` and `url.domainToUnicode(domain)`.
fn domain(c: &mut Context<'_>, args: &[RootId], name: &str) -> Result<RootId, RootedError> {
    if args.is_empty() {
        let error = c.type_error_rooted("The \"domain\" argument must be specified")?;
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS")?;
        return Err(c.throw(error));
    }
    let text = c.to_string(args[0])?;
    let text = c.string_rooted(&text);
    let binding = super::url_binding_shared_vm::binding(c)?;
    let convert = get(c, binding, name)?;
    let undefined = c.undefined();
    c.call_rooted(convert, undefined, &[text])
}

pub(crate) fn domain_to_ascii(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    domain(c, args, "domainToASCII")
}

pub(crate) fn domain_to_unicode(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    domain(c, args, "domainToUnicode")
}

/// `url.format(URL, options)`: the serialization with Node's fragment, unicode, search and
/// auth switches.
pub(crate) fn format_whatwg(c: &mut Context<'_>, href: RootId, options: RootId) -> Result<RootId, RootedError> {
    let mut flags = [true, false, true, true];
    if !type_of(c, options).is_nullish() {
        if type_of(c, options) != ValueType::Object {
            return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
        }
        for (flag, name) in flags.iter_mut().zip(["fragment", "unicode", "search", "auth"]) {
            let value = get(c, options, name)?;
            if !type_of(c, value).is_nullish() {
                *flag = c.truthy_rooted(value)?;
            }
        }
    }
    let binding = super::url_binding_shared_vm::binding(c)?;
    let format = get(c, binding, "format")?;
    let mut arguments = vec![href];
    for flag in flags {
        arguments.push(c.boolean(flag));
    }
    let undefined = c.undefined();
    c.call_rooted(format, undefined, &arguments)
}

/// A string argument, or the serialization of a URL object; `None` for anything else.
pub(crate) fn url_argument_text(c: &mut Context<'_>, value: RootId) -> Result<Option<String>, RootedError> {
    if let Some(text) = c.string_text(value)? {
        return Ok(Some(text));
    }
    if !is_url_like(c, value)? {
        return Ok(None);
    }
    let href = get(c, value, "href")?;
    c.string_text(href)
}
