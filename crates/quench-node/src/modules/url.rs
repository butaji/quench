//! `url` module — legacy `url.parse/format/resolve` + WHATWG
//! `URL` / `URLSearchParams` supplied as native Rust objects.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quench_runtime::execute::VmError;
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

pub fn format(args: &[Value]) -> String {
    let (protocol, auth, host, pathname, query, hash) = read_url_parts(args);
    assemble_url(&protocol, &auth, &host, &pathname, &query, &hash)
}

fn read_url_parts(args: &[Value]) -> (String, String, String, String, String, String) {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let mut protocol = String::new();
    let mut auth = String::new();
    let mut host = String::new();
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
            "hostname" if host.is_empty() => host = value,
            "port" if host.is_empty() && !value.is_empty() => {
                host.push(':');
                host.push_str(&value);
            }
            "pathname" => pathname = value,
            "search" => query = value,
            "query" => query = value,
            "hash" => hash = value,
            _ => {}
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
) -> String {
    let mut out = String::new();
    out.push_str(protocol);
    if !protocol.is_empty() && !out.ends_with(':') {
        out.push(':');
    }
    if !host.is_empty() || !auth.is_empty() {
        out.push_str("//");
        if !auth.is_empty() {
            out.push_str(auth);
            out.push('@');
        }
        out.push_str(host);
    }
    out.push_str(pathname);
    if !query.is_empty() {
        if !query.starts_with('?') {
            out.push('?');
        }
        out.push_str(query);
    }
    if !hash.is_empty() {
        if !hash.starts_with('#') {
            out.push('#');
        }
        out.push_str(hash);
    }
    out
}

pub fn new_url(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let url = args.first().map(value_to_string).unwrap_or_default();
    let base = args.get(1).map(value_to_string);
    let parsed = match base {
        Some(b) => legacy_parse_url(&legacy_resolve(&b, &url)),
        None => legacy_parse_url(&url),
    };
    let mut out = Vec::new();
    for (k, v) in parsed.into_iter() {
        out.push((k, Value::String(v)));
    }
    out.push((
        "toString".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("url:toString", 0x0505)),
    ));
    Ok(host_api::object(out))
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
    let (auth_host, pathname) = match rest.find('/') {
        Some(i) => (&rest[..i], rest[i..].to_string()),
        None => (rest, String::new()),
    };
    let after_protocol = match auth_host.find("://") {
        Some(i) => {
            out.insert("protocol".into(), auth_host[..i + 3].to_string());
            &auth_host[i + 3..]
        }
        None => auth_host,
    };
    if let Some(at) = after_protocol.find('@') {
        let (a, host) = after_protocol.split_at(at);
        out.insert("auth".into(), a.to_string());
        let host = &host[1..];
        split_host_port(host, &mut out);
    } else {
        split_host_port(after_protocol, &mut out);
    }
    if !pathname.is_empty() {
        out.insert("pathname".into(), pathname);
    }
    if let Some(s) = out.get("search") {
        out.insert("query".into(), s.trim_start_matches('?').to_string());
    }
    out
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
        _ => String::new(),
    }
}

pub fn build_root() -> Value {
    crate::host::namespace_object(vec![
        (
            "parse",
            crate::host::capability(crate::registry::NodeSpec::new("require:url:parse", 0x0500)),
        ),
        (
            "format",
            crate::host::capability(crate::registry::NodeSpec::new("require:url:format", 0x0501)),
        ),
        (
            "resolve",
            crate::host::capability(crate::registry::NodeSpec::new(
                "require:url:resolve",
                0x0502,
            )),
        ),
        (
            "URL",
            crate::host::capability(crate::registry::NodeSpec::new("require:url:URL", 0x0503)),
        ),
        (
            "URLSearchParams",
            crate::host::capability(crate::registry::NodeSpec::new(
                "require:url:URLSearchParams",
                0x0504,
            )),
        ),
        (
            "pathToFileURL",
            crate::host::capability(crate::registry::SPEC_URL_PATH_TO_FILE_URL),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

/// `url.pathToFileURL` — minimal POSIX form: `file://` + absolute path.
pub fn path_to_file_url(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let path = args.first().map(value_to_string).unwrap_or_default();
    let cwd = value_to_string(&crate::modules::process::cwd(state, &[])?);
    let absolute = if path.starts_with('/') {
        path
    } else {
        format!("{cwd}/{path}")
    };
    let mut encoded = String::new();
    for c in absolute.chars() {
        if c.is_ascii_alphanumeric() || "-._~/".contains(c) {
            encoded.push(c);
        } else {
            encoded.push_str(&format!("%{:02X}", c as u32));
        }
    }
    new_url(state, &[Value::String(format!("file://{encoded}"))])
}
