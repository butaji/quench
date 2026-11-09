//! Node's legacy URL API (lib/url.js): the `Url` class with `parse`, `format`, `resolve`,
//! `resolveObject` and `parseHost`, and the module functions over it.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError, ValueType};

type Context<'a> = NativeContext<'a, NodeHost>;

/// The `Url` fields, in the order its constructor creates them.
const FIELDS: [&str; 12] = [
    "protocol", "slashes", "auth", "host", "port", "hostname", "hash", "search", "query", "pathname", "path", "href",
];
/// `hostnameMaxLen`.
const HOSTNAME_MAX_LENGTH: usize = 255;
/// Protocols whose URLs never have a hostname.
const HOSTLESS: &[&str] = &["javascript", "javascript:"];
/// Protocols that may hold characters `autoEscapeStr` would escape.
const UNSAFE: &[&str] = &["javascript", "javascript:"];
/// Protocols that always contain a `//` bit.
const SLASHED: &[&str] = &[
    "http", "http:", "https", "https:", "ftp", "ftp:", "gopher", "gopher:", "file", "file:", "ws", "ws:", "wss", "wss:",
];
/// `forbiddenHostChars` and, for IPv6, `forbiddenHostCharsIpv6`.
const FORBIDDEN_HOST: &[char] = &['\0', '\t', '\n', '\r', ' ', '#', '%', '/', ':', '<', '>', '?', '@', '[', '\\', ']', '^', '|'];
const FORBIDDEN_HOST_IPV6: &[char] = &['\0', '\t', '\n', '\r', ' ', '#', '%', '/', '<', '>', '?', '@', '\\', '^', '|'];
/// Characters that end a hostname in the host scan, and characters never allowed in one.
const HOST_ENDING: &[char] = &['#', '/', '?'];
const NON_HOST: &[char] = &['\t', '\n', '\r', ' ', '"', '%', '\'', ';', '<', '>', '\\', '^', '`', '{', '|', '}'];
/// `escapedCodes`: what `autoEscapeStr` writes for each character it escapes.
const AUTO_ESCAPES: &[(char, &str)] = &[
    ('\t', "%09"),
    ('\n', "%0A"),
    ('\r', "%0D"),
    (' ', "%20"),
    ('"', "%22"),
    ('\'', "%27"),
    ('<', "%3C"),
    ('>', "%3E"),
    ('\\', "%5C"),
    ('^', "%5E"),
    ('`', "%60"),
    ('{', "%7B"),
    ('|', "%7C"),
    ('}', "%7D"),
];
/// The whitespace `parse` trims beyond the C0 controls and space.
const NO_BREAK_SPACE: char = '\u{a0}';
const ZERO_WIDTH_NO_BREAK_SPACE: char = '\u{feff}';
const INVALID_PORT_DEPRECATION: &str = "DEP0170";

/// A `Url` object's query: its text, or a parsed object.
#[derive(Clone, Copy)]
enum Query {
    Text,
    Object(RootId),
}

/// A `Url` object's fields as `parse`, `format` and `resolveObject` read them: a missing
/// field is null.
#[derive(Clone, Default)]
struct Legacy {
    protocol: Option<String>,
    slashes: Option<bool>,
    auth: Option<String>,
    host: Option<String>,
    port: Option<String>,
    hostname: Option<String>,
    hash: Option<String>,
    search: Option<String>,
    query: Option<(Query, String)>,
    pathname: Option<String>,
    path: Option<String>,
    href: Option<String>,
}

fn has_text(field: &Option<String>) -> bool {
    field.as_deref().is_some_and(|text| !text.is_empty())
}

fn slashed(protocol: Option<&str>) -> bool {
    protocol.is_some_and(|protocol| SLASHED.contains(&protocol))
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn text_field(c: &mut Context<'_>, object: RootId, name: &str) -> Result<Option<String>, RootedError> {
    let value = get(c, object, name)?;
    match type_of(c, value) {
        ValueType::Undefined | ValueType::Null => Ok(None),
        _ => c.to_string(value).map(Some),
    }
}

impl Legacy {
    fn read(c: &mut Context<'_>, object: RootId) -> Result<Self, RootedError> {
        let slashes = get(c, object, "slashes")?;
        let slashes = match type_of(c, slashes) {
            ValueType::Undefined | ValueType::Null => None,
            _ => Some(c.truthy_rooted(slashes)?),
        };
        let query = get(c, object, "query")?;
        let query = match type_of(c, query) {
            ValueType::Undefined | ValueType::Null => None,
            ValueType::Object => Some((Query::Object(query), String::new())),
            _ => Some((Query::Text, c.to_string(query)?)),
        };
        Ok(Self {
            protocol: text_field(c, object, "protocol")?,
            slashes,
            auth: text_field(c, object, "auth")?,
            host: text_field(c, object, "host")?,
            port: text_field(c, object, "port")?,
            hostname: text_field(c, object, "hostname")?,
            hash: text_field(c, object, "hash")?,
            search: text_field(c, object, "search")?,
            query,
            pathname: text_field(c, object, "pathname")?,
            path: text_field(c, object, "path")?,
            href: text_field(c, object, "href")?,
        })
    }

    fn write(&self, c: &mut Context<'_>, object: RootId) -> Result<(), RootedError> {
        for name in FIELDS {
            let text = |c: &mut Context<'_>, field: &Option<String>| match field {
                Some(text) => c.string_rooted(text),
                None => c.null(),
            };
            let value = match name {
                "protocol" => text(c, &self.protocol),
                "slashes" => match self.slashes {
                    Some(slashes) => c.boolean(slashes),
                    None => c.null(),
                },
                "auth" => text(c, &self.auth),
                "host" => text(c, &self.host),
                "port" => text(c, &self.port),
                "hostname" => text(c, &self.hostname),
                "hash" => text(c, &self.hash),
                "search" => text(c, &self.search),
                "query" => match &self.query {
                    None => c.null(),
                    Some((Query::Object(object), _)) => *object,
                    Some((_, text)) => c.string_rooted(text),
                },
                "pathname" => text(c, &self.pathname),
                "path" => text(c, &self.path),
                _ => text(c, &self.href),
            };
            let key = c.string_rooted(name);
            c.set_property_rooted(object, key, value, object)?;
        }
        Ok(())
    }

    /// `Url.prototype.parseHost`.
    fn parse_host(&mut self) {
        let mut host = self.host.clone().unwrap_or_default();
        if let Some(colon) = host.rfind(':') {
            let port = &host[colon + 1..];
            if port.bytes().all(|byte| byte.is_ascii_digit()) {
                if !port.is_empty() {
                    self.port = Some(port.to_owned());
                }
                host.truncate(colon);
            }
        }
        if !host.is_empty() {
            self.hostname = Some(host);
        }
    }

    /// `Url.prototype.format`.
    fn format(&self, c: &mut Context<'_>) -> Result<String, RootedError> {
        let mut auth = self.auth.clone().unwrap_or_default();
        if !auth.is_empty() {
            let units: Vec<u16> = auth.encode_utf16().collect();
            let encoded = super::querystring_shared_vm::encode_units(&units, auth_unreserved).unwrap_or(units);
            auth = String::from_utf16_lossy(&encoded) + "@";
        }
        let mut protocol = self.protocol.clone().unwrap_or_default();
        let mut pathname = self.pathname.clone().unwrap_or_default();
        let mut hash = self.hash.clone().unwrap_or_default();
        let mut host = String::new();
        if has_text(&self.host) {
            host = auth + self.host.as_deref().unwrap_or_default();
        } else if has_text(&self.hostname) {
            let hostname = self.hostname.as_deref().unwrap_or_default();
            host = if hostname.contains(':') && !is_ipv6(hostname) { format!("{auth}[{hostname}]") } else { auth + hostname };
            if has_text(&self.port) {
                host.push(':');
                host.push_str(self.port.as_deref().unwrap_or_default());
            }
        }
        let query = match &self.query {
            Some((Query::Object(object), _)) => {
                let querystring = crate::host::shared_vm::commonjs::builtin_require(c, "querystring")?;
                let stringify = get(c, querystring, "stringify")?;
                let text = c.call_rooted(stringify, querystring, &[*object])?;
                c.to_string(text)?
            }
            _ => String::new(),
        };
        let mut search = match &self.search {
            Some(search) if !search.is_empty() => search.clone(),
            _ if !query.is_empty() => format!("?{query}"),
            _ => String::new(),
        };
        if !protocol.is_empty() && !protocol.ends_with(':') {
            protocol.push(':');
        }
        pathname = pathname.replace('#', "%23").replace('?', "%3F");
        if self.slashes == Some(true) || slashed(Some(&protocol)) {
            if self.slashes == Some(true) || !host.is_empty() {
                if !pathname.is_empty() && !pathname.starts_with('/') {
                    pathname.insert(0, '/');
                }
                host = format!("//{host}");
            } else if protocol.starts_with("file") {
                "//".clone_into(&mut host);
            }
        }
        search = search.replace('#', "%23");
        if !hash.is_empty() && !hash.starts_with('#') {
            hash.insert(0, '#');
        }
        if !search.is_empty() && !search.starts_with('?') {
            search.insert(0, '?');
        }
        Ok(format!("{protocol}{host}{pathname}{search}{hash}"))
    }
}

/// `noEscapeAuth`: what `format` keeps unescaped in the auth part.
fn auth_unreserved(unit: u16) -> bool {
    u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric() || b"!'()*-._~:".contains(&byte))
}

fn is_ipv6(hostname: &str) -> bool {
    hostname.starts_with('[') && hostname.ends_with(']')
}

/// `autoEscapeStr`.
fn auto_escape(rest: &str) -> String {
    let mut escaped = String::with_capacity(rest.len());
    for character in rest.chars() {
        match AUTO_ESCAPES.iter().find(|(escaped, _)| *escaped == character) {
            Some((_, escape)) => escaped.push_str(escape),
            None => escaped.push(character),
        }
    }
    escaped
}

/// `protocolPattern`: `/^[a-z0-9.+-]+:/i`.
fn protocol_prefix(rest: &str) -> Option<&str> {
    let end = rest.bytes().position(|byte| !(byte.is_ascii_alphanumeric() || b".+-".contains(&byte)))?;
    (end > 0 && rest.as_bytes()[end] == b':').then(|| &rest[..=end])
}

/// `hostPattern`: `/^\/\/[^@/]+@[^@/]+/`.
fn has_auth_host(rest: &str) -> bool {
    let Some(tail) = rest.strip_prefix("//") else { return false };
    let Some(at) = tail.find(['@', '/']) else { return false };
    at > 0 && tail.as_bytes()[at] == b'@' && tail[at + 1..].chars().next().is_some_and(|next| next != '@' && next != '/')
}

/// `simplePathPattern`: `/^(\/\/?(?!\/)[^?\s]*)(\?[^\s]*)?$/`, as (pathname, search).
fn simple_path(rest: &str) -> Option<(&str, Option<&str>)> {
    let after_slash = rest.strip_prefix('/')?;
    let start = if after_slash.starts_with('/') { 2 } else { 1 };
    if rest[start..].starts_with('/') {
        return None;
    }
    let whitespace = |character: char| character.is_whitespace() || character == ZERO_WIDTH_NO_BREAK_SPACE;
    let path_end = rest[start..].find(|character: char| character == '?' || whitespace(character)).map_or(rest.len(), |end| end + start);
    let (pathname, search) = rest.split_at(path_end);
    if search.is_empty() {
        return Some((pathname, None));
    }
    (search.starts_with('?') && !search.chars().any(whitespace)).then_some((pathname, Some(search)))
}

/// What `Url.prototype.parse` was asked for.
struct ParseRequest {
    parse_query: bool,
    slashes_denote_host: bool,
}

/// `Url.prototype.parse(url, parseQueryString, slashesDenoteHost)` into `url`.
fn parse(c: &mut Context<'_>, url: &mut Legacy, input: &str, request: &ParseRequest) -> Result<(), RootedError> {
    let mut has_hash = false;
    let mut has_at = false;
    let mut rest = String::new();
    let (mut start, mut end): (Option<usize>, Option<usize>) = (None, None);
    let mut last = 0;
    let mut in_whitespace = false;
    let mut split = false;
    for (index, character) in input.char_indices() {
        let whitespace = (character as u32) < 33 || character == NO_BREAK_SPACE || character == ZERO_WIDTH_NO_BREAK_SPACE;
        if start.is_none() {
            if whitespace {
                continue;
            }
            start = Some(index);
            last = index;
        } else if in_whitespace {
            if !whitespace {
                end = None;
                in_whitespace = false;
            }
        } else if whitespace {
            end = Some(index);
            in_whitespace = true;
        }
        if !split {
            match character {
                '@' => has_at = true,
                '#' => {
                    has_hash = true;
                    split = true;
                }
                '?' => split = true,
                '\\' => {
                    rest.push_str(&input[last..index]);
                    rest.push('/');
                    last = index + 1;
                }
                _ => {}
            }
        } else if !has_hash && character == '#' {
            has_hash = true;
        }
    }
    if let Some(start) = start {
        if last == start {
            rest = match end {
                None => input[start..].to_owned(),
                Some(end) => input[start..end].to_owned(),
            };
        } else {
            match end {
                None if last < input.len() => rest.push_str(&input[last..]),
                Some(end) if last < end => rest.push_str(&input[last..end]),
                _ => {}
            }
        }
    }
    if !request.slashes_denote_host && !has_hash && !has_at {
        if let Some((pathname, search)) = simple_path(&rest) {
            url.path = Some(rest.clone());
            url.href = Some(rest.clone());
            url.pathname = Some(pathname.to_owned());
            match search {
                Some(search) => {
                    url.search = Some(search.to_owned());
                    url.query = Some(query(c, &search[1..], request.parse_query)?);
                }
                None if request.parse_query => {
                    url.search = None;
                    url.query = Some(query(c, "", true)?);
                }
                None => {}
            }
            return Ok(());
        }
    }
    let mut lower_protocol = None;
    if let Some(protocol) = protocol_prefix(&rest).map(str::to_owned) {
        let lower = protocol.to_lowercase();
        url.protocol = Some(lower.clone());
        lower_protocol = Some(lower);
        rest.drain(..protocol.len());
    }
    let lower = lower_protocol.as_deref();
    let hostless = lower.is_some_and(|protocol| HOSTLESS.contains(&protocol));
    let mut slashes = false;
    if request.slashes_denote_host || lower.is_some() || has_auth_host(&rest) {
        slashes = rest.starts_with("//");
        if slashes && !(lower.is_some() && hostless) {
            rest.drain(..2);
            url.slashes = Some(true);
        }
    }
    if !hostless && (slashes || (lower.is_some() && !slashed(lower))) {
        rest = parse_authority(c, url, rest, input)?;
    }
    if !lower.is_some_and(|protocol| UNSAFE.contains(&protocol)) {
        rest = auto_escape(&rest);
    }
    let hash_index = rest.find('#');
    if let Some(hash_index) = hash_index {
        url.hash = Some(rest[hash_index..].to_owned());
    }
    let before_hash = &rest[..hash_index.unwrap_or(rest.len())];
    let question_index = before_hash.find('?');
    match question_index {
        Some(question) => {
            url.search = Some(before_hash[question..].to_owned());
            url.query = Some(query(c, &before_hash[question + 1..], request.parse_query)?);
        }
        None if request.parse_query => {
            url.search = None;
            url.query = Some(query(c, "", true)?);
        }
        None => {}
    }
    let first = question_index.or(hash_index);
    match first {
        None if !rest.is_empty() => url.pathname = Some(rest.clone()),
        Some(first) if first > 0 => url.pathname = Some(rest[..first].to_owned()),
        _ => {}
    }
    if slashed(lower) && has_text(&url.hostname) && !has_text(&url.pathname) {
        url.pathname = Some("/".to_owned());
    }
    if has_text(&url.pathname) || has_text(&url.search) {
        url.path = Some(format!("{}{}", url.pathname.as_deref().unwrap_or_default(), url.search.as_deref().unwrap_or_default()));
    }
    url.href = Some(url.format(c)?);
    Ok(())
}

/// The query `parse` stores: the text, or `querystring.parse` of it.
fn query(c: &mut Context<'_>, text: &str, parse_query: bool) -> Result<(Query, String), RootedError> {
    if !parse_query {
        return Ok((Query::Text, text.to_owned()));
    }
    let querystring = crate::host::shared_vm::commonjs::builtin_require(c, "querystring")?;
    let parse = get(c, querystring, "parse")?;
    let text = c.string_rooted(text);
    let object = c.call_rooted(parse, querystring, &[text])?;
    Ok((Query::Object(object), String::new()))
}

/// The host part of `parse`: auth, host, port and hostname, returning the rest.
fn parse_authority(c: &mut Context<'_>, url: &mut Legacy, rest: String, input: &str) -> Result<String, RootedError> {
    let mut host_end = None;
    let mut at_sign = None;
    let mut non_host = None;
    for (index, character) in rest.char_indices() {
        if NON_HOST.contains(&character) {
            non_host.get_or_insert(index);
        } else if HOST_ENDING.contains(&character) {
            non_host.get_or_insert(index);
            host_end = Some(index);
        } else if character == '@' {
            at_sign = Some(index);
            non_host = None;
        }
        if host_end.is_some() {
            break;
        }
    }
    let mut start = 0;
    if let Some(at) = at_sign {
        url.auth = Some(decode_uri_component(c, &rest[..at])?);
        start = at + 1;
    }
    let mut rest = match non_host {
        None => {
            url.host = Some(rest[start..].to_owned());
            String::new()
        }
        Some(non_host) => {
            url.host = Some(rest[start..non_host].to_owned());
            rest[non_host..].to_owned()
        }
    };
    url.parse_host();
    let hostname = url.hostname.clone().unwrap_or_default();
    url.hostname = Some(hostname.clone());
    let ipv6 = is_ipv6(&hostname);
    if !ipv6 {
        if let Some(index) = hostname.find(['/', '\\', '#', '?', ':']) {
            if hostname.as_bytes()[index] == b':' {
                warn_invalid_port(c, input)?;
            }
            url.hostname = Some(hostname[..index].to_owned());
            rest = format!("/{}{rest}", &hostname[index..]);
        }
    }
    let mut hostname = url.hostname.clone().unwrap_or_default();
    hostname = if hostname.len() > HOSTNAME_MAX_LENGTH { String::new() } else { hostname.to_lowercase() };
    if !hostname.is_empty() {
        if ipv6 {
            if hostname.contains(FORBIDDEN_HOST_IPV6) {
                return Err(super::url_whatwg_shared_vm::invalid_url(c, input, None)?);
            }
        } else {
            hostname = idna::domain_to_ascii(&hostname).unwrap_or_default();
            if hostname.is_empty() || hostname.contains(FORBIDDEN_HOST) {
                return Err(super::url_whatwg_shared_vm::invalid_url(c, input, None)?);
            }
        }
    }
    let port = url.port.clone().filter(|port| !port.is_empty()).map(|port| format!(":{port}")).unwrap_or_default();
    url.host = Some(format!("{hostname}{port}"));
    if ipv6 {
        hostname.pop();
        hostname.remove(0);
        if !rest.starts_with('/') {
            rest.insert(0, '/');
        }
    }
    url.hostname = Some(hostname);
    Ok(rest)
}

/// DEP0170, once per process: `url.parse` accepted an invalid port.
fn warn_invalid_port(c: &mut Context<'_>, input: &str) -> Result<(), RootedError> {
    let primordials = super::util_shared_vm::primordials(c)?;
    let codes = get(c, primordials, "codesWarned")?;
    let key = c.string_rooted(INVALID_PORT_DEPRECATION);
    let warned = c.get_property_rooted(codes, key)?;
    if c.truthy_rooted(warned)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    c.set_property_rooted(codes, key, yes, codes)?;
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))?;
    let emit = get(c, process, "emitWarning")?;
    let message = c.string_rooted(&format!("The URL {input} is invalid. Future versions of Node.js will throw an error."));
    let kind = c.string_rooted("DeprecationWarning");
    c.call_rooted(emit, process, &[message, kind, key])?;
    Ok(())
}

fn decode_uri_component(c: &mut Context<'_>, text: &str) -> Result<String, RootedError> {
    let global = c.global_root()?;
    let decode = get(c, global, "decodeURIComponent")?;
    let text = c.string_rooted(text);
    let undefined = c.undefined();
    let decoded = c.call_rooted(decode, undefined, &[text])?;
    c.to_string(decoded)
}

/// `Url.prototype.resolveObject(relative)`.
fn resolve_object(c: &mut Context<'_>, source: &Legacy, relative: &Legacy) -> Result<Legacy, RootedError> {
    let mut result = source.clone();
    let mut relative = relative.clone();
    result.hash.clone_from(&relative.hash);
    if relative.href.as_deref() == Some("") {
        result.href = Some(result.format(c)?);
        return Ok(result);
    }
    if relative.slashes == Some(true) && !has_text(&relative.protocol) {
        let protocol = result.protocol.clone();
        result = relative;
        result.protocol = protocol;
        if slashed(result.protocol.as_deref()) && has_text(&result.hostname) && !has_text(&result.pathname) {
            result.pathname = Some("/".to_owned());
            result.path = Some("/".to_owned());
        }
        result.href = Some(result.format(c)?);
        return Ok(result);
    }
    if has_text(&relative.protocol) && relative.protocol != result.protocol {
        if !slashed(relative.protocol.as_deref()) {
            let mut copy = relative;
            copy.href = Some(copy.format(c)?);
            return Ok(copy);
        }
        result.protocol.clone_from(&relative.protocol);
        let file = matches!(relative.protocol.as_deref(), Some("file" | "file:"));
        if !has_text(&relative.host) && !file && !relative.protocol.as_deref().is_some_and(|protocol| HOSTLESS.contains(&protocol)) {
            let mut parts: Vec<String> = relative.pathname.clone().unwrap_or_default().split('/').map(str::to_owned).collect();
            relative.host = None;
            while !parts.is_empty() {
                let part = parts.remove(0);
                if !part.is_empty() {
                    relative.host = Some(part);
                    break;
                }
            }
            if !has_text(&relative.host) {
                relative.host = Some(String::new());
            }
            if !has_text(&relative.hostname) {
                relative.hostname = Some(String::new());
            }
            if parts.first().map(String::as_str) != Some("") {
                parts.insert(0, String::new());
            }
            if parts.len() < 2 {
                parts.insert(0, String::new());
            }
            result.pathname = Some(parts.join("/"));
        } else {
            result.pathname.clone_from(&relative.pathname);
        }
        result.search.clone_from(&relative.search);
        result.query.clone_from(&relative.query);
        result.host = Some(relative.host.clone().unwrap_or_default());
        result.auth.clone_from(&relative.auth);
        result.hostname = if has_text(&relative.hostname) { relative.hostname.clone() } else { relative.host.clone() };
        result.port.clone_from(&relative.port);
        if has_text(&result.pathname) || has_text(&result.search) {
            result.path = Some(format!("{}{}", result.pathname.as_deref().unwrap_or_default(), result.search.as_deref().unwrap_or_default()));
        }
        result.slashes = if result.slashes == Some(true) { Some(true) } else { relative.slashes };
        result.href = Some(result.format(c)?);
        return Ok(result);
    }
    let source_absolute = result.pathname.as_deref().is_some_and(|path| path.starts_with('/'));
    let relative_absolute = has_text(&relative.host) || relative.pathname.as_deref().is_some_and(|path| path.starts_with('/'));
    let mut must_end_absolute = relative_absolute || source_absolute || (has_text(&result.host) && has_text(&relative.pathname));
    let remove_all_dots = must_end_absolute;
    let split = |path: &Option<String>| -> Vec<String> {
        match path {
            Some(path) if !path.is_empty() => path.split('/').map(str::to_owned).collect(),
            _ => Vec::new(),
        }
    };
    let mut source_path = split(&result.pathname);
    let mut relative_path = split(&relative.pathname);
    let no_leading_slashes = has_text(&result.protocol) && !slashed(result.protocol.as_deref());
    if no_leading_slashes {
        result.hostname = Some(String::new());
        result.port = None;
        if has_text(&result.host) {
            let host = result.host.clone().unwrap_or_default();
            if source_path.first().map(String::as_str) == Some("") {
                source_path[0] = host;
            } else {
                source_path.insert(0, host);
            }
        }
        result.host = Some(String::new());
        if has_text(&relative.protocol) {
            relative.hostname = None;
            relative.port = None;
            result.auth = None;
            if has_text(&relative.host) {
                let host = relative.host.clone().unwrap_or_default();
                if relative_path.first().map(String::as_str) == Some("") {
                    relative_path[0] = host;
                } else {
                    relative_path.insert(0, host);
                }
            }
            relative.host = None;
        }
        must_end_absolute = must_end_absolute
            && (relative_path.first().map(String::as_str) == Some("") || source_path.first().map(String::as_str) == Some(""));
    }
    if relative_absolute {
        if relative.host.is_some() {
            if result.host != relative.host {
                result.auth = None;
            }
            result.host.clone_from(&relative.host);
            result.port.clone_from(&relative.port);
        }
        if relative.hostname.is_some() {
            if result.hostname != relative.hostname {
                result.auth = None;
            }
            result.hostname.clone_from(&relative.hostname);
        }
        result.search.clone_from(&relative.search);
        result.query.clone_from(&relative.query);
        source_path = relative_path;
    } else if !relative_path.is_empty() {
        source_path.pop();
        source_path.extend(relative_path);
        result.search.clone_from(&relative.search);
        result.query.clone_from(&relative.query);
    } else if relative.search.is_some() {
        if no_leading_slashes {
            let host = if source_path.is_empty() { None } else { Some(source_path.remove(0)) };
            result.hostname.clone_from(&host);
            result.host = host;
            split_auth_in_host(&mut result);
        }
        result.search.clone_from(&relative.search);
        result.query = relative.query;
        if result.pathname.is_some() || result.search.is_some() {
            result.path = Some(format!("{}{}", result.pathname.as_deref().unwrap_or_default(), result.search.as_deref().unwrap_or_default()));
        }
        result.href = Some(result.format(c)?);
        return Ok(result);
    }
    if source_path.is_empty() {
        result.pathname = None;
        result.path = if has_text(&result.search) { Some(format!("/{}", result.search.as_deref().unwrap_or_default())) } else { None };
        result.href = Some(result.format(c)?);
        return Ok(result);
    }
    let last = source_path.last().cloned().unwrap_or_default();
    let has_trailing_slash = ((has_text(&result.host) || has_text(&relative.host) || source_path.len() > 1) && (last == "." || last == ".."))
        || last.is_empty();
    let mut up = 0;
    let mut index = source_path.len();
    while index > 0 {
        index -= 1;
        match source_path[index].as_str() {
            "." => {
                source_path.remove(index);
            }
            ".." => {
                source_path.remove(index);
                up += 1;
            }
            _ if up > 0 => {
                source_path.remove(index);
                up -= 1;
            }
            _ => {}
        }
    }
    if !must_end_absolute && !remove_all_dots {
        for _ in 0..up {
            source_path.insert(0, "..".to_owned());
        }
    }
    if must_end_absolute && source_path.first().map(String::as_str) != Some("") && !source_path.first().is_some_and(|first| first.starts_with('/')) {
        source_path.insert(0, String::new());
    }
    if has_trailing_slash && !source_path.join("/").ends_with('/') {
        source_path.push(String::new());
    }
    let absolute = source_path.first().is_some_and(|first| first.is_empty() || first.starts_with('/'));
    if no_leading_slashes {
        let host = if absolute || source_path.is_empty() { String::new() } else { source_path.remove(0) };
        result.hostname = Some(host.clone());
        result.host = Some(host);
        split_auth_in_host(&mut result);
    }
    must_end_absolute = must_end_absolute || (has_text(&result.host) && !source_path.is_empty());
    if must_end_absolute && !absolute {
        source_path.insert(0, String::new());
    }
    if source_path.is_empty() {
        result.pathname = None;
        result.path = None;
    } else {
        result.pathname = Some(source_path.join("/"));
    }
    if result.pathname.is_some() || result.search.is_some() {
        result.path = Some(format!("{}{}", result.pathname.as_deref().unwrap_or_default(), result.search.as_deref().unwrap_or_default()));
    }
    if has_text(&relative.auth) {
        result.auth.clone_from(&relative.auth);
    }
    result.slashes = if result.slashes == Some(true) { Some(true) } else { relative.slashes };
    result.href = Some(result.format(c)?);
    Ok(result)
}

/// `authInHost`: `local@domain` left in the host of a non-slashed URL.
fn split_auth_in_host(result: &mut Legacy) {
    let Some(host) = result.host.clone() else { return };
    if host.find('@').is_some_and(|at| at > 0) {
        let mut parts = host.split('@');
        result.auth = parts.next().map(str::to_owned);
        let rest = parts.next().map(str::to_owned);
        result.host.clone_from(&rest);
        result.hostname = rest;
    }
}

/// The `Url` class, made once.
const URL_CLASS: &str = "\0node/url/Url";

pub(crate) fn class(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    if let Some(class) = c.host_mut().shared_state().borrow().builtin_cache.get(URL_CLASS).copied() {
        return Ok(class);
    }
    let class = c.host_constructor_named(crate::host::shared_vm::operation!("urlLegacyConstruct"), "Url")?;
    let key = c.string_rooted("length");
    let zero = c.number(0.0);
    c.define_data_property_rooted(class, key, zero, false, false, true)?;
    let prototype = get(c, class, "prototype")?;
    for (name, operation_name, length) in [
        ("parse", "urlLegacyParseMethod", 3.0),
        ("format", "urlLegacyFormatMethod", 0.0),
        ("resolve", "urlLegacyResolveMethod", 1.0),
        ("resolveObject", "urlLegacyResolveObjectMethod", 1.0),
        ("parseHost", "urlLegacyParseHostMethod", 0.0),
    ] {
        let function = c.host_function_named(operation(operation_name), name)?;
        let key = c.string_rooted("length");
        let length = c.number(length);
        c.define_data_property_rooted(function, key, length, false, false, true)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(prototype, key, function, true, true, true)?;
    }
    let class = c.retain(class)?;
    c.host_mut().shared_state().borrow_mut().builtin_cache.insert(URL_CLASS.to_owned(), class);
    Ok(class)
}

/// A new `Url` with every field null.
fn new_url(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let class = class(c)?;
    c.construct_rooted(class, class, &[])
}

pub(crate) fn construct(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Legacy::default().write(c, this)?;
    Ok(this)
}

fn is_url_instance(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let class = class(c)?;
    c.instance_of_rooted(value, class)
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

/// `url.parse(input, parseQueryString, slashesDenoteHost)` into `this`.
fn parse_into(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let input = argument(c, args, 0);
    let Some(text) = c.string_text(input)? else {
        return Err(super::util_shared_vm::argument_type_error(c, input, "url", "string")?);
    };
    let parse_query = argument(c, args, 1);
    let slashes_denote_host = argument(c, args, 2);
    let request = ParseRequest { parse_query: c.truthy_rooted(parse_query)?, slashes_denote_host: c.truthy_rooted(slashes_denote_host)? };
    let mut url = Legacy::read(c, this)?;
    parse(c, &mut url, &text, &request)?;
    url.write(c, this)
}

pub(crate) fn parse_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    parse_into(c, this, args)?;
    Ok(this)
}

/// `urlParse(url, parseQueryString, slashesDenoteHost)`.
pub(crate) fn parse_function(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = argument(c, args, 0);
    if is_url_instance(c, input)? {
        return Ok(input);
    }
    let url = new_url(c)?;
    parse_into(c, url, args)?;
    Ok(url)
}

pub(crate) fn format_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let url = Legacy::read(c, this)?;
    let text = url.format(c)?;
    Ok(c.string_rooted(&text))
}

pub(crate) fn parse_host_method(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let mut url = Legacy::read(c, this)?;
    url.parse_host();
    url.write(c, this)?;
    Ok(c.undefined())
}

/// A relative URL argument: a string parsed with slashes denoting a host, or an object.
fn relative_of(c: &mut Context<'_>, relative: RootId) -> Result<Legacy, RootedError> {
    match c.string_text(relative)? {
        Some(text) => {
            let mut url = Legacy::default();
            parse(c, &mut url, &text, &ParseRequest { parse_query: false, slashes_denote_host: true })?;
            Ok(url)
        }
        None => Legacy::read(c, relative),
    }
}

fn resolved_object(c: &mut Context<'_>, source: &Legacy, relative: RootId) -> Result<RootId, RootedError> {
    let relative = relative_of(c, relative)?;
    let result = resolve_object(c, source, &relative)?;
    let object = new_url(c)?;
    result.write(c, object)?;
    Ok(object)
}

pub(crate) fn resolve_object_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = Legacy::read(c, this)?;
    let relative = argument(c, args, 0);
    resolved_object(c, &source, relative)
}

pub(crate) fn resolve_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = Legacy::read(c, this)?;
    let relative = argument(c, args, 0);
    let relative = relative_of(c, relative)?;
    let result = resolve_object(c, &source, &relative)?;
    let text = result.format(c)?;
    Ok(c.string_rooted(&text))
}

/// `urlParse(source, false, true)` for `resolve` and `resolveObject`.
fn source_of(c: &mut Context<'_>, source: RootId) -> Result<Legacy, RootedError> {
    if is_url_instance(c, source)? {
        return Legacy::read(c, source);
    }
    let Some(text) = c.string_text(source)? else {
        return Err(super::util_shared_vm::argument_type_error(c, source, "url", "string")?);
    };
    let mut url = Legacy::default();
    parse(c, &mut url, &text, &ParseRequest { parse_query: false, slashes_denote_host: true })?;
    Ok(url)
}

/// `url.resolve(source, relative)`.
pub(crate) fn resolve_function(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = argument(c, args, 0);
    let source = source_of(c, source)?;
    let relative = argument(c, args, 1);
    let relative = relative_of(c, relative)?;
    let result = resolve_object(c, &source, &relative)?;
    let text = result.format(c)?;
    Ok(c.string_rooted(&text))
}

/// `url.resolveObject(source, relative)`.
pub(crate) fn resolve_object_function(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let source = argument(c, args, 0);
    let relative = argument(c, args, 1);
    if !c.truthy_rooted(source)? {
        return Ok(relative);
    }
    let source = source_of(c, source)?;
    resolved_object(c, &source, relative)
}

/// `url.format(urlObject, options)`.
pub(crate) fn format_function(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let object = argument(c, args, 0);
    if let Some(text) = c.string_text(object)? {
        let mut url = Legacy::default();
        parse(c, &mut url, &text, &ParseRequest { parse_query: false, slashes_denote_host: false })?;
        let text = url.format(c)?;
        return Ok(c.string_rooted(&text));
    }
    if type_of(c, object) != ValueType::Object {
        return Err(super::util_shared_vm::argument_type_error_with_expected(c, object, "urlObject", "one of type object or string")?);
    }
    if super::url_shared_vm::is_url_like(c, object)? {
        let options = argument(c, args, 1);
        let href = get(c, object, "href")?;
        return super::url_shared_vm::format_whatwg(c, href, options);
    }
    let url = Legacy::read(c, object)?;
    let text = url.format(c)?;
    Ok(c.string_rooted(&text))
}
