//! `internalBinding('url')` (src/node_url.cc): WHATWG URL parsing and setters for Node's
//! lib/internal/url.js. The `url` crate stands in for ada; both implement the WHATWG URL
//! Standard, and ada's component offsets are derived from the crate's.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use url::Url;
type Context<'a> = NativeContext<'a, NodeHost>;

/// `kURLComponentsLength`: the slots of `urlComponents`.
const COMPONENTS: usize = 9;
/// ada's `uint32_t(-1)` for an omitted component.
const OMITTED: u32 = u32::MAX;
/// The `urlComponents` slot of each ada component (`UpdateComponents`).
mod slot {
    pub(super) const PROTOCOL_END: usize = 0;
    pub(super) const USERNAME_END: usize = 1;
    pub(super) const HOST_START: usize = 2;
    pub(super) const HOST_END: usize = 3;
    pub(super) const PORT: usize = 4;
    pub(super) const PATHNAME_START: usize = 5;
    pub(super) const SEARCH_START: usize = 6;
    pub(super) const HASH_START: usize = 7;
    pub(super) const SCHEME_TYPE: usize = 8;
}

/// `ada::scheme::type`.
fn scheme_type(scheme: &str) -> u32 {
    match scheme {
        "http" => 0,
        "https" => 2,
        "ws" => 3,
        "ftp" => 4,
        "wss" => 5,
        "file" => 6,
        _ => 1,
    }
}

/// `url_update_action` (lib/internal/url.js `updateActions`).
#[derive(Clone, Copy)]
enum Action {
    Protocol,
    Host,
    Hostname,
    Port,
    Username,
    Password,
    Pathname,
    Search,
    Hash,
    Href,
}

impl Action {
    const ALL: [Self; 10] = [
        Self::Protocol,
        Self::Host,
        Self::Hostname,
        Self::Port,
        Self::Username,
        Self::Password,
        Self::Pathname,
        Self::Search,
        Self::Hash,
        Self::Href,
    ];

    /// Apply the WHATWG setter; `false` when the setter rejects the value.
    fn apply(self, url: &mut Url, value: &str) -> bool {
        use url::quirks;
        match self {
            Self::Protocol => quirks::set_protocol(url, value).is_ok(),
            Self::Host => quirks::set_host(url, value).is_ok(),
            Self::Hostname => quirks::set_hostname(url, value).is_ok(),
            Self::Port => quirks::set_port(url, value).is_ok(),
            Self::Username => quirks::set_username(url, value).is_ok(),
            Self::Password => quirks::set_password(url, value).is_ok(),
            Self::Pathname => {
                quirks::set_pathname(url, value);
                true
            }
            Self::Search => {
                quirks::set_search(url, value);
                true
            }
            Self::Hash => {
                quirks::set_hash(url, value);
                true
            }
            Self::Href => quirks::set_href(url, value).is_ok(),
        }
    }
}

/// ada's `url_components` for a parsed URL. ada's `host_start` names the `@` ending
/// credentials, and its `pathname_start` follows the `/.` that keeps a hostless path
/// starting with `//` from reading as a host.
fn components(url: &Url) -> [u32; COMPONENTS] {
    let parts = url::quirks::internal_components(url);
    let length = url.as_str().len() as u32;
    let path_end = parts.query_start.or(parts.fragment_start).unwrap_or(length);
    let credentials = !url.username().is_empty() || url.password().is_some();
    let mut values = [0; COMPONENTS];
    values[slot::PROTOCOL_END] = parts.scheme_end + 1;
    values[slot::USERNAME_END] = parts.username_end;
    values[slot::HOST_START] = parts.host_start - u32::from(credentials);
    values[slot::HOST_END] = parts.host_end;
    values[slot::PORT] = parts.port.map_or(OMITTED, u32::from);
    values[slot::PATHNAME_START] = path_end - url.path().len() as u32;
    values[slot::SEARCH_START] = parts.query_start.unwrap_or(OMITTED);
    values[slot::HASH_START] = parts.fragment_start.unwrap_or(OMITTED);
    values[slot::SCHEME_TYPE] = scheme_type(url.scheme());
    values
}

const NATIVES: &[(&str, &str)] = &[
    ("domainToASCII", "urlBindingDomainToAscii"),
    ("domainToUnicode", "urlBindingDomainToUnicode"),
    ("format", "urlBindingFormat"),
    ("getOrigin", "urlBindingGetOrigin"),
    ("parse", "urlBindingParse"),
    ("pathToFileURL", "urlBindingPathToFileUrl"),
    ("update", "urlBindingUpdate"),
    ("canParse", "urlBindingCanParse"),
];

pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:url", |c| {
        c.with_startup_intrinsics(|c| {
            let binding = c.object_rooted()?;
            let global = c.global_root()?;
            let constructor = c.get_named_property_rooted(global, "Uint32Array")?;
            let length = c.number(COMPONENTS as f64);
            let components = c.construct_rooted(constructor, constructor, &[length])?;
            put(c, binding, "urlComponents", components)?;
            for &(name, operation) in NATIVES {
                let operation = crate::host::shared_vm::operation(operation);
                let function = c.host_function_named_with_data(operation, name, components)?;
                put(c, binding, name, function)?;
            }
            Ok(binding)
        })
    })
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

/// A string argument as UTF-8 (`Utf8Value`); `None` for a non-string.
fn text(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<Option<String>, RootedError> {
    let value = arg(c, args, index);
    c.string_text(value)
}

fn required_text(c: &mut Context<'_>, args: &[RootId], index: usize) -> Result<String, RootedError> {
    text(c, args, index)?.ok_or_else(|| RootedError::host("url binding expects a string argument"))
}

/// Record `url`'s components in `urlComponents` and return its href.
fn publish(c: &mut Context<'_>, url: &Url) -> Result<RootId, RootedError> {
    let slots = c.host_function_data()?;
    let bytes: Vec<u8> = components(url).iter().flat_map(|value| value.to_ne_bytes()).collect();
    c.with_buffer_bytes_mut_rooted(slots, |slots| slots.copy_from_slice(&bytes));
    Ok(c.string_rooted(url.as_str()))
}

/// `ThrowInvalidURL`: `ERR_INVALID_URL` with the `input` and, when given, the `base`.
fn invalid_url(c: &mut Context<'_>, input: Option<&str>, base: Option<&str>) -> RootedError {
    let build = |c: &mut Context<'_>| -> Result<RootedError, RootedError> {
        let error = c.type_error_rooted("Invalid URL")?;
        for (name, value) in [("code", Some("ERR_INVALID_URL")), ("input", input), ("base", base)] {
            if let Some(value) = value {
                let value = c.string_rooted(value);
                put(c, error, name, value)?;
            }
        }
        Ok(c.throw(error))
    };
    build(c).unwrap_or_else(|error| error)
}

fn parse_with_base(input: &str, base: Option<&Url>) -> Option<Url> {
    Url::options().base_url(base).parse(input).ok()
}

/// `parse(input, base, raiseException)`.
pub(crate) fn parse(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = required_text(c, args, 0)?;
    let base_text = text(c, args, 1)?;
    let raise = arg(c, args, 2);
    let raise = c.rooted_value(raise).and_then(|value| value.as_bool()).unwrap_or(false);
    let base = match &base_text {
        Some(base) => match Url::parse(base) {
            Ok(base) => Some(base),
            Err(_) if raise => return Err(invalid_url(c, Some(&input), Some(base))),
            Err(_) => return Ok(c.undefined()),
        },
        None => None,
    };
    match parse_with_base(&input, base.as_ref()) {
        Some(url) => publish(c, &url),
        None if raise => Err(invalid_url(c, Some(&input), base_text.as_deref())),
        None => Ok(c.undefined()),
    }
}

/// `canParse(input, base)`.
pub(crate) fn can_parse(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = arg(c, args, 0);
    let input = c.to_string(input)?;
    let base = match args.get(1).copied() {
        Some(base) if c.rooted_value(base).is_some_and(|value| !value.is_undefined()) => Some(c.to_string(base)?),
        _ => None,
    };
    let parsed = match base {
        Some(base) => Url::parse(&base).ok().and_then(|base| parse_with_base(&input, Some(&base))),
        None => parse_with_base(&input, None),
    };
    Ok(c.boolean(parsed.is_some()))
}

/// `update(href, action, value)`: the new href, or false when the href or value is rejected.
pub(crate) fn update(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let href = required_text(c, args, 0)?;
    let action = arg(c, args, 1);
    let action = c.to_number(action)? as usize;
    let value = required_text(c, args, 2)?;
    let action = *Action::ALL.get(action).ok_or_else(|| RootedError::host("Unsupported URL update action"))?;
    let Ok(mut url) = Url::parse(&href) else {
        return Ok(c.boolean(false));
    };
    if !action.apply(&mut url, &value) {
        return Ok(c.boolean(false));
    }
    publish(c, &url)
}

/// The URL's host in Unicode (`ada::idna::to_unicode`).
fn unicode_host(host: &str) -> String {
    idna::domain_to_unicode(host).0
}

/// `format(href, hash, unicode, search, auth)`.
pub(crate) fn format(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let href = required_text(c, args, 0)?;
    let mut flags = [false; 4];
    for (flag, index) in flags.iter_mut().zip(1..) {
        let value = arg(c, args, index);
        *flag = c.rooted_value(value).and_then(|value| value.as_bool()).unwrap_or(false);
    }
    let [hash, unicode, search, auth] = flags;
    let Ok(mut url) = Url::parse(&href) else {
        return Ok(arg(c, args, 0));
    };
    if !hash {
        url.set_fragment(None);
    }
    if !search {
        url.set_query(None);
    }
    if !auth {
        let _ = url.set_username("");
        let _ = url.set_password(None);
    }
    let mut result = url.as_str().to_owned();
    if let Some(host) = url.host_str().filter(|host| unicode && !host.is_empty()) {
        let parts = url::quirks::internal_components(&url);
        result.replace_range(parts.host_start as usize..parts.host_end as usize, &unicode_host(host));
    }
    Ok(c.string_rooted(&result))
}

/// `getOrigin(input)`.
pub(crate) fn get_origin(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = required_text(c, args, 0)?;
    match Url::parse(&input) {
        Ok(url) => Ok(c.string_rooted(&url.origin().ascii_serialization())),
        Err(_) => Err(invalid_url(c, None, None)),
    }
}

/// The hostname a special-scheme URL takes for `domain`, the way `domainTo*` reach the
/// host parser; `None` when the setter rejects it.
fn special_hostname(domain: &str) -> Option<String> {
    const SPECIAL_SEED: &str = "ws://x";
    let mut url = Url::parse(SPECIAL_SEED).ok()?;
    url::quirks::set_hostname(&mut url, domain).ok()?;
    Some(url::quirks::hostname(&url).to_owned())
}

pub(crate) fn domain_to_ascii(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = required_text(c, args, 0)?;
    let host = if input.is_empty() { Some(String::new()) } else { special_hostname(&input) };
    Ok(c.string_rooted(&host.unwrap_or_default()))
}

pub(crate) fn domain_to_unicode(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = required_text(c, args, 0)?;
    let host = if input.is_empty() { Some(String::new()) } else { special_hostname(&input).map(|host| unicode_host(&host)) };
    Ok(c.string_rooted(&host.unwrap_or_default()))
}

/// RFC 1738's unsafe characters, which `EncodePathChars` percent-encodes.
const PATH_ESCAPES: &[(char, &str)] = &[
    ('\0', "%00"),
    ('\t', "%09"),
    ('\n', "%0A"),
    ('\r', "%0D"),
    (' ', "%20"),
    ('"', "%22"),
    ('#', "%23"),
    ('%', "%25"),
    ('?', "%3F"),
    ('[', "%5B"),
    ('\\', "%5C"),
    (']', "%5D"),
    ('^', "%5E"),
    ('|', "%7C"),
    ('~', "%7E"),
];
const FILE_URL_PREFIX: &str = "file://";

/// `EncodePathChars`: a file URL's text for `path`; Windows separators become `/`.
fn encode_path_chars(path: &str, windows: bool) -> String {
    let mut encoded = String::with_capacity(FILE_URL_PREFIX.len() + path.len());
    encoded.push_str(FILE_URL_PREFIX);
    for character in path.chars() {
        match PATH_ESCAPES.iter().find(|(escaped, _)| *escaped == character) {
            Some(('\\', _)) if windows => encoded.push('/'),
            Some((_, escape)) => encoded.push_str(escape),
            None => encoded.push(character),
        }
    }
    encoded
}

/// `pathToFileURL(path, windows, hostname)`.
pub(crate) fn path_to_file_url(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = required_text(c, args, 0)?;
    let windows = arg(c, args, 1);
    let windows = c.rooted_value(windows).and_then(|value| value.as_bool()).unwrap_or(false);
    let hostname = text(c, args, 2)?;
    let Ok(mut url) = Url::parse(&encode_path_chars(&path, windows)) else {
        return Err(invalid_url(c, Some(&path), None));
    };
    if let Some(hostname) = hostname.filter(|_| windows) {
        if url::quirks::set_hostname(&mut url, &hostname).is_err() {
            return Err(invalid_url(c, Some(&path), Some(&hostname)));
        }
    }
    publish(c, &url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(href: &str) -> [u32; COMPONENTS] {
        components(&Url::parse(href).unwrap())
    }

    #[test]
    fn components_follow_ada() {
        assert_eq!(of("https://user:pass@example.com:1234/foo/bar?baz#quux"), [6, 12, 17, 29, 1234, 34, 42, 46, 2]);
        assert_eq!(of("https://example.com"), [6, 8, 8, 19, OMITTED, 19, OMITTED, OMITTED, 2]);
        assert_eq!(of("mailto:x"), [7, 7, 7, 7, OMITTED, 7, OMITTED, OMITTED, 1]);
        assert_eq!(of("file:///tmp/a"), [5, 7, 7, 7, OMITTED, 7, OMITTED, OMITTED, 6]);
    }

    #[test]
    fn path_characters_are_escaped() {
        assert_eq!(encode_path_chars("/a b#?", false), "file:///a%20b%23%3F");
        assert_eq!(encode_path_chars("C:\\x", true), "file://C:/x");
    }
}
