//! `url` module — legacy `url.parse/format/resolve` + WHATWG
//! `URL` / `URLSearchParams` supplied as native Rust objects.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const URL_KEYS: &[&str] = &[
    "protocol", "auth", "host", "hostname", "port", "pathname", "search", "query", "hash",
];

const SEARCH_KEYS: &[&str] = &[
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z",
];

pub fn parse_handler(
    _state: &std::rc::Rc<std::cell::RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    parse(_state, args)
}
pub fn resolve_handler(
    _state: &std::rc::Rc<std::cell::RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let from = args.first().map(value_to_string).unwrap_or_default();
    let to = args.get(1).map(value_to_string).unwrap_or_default();
    Ok(Value::String(legacy_resolve(&from, &to)))
}

pub fn resolve(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    resolve_handler(_state, args)
}

pub fn parse(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let url = args.first().map(value_to_string).unwrap_or_default();
    let parsed = legacy_parse_url(&url);
    let mut out = Vec::new();
    for (k, v) in parsed {
        out.push((k, Value::String(v)));
    }
    Ok(host_api::object(out))
}

/// `url.format(urlObject[, options])` — port of Node's `urlFormat`:
/// strings go through the legacy parser, WHATWG `URL` instances through
/// the flag-driven serializer, other non-objects throw `ERR_INVALID_ARG_TYPE`.
pub fn format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if matches!(&obj, Value::String(s) if !execute::is_symbol(&obj)) {
        let Value::String(s) = obj else {
            return Ok(Value::String(String::new()));
        };
        let parsed = legacy_parse_url(&s);
        let get = |k: &str| parsed.get(k).cloned().unwrap_or_default();
        let formatted = assemble_url(
            &get("protocol"),
            &get("auth"),
            &get("host"),
            &get("pathname"),
            &get("search"),
            &get("hash"),
            s.contains("://"),
        );
        return Ok(Value::String(formatted));
    }
    if !is_object_arg(&obj) {
        return Err(invalid_arg_type_object(&obj));
    }
    if crate::modules::url_whatwg::is_url_instance(&obj) {
        return format_whatwg(&obj, args.get(1));
    }
    let (protocol, auth, host, pathname, query, hash) = read_url_parts(_state, args);
    let slashes = execute::is_truthy(&execute::get_property(&obj, "slashes"))
        || protocol_uses_authority_slashes(&protocol);
    Ok(Value::String(assemble_url(
        &protocol, &auth, &host, &pathname, &query, &hash, slashes,
    )))
}

fn protocol_uses_authority_slashes(protocol: &str) -> bool {
    let protocol = protocol.trim_end_matches(':');
    matches!(
        protocol.to_ascii_lowercase().as_str(),
        "http" | "https" | "ftp" | "gopher" | "file" | "ws" | "wss"
    )
}

pub(crate) fn is_object_arg(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) && !execute::is_symbol(value)
}

fn invalid_arg_type_object(value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The \"urlObject\" argument must be one of type Object or string.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

fn format_whatwg(obj: &Value, options: Option<&Value>) -> Result<Value, VmError> {
    let (mut fragment, mut unicode, mut search, mut auth) = (true, false, true, true);
    if let Some(options) = options {
        if !matches!(options, Value::Undefined) && !is_object_arg(options) {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".to_string(), Value::String("TypeError".to_string())),
                (
                    "message".to_string(),
                    Value::String(format!(
                        "The \"options\" argument must be of type object.{}",
                        crate::modules::util::invalid_arg_received(options)
                    )),
                ),
                (
                    "code".to_string(),
                    Value::String("ERR_INVALID_ARG_TYPE".to_string()),
                ),
            ])));
        }
        flag(options, "fragment", &mut fragment);
        flag(options, "unicode", &mut unicode);
        flag(options, "search", &mut search);
        flag(options, "auth", &mut auth);
    }
    let href = crate::modules::url_whatwg::parsed_of(Some(obj))?.get("href");
    Ok(Value::String(crate::modules::url_whatwg::format_href(
        &href, fragment, unicode, search, auth,
    )))
}

fn flag(options: &Value, key: &str, slot: &mut bool) {
    let value = execute::get_property(options, key);
    if !matches!(value, Value::Undefined | Value::Null) {
        *slot = execute::is_truthy(&value);
    }
}

fn read_url_parts(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> (String, String, String, String, String, String) {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let mut protocol = String::new();
    let mut auth = String::new();
    let mut host = String::new();
    let mut hostname = String::new();
    let mut port = String::new();
    let mut pathname = String::new();
    let mut query = String::new();
    let mut hash = String::new();
    for key in URL_KEYS {
        let raw = quench_runtime::vm::get_property(&obj, key);
        let value = value_to_string(&raw);
        if matches!(raw, Value::Undefined) {
            continue;
        }
        match *key {
            "protocol" => protocol = value,
            "auth" => auth = value,
            "host" => host = value,
            "hostname" => hostname = value,
            "port" => port = value,
            "pathname" => pathname = encode_path_component(&value),
            "search" => query = value,
            "query"
                if !value.is_empty()
                    && !matches!(
                        raw,
                        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
                    ) =>
            {
                query = value
            }
            "hash" => hash = value,
            _ => {}
        }
    }
    if host.is_empty() {
        host = hostname;
        if !port.is_empty() {
            if host.contains(':') && !host.starts_with('[') {
                host = format!("[{host}]");
            }
            host.push(':');
            host.push_str(&port);
        }
    }
    let query_value = quench_runtime::vm::get_property(&obj, "query");
    if matches!(
        query_value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        if let Ok(Value::String(serialized)) =
            crate::modules::querystring_stringify::stringify(state, None, &[query_value])
        {
            if !serialized.is_empty() {
                query = serialized;
            }
        }
    }
    (protocol, auth, host, pathname, query, hash)
}

fn assemble_url(
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
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':' | b'%') {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

pub fn new_search_params(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let init = args.first().cloned().unwrap_or(Value::Undefined);
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    match init {
        Value::String(s) => parse_into(&s, &mut map),
        Value::Object(_) => {
            for key in SEARCH_KEYS {
                let v = quench_runtime::vm::get_property(&init, key);
                if matches!(v, Value::Undefined) {
                    continue;
                }
                map.entry(key.to_string())
                    .or_default()
                    .push(value_to_string(&v));
            }
        }
        _ => {}
    }
    let mut out = Vec::new();
    for (k, vs) in map {
        let values = vs.into_iter().map(Value::String).collect::<Vec<_>>();
        out.push((k, host_api::array(values)));
    }
    Ok(host_api::object(out))
}

fn parse_into(s: &str, map: &mut BTreeMap<String, Vec<String>>) {
    for pair in s.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.find('=') {
            Some(i) => (pair[..i].to_string(), pair[i + 1..].to_string()),
            None => (pair.to_string(), String::new()),
        };
        map.entry(k).or_default().push(v);
    }
}

fn legacy_parse_url(url: &str) -> BTreeMap<String, String> {
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
    let after_protocol = if let Some(i) = rest.find("://") {
        out.insert("protocol".into(), rest[..i + 1].to_string());
        &rest[i + 3..]
    } else if let Some(i) = rest.find(':') {
        out.insert("protocol".into(), rest[..i + 1].to_string());
        &rest[i + 1..]
    } else {
        rest
    };
    let split = after_protocol
        .char_indices()
        .find(|(_, character)| matches!(character, '/' | ' ' | '"'));
    let (auth_host, pathname) = match split {
        Some((index, character)) if character == '/' => (
            &after_protocol[..index],
            after_protocol[index..].to_string(),
        ),
        Some((index, _)) => (
            &after_protocol[..index],
            format!("/{}", &after_protocol[index..]),
        ),
        None => (after_protocol, String::new()),
    };
    if let Some(at) = auth_host.find('@') {
        let (a, host) = auth_host.split_at(at);
        out.insert("auth".into(), a.to_string());
        let host = &host[1..];
        split_host_port(host, &mut out);
    } else {
        split_host_port(auth_host, &mut out);
    }
    if !pathname.is_empty() {
        out.insert("pathname".into(), encode_path_component(&pathname));
    }
    if let Some(s) = out.get("search") {
        out.insert("query".into(), s.trim_start_matches('?').to_string());
    }
    out
}

fn encode_path_component(path: &str) -> String {
    path.chars()
        .map(|character| match character {
            ' ' => "%20".to_string(),
            '"' => "%22".to_string(),
            '<' => "%3C".to_string(),
            '>' => "%3E".to_string(),
            '`' => "%60".to_string(),
            '#' => "%23".to_string(),
            '?' => "%3F".to_string(),
            _ => character.to_string(),
        })
        .collect()
}

fn split_host_port(host: &str, out: &mut BTreeMap<String, String>) {
    if let Some((hostname, port)) = host.rsplit_once(':') {
        out.insert("hostname".into(), hostname.to_string());
        out.insert("port".into(), port.to_string());
        out.insert("host".into(), host.to_string());
    } else if !host.is_empty() {
        out.insert("hostname".into(), host.to_string());
        out.insert("host".into(), host.to_string());
    }
}

fn legacy_resolve(from: &str, to: &str) -> String {
    if to.is_empty() {
        return from.to_string();
    }
    if to.contains("://") {
        return to.to_string();
    }
    if to.starts_with('?') || to.starts_with('#') {
        let base = if let Some(i) = from.find('?') {
            &from[..i]
        } else if let Some(i) = from.find('#') {
            &from[..i]
        } else {
            from
        };
        return format!("{base}{to}");
    }
    let (protocol, host_part) = split_protocol(from);
    let (host, base_path) = match host_part.find('/') {
        Some(i) => (host_part[..i].to_string(), host_part[i..].to_string()),
        None => (host_part.to_string(), "/".to_string()),
    };
    let base_dir = if base_path.ends_with('/') {
        base_path
    } else {
        let last = base_path.rfind('/').unwrap_or(0);
        base_path[..last + 1].to_string()
    };
    let combined = if to.starts_with('/') {
        format!("{}{to}", host)
    } else {
        format!("{base_dir}{to}")
    };
    let normalized = normalize_path(&combined);
    format!("{protocol}//{normalized}")
}

fn split_protocol(url: &str) -> (String, &str) {
    match url.find("://") {
        Some(i) => (url[..i + 3].to_string(), &url[i + 3..]),
        None => (String::new(), url),
    }
}

fn normalize_path(path: &str) -> String {
    let mut parts = Vec::new();
    let absolute = path.starts_with('/');
    let body = path.trim_start_matches('/');
    for part in body.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            parts.pop();
            continue;
        }
        parts.push(part);
    }
    let mut out = parts.join("/");
    if absolute {
        out.insert(0, '/');
    }
    out
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => execute::to_js_string(value).unwrap_or_default(),
    }
}

pub fn build_root(state: &Rc<RefCell<HostState>>) -> Value {
    let (url_class, _) = crate::modules::url_whatwg::url_class(state);
    crate::host::namespace_object(vec![
        spec_fn("parse", "require:url:parse", 0x0500),
        spec_fn("format", "require:url:format", 0x0501),
        spec_fn("resolve", "require:url:resolve", 0x0502),
        ("URL", url_class),
        spec_fn("URLSearchParams", "require:url:URLSearchParams", 0x0504),
        (
            "pathToFileURL",
            crate::host::capability(crate::registry::SPEC_URL_PATH_TO_FILE_URL),
        ),
        (
            "fileURLToPath",
            crate::host::capability(crate::registry::SPEC_URL_FILE_URL_TO_PATH),
        ),
        (
            "urlToHttpOptions",
            crate::host::capability(crate::registry::SPEC_URL_TO_HTTP_OPTIONS),
        ),
        (
            "domainToASCII",
            crate::host::capability(crate::registry::SPEC_URL_DOMAIN_TO_ASCII),
        ),
        (
            "domainToUnicode",
            crate::host::capability(crate::registry::SPEC_URL_DOMAIN_TO_UNICODE),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

fn spec_fn(name: &'static str, spec: &'static str, id: u16) -> (&'static str, Value) {
    (
        name,
        crate::host::capability(crate::registry::NodeSpec::new(spec, id)),
    )
}

// `url.pathToFileURL` lives in `url_file`; see `SPEC_URL_PATH_TO_FILE_URL`.
