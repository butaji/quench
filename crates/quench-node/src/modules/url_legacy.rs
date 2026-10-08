//! Runtime-neutral parsing and formatting for Node's legacy URL API.
//!
//! The legacy Value facade and shared NodeHost adapter both lower this data
//! into their own runtime representation. WHATWG URL parsing remains owned by
//! `url_whatwg`.

use std::collections::BTreeMap;

pub(crate) const LEGACY_URL_FIELD_ORDER: &[&str] = &[
    "protocol", "slashes", "auth", "host", "port", "hostname", "hash", "search", "query",
    "pathname", "path", "href",
];

#[derive(Clone, Debug)]
pub(crate) enum LegacyUrlField {
    Null,
    Boolean(bool),
    Text(String),
}

#[derive(Clone, Debug)]
pub(crate) struct LegacyUrlParts {
    pub fields: Vec<(&'static str, LegacyUrlField)>,
    pub query: Option<String>,
    pub query_object: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum LegacyUrlParseError {
    Invalid { code: String, input: String },
    MalformedUri,
}

/// Parse Node's legacy URL fields without constructing either engine's values.
pub(crate) fn parse_legacy_parts(input: &str) -> Result<LegacyUrlParts, LegacyUrlParseError> {
    let raw_url = input.trim_matches(|character: char| character <= '\u{20}');
    if let Some(error) = invalid_legacy_authority(raw_url) {
        return Err(error);
    }
    let url = if let Some((head, fragment)) = raw_url.split_once('#') {
        format!("{}#{}", normalize_legacy_input(head), fragment)
    } else {
        normalize_legacy_input(raw_url)
    };
    let mut parsed = BTreeMap::new();
    let mut query = None;
    let mut query_object = false;

    if url.starts_with('<') {
        let pathname = encode_path_component(&url);
        parsed.insert("href".into(), pathname.clone());
        parsed.insert("pathname".into(), pathname.clone());
        parsed.insert("path".into(), pathname);
    } else if url.starts_with('[') && url.ends_with(']') {
        parsed.insert("pathname".into(), url.clone());
        parsed.insert("path".into(), url.clone());
        parsed.insert("href".into(), url);
    } else if is_bare_protocol_relative(&url) {
        parsed.insert("href".into(), url.clone());
        parsed.insert("pathname".into(), url.clone());
        parsed.insert("path".into(), url);
    } else if !url.contains(':') && !url.starts_with("//") {
        query_object = true;
        parse_relative_path(&url, &mut parsed, &mut query);
    } else {
        query_object = true;
        parsed = parse_legacy_authority(&url, raw_url, &mut query)?;
    }

    let fields = LEGACY_URL_FIELD_ORDER
        .iter()
        .map(|key| {
            let value = match parsed.remove(*key) {
                Some(value) if *key == "slashes" && value == "true" => {
                    LegacyUrlField::Boolean(true)
                }
                Some(value) => LegacyUrlField::Text(value),
                None => LegacyUrlField::Null,
            };
            (*key, value)
        })
        .collect();
    Ok(LegacyUrlParts {
        fields,
        query,
        query_object,
    })
}

fn is_bare_protocol_relative(url: &str) -> bool {
    url.strip_prefix("//")
        .is_some_and(|rest| !rest.contains('@') && !rest.contains(':') && !rest.contains('/'))
}

fn parse_relative_path(
    url: &str,
    parsed: &mut BTreeMap<String, String>,
    query_source: &mut Option<String>,
) {
    let (without_hash, hash) = url
        .split_once('#')
        .map_or((url, None), |(path, hash)| (path, Some(hash)));
    let (pathname, query) = without_hash
        .split_once('?')
        .map_or((without_hash, None), |(path, query)| (path, Some(query)));
    let pathname = encode_path_component(pathname);
    let search = query.map(|value| format!("?{}", encode_query_component(value)));
    let hash = hash.map(|value| format!("#{}", encode_path_component(value)));
    let path = format!("{pathname}{}", search.as_deref().unwrap_or_default());
    parsed.insert("pathname".into(), pathname);
    parsed.insert("path".into(), path.clone());
    parsed.insert(
        "href".into(),
        format!("{path}{}", hash.as_deref().unwrap_or_default()),
    );
    if let Some(search) = search {
        parsed.insert("search".into(), search);
    }
    if let Some(value) = query {
        *query_source = Some(value.to_string());
        parsed.insert("query".into(), encode_query_component(value));
    }
    if let Some(hash) = hash {
        parsed.insert("hash".into(), hash);
    }
}

fn parse_legacy_authority(
    url: &str,
    raw_url: &str,
    query_source: &mut Option<String>,
) -> Result<BTreeMap<String, String>, LegacyUrlParseError> {
    let mut parsed = legacy_parse_url(url);
    *query_source = parsed.get("query").cloned();
    for key in ["search", "query"] {
        if let Some(value) = parsed.get_mut(key) {
            *value = encode_query_component(value);
        }
    }
    normalize_legacy_authority_fields(&mut parsed, url, raw_url)?;
    complete_legacy_fields(&mut parsed, url);
    Ok(parsed)
}

fn normalize_legacy_authority_fields(
    parsed: &mut BTreeMap<String, String>,
    url: &str,
    raw_url: &str,
) -> Result<(), LegacyUrlParseError> {
    if let Some(protocol) = parsed.get_mut("protocol") {
        *protocol = protocol.to_ascii_lowercase();
    }
    if matches!(
        parsed.get("protocol").map(String::as_str),
        Some("http:" | "https:" | "ftp:" | "coap:" | "ws:" | "wss:")
    ) {
        normalize_legacy_host(parsed, raw_url)?;
    }
    if let Some(auth) = parsed.get_mut("auth") {
        *auth = auth.replace("%3A", ":").replace("%40", "@");
    }
    if let Some(hash) = parsed.get_mut("hash") {
        *hash = hash
            .replace('\\', "%5C")
            .replace(' ', "%20")
            .replace('<', "%3C")
            .replace('>', "%3E");
    }
    if matches!(
        parsed.get("protocol").map(String::as_str),
        Some("http:" | "https:" | "ftp:" | "coap:" | "ws:" | "wss:")
    ) && parsed.contains_key("host")
    {
        parsed.insert("slashes".into(), "true".into());
        if parsed
            .get("pathname")
            .is_none_or(|pathname| pathname.is_empty())
        {
            parsed.insert("pathname".into(), "/".into());
        }
    }
    if url.contains("://") && parsed.contains_key("protocol") {
        parsed.insert("slashes".into(), "true".into());
        if !parsed.contains_key("host") {
            parsed.insert("host".into(), String::new());
            parsed.insert("hostname".into(), String::new());
        }
    }
    if url.starts_with("//") && parsed.contains_key("host") {
        parsed.insert("slashes".into(), "true".into());
    }
    Ok(())
}

fn normalize_legacy_host(
    parsed: &mut BTreeMap<String, String>,
    raw_url: &str,
) -> Result<(), LegacyUrlParseError> {
    for key in ["host", "hostname"] {
        if let Some(value) = parsed.get_mut(key) {
            *value = value.to_ascii_lowercase();
        }
    }
    if let Some(hostname) = parsed.get_mut("hostname") {
        *hostname = idna::domain_to_ascii(hostname)
            .map_err(|_| invalid_url_error("ERR_INVALID_URL", raw_url))?;
    }
    let hostname = parsed.get("hostname").cloned();
    if let (Some(host), Some(hostname)) = (parsed.get_mut("host"), hostname) {
        if !host.starts_with('[') {
            if let Some(port) = host.rsplit_once(':').map(|(_, port)| port.to_string()) {
                *host = format!("{hostname}:{port}");
            } else {
                *host = hostname;
            }
        }
    }
    Ok(())
}

fn complete_legacy_fields(parsed: &mut BTreeMap<String, String>, url: &str) {
    if !parsed.contains_key("href") {
        parsed.insert(
            "href".into(),
            assemble_url(
                parsed
                    .get("protocol")
                    .map(String::as_str)
                    .unwrap_or_default(),
                parsed.get("auth").map(String::as_str).unwrap_or_default(),
                parsed.get("host").map(String::as_str).unwrap_or_default(),
                parsed
                    .get("pathname")
                    .map(String::as_str)
                    .unwrap_or_default(),
                parsed.get("search").map(String::as_str).unwrap_or_default(),
                parsed.get("hash").map(String::as_str).unwrap_or_default(),
                url.contains("://") || url.starts_with("//"),
            ),
        );
    }
    if !parsed.contains_key("path")
        && (parsed.contains_key("pathname") || parsed.contains_key("search"))
    {
        let pathname = parsed
            .get("pathname")
            .map(String::as_str)
            .unwrap_or_default();
        let search = parsed.get("search").map(String::as_str).unwrap_or_default();
        parsed.insert("path".into(), format!("{pathname}{search}"));
    }
}

fn invalid_legacy_authority(input: &str) -> Option<LegacyUrlParseError> {
    let (_, rest) = input.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.contains('\0') {
        return Some(invalid_url_error("ERR_INVALID_URL", input));
    }
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if let Some((auth, _)) = authority.rsplit_once('@') {
        if has_malformed_percent_encoding(auth) {
            return Some(LegacyUrlParseError::MalformedUri);
        }
    }
    if host.starts_with('[') {
        let Some(end) = host.find(']') else {
            return Some(invalid_url_error("ERR_INVALID_URL", input));
        };
        let suffix = &host[end + 1..];
        if suffix.is_empty() {
            return None;
        }
        if let Some(port) = suffix.strip_prefix(':') {
            if port.is_empty() || port.chars().all(|character| character.is_ascii_digit()) {
                return None;
            }
            return Some(invalid_url_error("ERR_INVALID_ARG_VALUE", input));
        }
        return Some(invalid_url_error("ERR_INVALID_URL", input));
    }
    let Some((hostname, port)) = host.rsplit_once(':') else {
        return None;
    };
    if hostname.is_empty() || port.is_empty() {
        return None;
    }
    if !port.chars().all(|character| character.is_ascii_digit()) {
        return Some(invalid_url_error("ERR_INVALID_ARG_VALUE", input));
    }
    if port.parse::<u32>().ok().is_some_and(|value| value > 65_535) {
        return Some(invalid_url_error("ERR_INVALID_ARG_VALUE", input));
    }
    None
}

fn invalid_url_error(code: &str, input: &str) -> LegacyUrlParseError {
    LegacyUrlParseError::Invalid {
        code: code.to_string(),
        input: input.to_string(),
    }
}

fn has_malformed_percent_encoding(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            decoded.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return true;
        }
        let Some(high) = percent_hex(bytes[index + 1]) else {
            return true;
        };
        let Some(low) = percent_hex(bytes[index + 2]) else {
            return true;
        };
        decoded.push((high << 4) | low);
        index += 3;
    }
    std::str::from_utf8(&decoded).is_err()
}

fn percent_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn normalize_legacy_input(value: &str) -> String {
    let Some((head, query)) = value.split_once('?') else {
        return value.replace("http:\\\\\\\\", "http://").replace('\\', "/");
    };
    format!(
        "{}?{}",
        head.replace("http:\\\\\\\\", "http://").replace('\\', "/"),
        query
    )
}

fn encode_query_component(value: &str) -> String {
    value
        .replace('"', "%22")
        .replace('\\', "%5C")
        .replace(' ', "%20")
        .replace('\'', "%27")
        .replace('^', "%5E")
        .replace('`', "%60")
        .replace('{', "%7B")
        .replace('}', "%7D")
        .replace('|', "%7C")
}

pub(crate) fn protocol_uses_authority_slashes(protocol: &str) -> bool {
    let protocol = protocol.trim_end_matches(':');
    matches!(
        protocol.to_ascii_lowercase().as_str(),
        "http" | "https" | "ftp" | "gopher" | "file" | "ws" | "wss"
    )
}

pub(crate) fn assemble_url(
    protocol: &str,
    auth: &str,
    host: &str,
    pathname: &str,
    query: &str,
    hash: &str,
    slashes: bool,
) -> String {
    let mut out = String::new();
    let host = if host.len() > 255 { "" } else { host };
    out.push_str(protocol);
    if !protocol.is_empty() && !out.ends_with(':') {
        out.push(':');
    }
    if slashes {
        out.push_str("//");
    }
    if !host.is_empty() || !auth.is_empty() {
        if !auth.is_empty() {
            out.push_str(&encode_auth(&auth));
            out.push('@');
        }
        out.push_str(host);
    }
    if (!host.is_empty() || !auth.is_empty()) && !pathname.is_empty() && !pathname.starts_with('/')
    {
        out.push('/');
    }
    out.push_str(pathname);
    if pathname.is_empty()
        && slashes
        && (!host.is_empty() || !auth.is_empty())
        && (!query.is_empty() || !hash.is_empty())
    {
        out.push('/');
    }
    if !query.is_empty() {
        if !query.starts_with('?') {
            out.push('?');
        }
        out.push_str(&query.replace('#', "%23"));
    }
    if !hash.is_empty() {
        if !hash.starts_with('#') {
            out.push('#');
        }
        out.push_str(hash);
    }
    out
}

fn encode_auth(auth: &str) -> String {
    let mut out = String::new();
    for byte in auth.as_bytes() {
        if byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':' | b'%' | b'\'')
        {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub(crate) fn legacy_parse_url(url: &str) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut rest = url;
    if let Some(i) = rest.find('#') {
        out.insert("hash".into(), rest[i..].to_string());
        rest = &rest[..i];
    }
    if let Some(i) = rest.find('?') {
        out.insert("search".into(), rest[i..].to_string());
        out.insert("query".into(), rest[i + 1..].to_string());
        rest = &rest[..i];
    }
    let after_protocol = if rest.starts_with("//") {
        rest
    } else if let Some(i) = rest.find("://") {
        out.insert("protocol".into(), rest[..i + 1].to_string());
        &rest[i + 3..]
    } else if let Some(i) = rest.find(':') {
        out.insert("protocol".into(), rest[..i + 1].to_string());
        &rest[i + 1..]
    } else {
        rest
    };
    let after_protocol = after_protocol.strip_prefix("//").unwrap_or(after_protocol);
    if out
        .get("protocol")
        .is_some_and(|protocol| protocol.eq_ignore_ascii_case("javascript:"))
    {
        if !after_protocol.is_empty() {
            out.insert("pathname".into(), after_protocol.to_string());
        }
        return out;
    }
    // The legacy parser treats known authority schemes without `//` as
    // opaque paths (`http:this`), not as a host named `this`.
    if out.get("protocol").is_some_and(|protocol| {
        protocol_uses_authority_slashes(protocol) && !url.contains("://") && !url.starts_with("//")
    }) {
        if !after_protocol.is_empty() {
            out.insert("pathname".into(), encode_path_component(after_protocol));
        }
        return out;
    }
    let split = if after_protocol.contains('@') {
        after_protocol
            .char_indices()
            .find(|(_, character)| *character == '/')
    } else {
        after_protocol
            .char_indices()
            .find(|(_, character)| matches!(character, '/' | ';' | ' ' | '"'))
    };
    let (auth_host, pathname) = match split {
        Some((index, character)) if matches!(character, '/' | ';') => (
            &after_protocol[..index],
            after_protocol[index..].to_string(),
        ),
        Some((index, _)) => (
            &after_protocol[..index],
            format!("/{}", &after_protocol[index..]),
        ),
        None => (after_protocol, String::new()),
    };
    let authority: String = auth_host
        .strip_prefix("//")
        .unwrap_or(auth_host)
        .chars()
        .filter(|character| !matches!(character, '\r' | '\n' | '\t'))
        .collect();
    if let Some(at) = authority.rfind('@') {
        let (a, host) = authority.split_at(at);
        out.insert("auth".into(), a.to_string());
        let host = &host[1..];
        split_host_port(host, &mut out);
    } else {
        split_host_port(&authority, &mut out);
    }
    if !pathname.is_empty() {
        out.insert("pathname".into(), encode_path_component(&pathname));
    }
    if let Some(s) = out.get("search") {
        out.insert("query".into(), s.strip_prefix('?').unwrap_or(s).to_string());
    }
    out
}

pub(crate) fn encode_path_component(path: &str) -> String {
    path.chars()
        .map(|character| match character {
            '\t' => "%09".to_string(),
            '\n' => "%0A".to_string(),
            '\r' => "%0D".to_string(),
            ' ' => "%20".to_string(),
            '"' => "%22".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            '`' => "%60".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            '\'' => "%27".to_string(),
            '{' => "%7B".to_string(),
            '}' => "%7D".to_string(),
            '|' => "%7C".to_string(),
            '\\' => "%5C".to_string(),
            '^' => "%5E".to_string(),
            _ => character.to_string(),
        })
        .collect()
}

fn split_host_port(host: &str, out: &mut BTreeMap<String, String>) {
    if let Some(end) = host.find(']') {
        if host.starts_with('[') {
            out.insert("hostname".into(), host[1..end].to_ascii_lowercase());
            if host.as_bytes().get(end + 1) == Some(&b':') {
                let port = &host[end + 2..];
                if !port.is_empty() {
                    out.insert("port".into(), port.to_string());
                }
                let bracketed = host[..=end].to_ascii_lowercase();
                out.insert(
                    "host".into(),
                    if port.is_empty() {
                        bracketed
                    } else {
                        format!("{bracketed}:{port}")
                    },
                );
            } else {
                out.insert("host".into(), host.to_ascii_lowercase());
            }
            return;
        }
    }
    if let Some((hostname, port)) = host.rsplit_once(':') {
        if port.is_empty() {
            out.insert("hostname".into(), hostname.to_string());
            out.insert("host".into(), hostname.to_string());
            return;
        }
        out.insert("hostname".into(), hostname.to_string());
        out.insert("port".into(), port.to_string());
        out.insert("host".into(), host.to_string());
    } else if !host.is_empty() {
        out.insert("hostname".into(), host.to_string());
        out.insert("host".into(), host.to_string());
    }
}
