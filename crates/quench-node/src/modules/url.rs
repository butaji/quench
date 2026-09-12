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

thread_local! {
    static LEGACY_URL_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

pub(crate) fn legacy_url_prototype() -> Value {
    LEGACY_URL_PROTOTYPE.with(|slot| {
        if let Some(value) = slot.borrow().clone() {
            return value;
        }
        let value = host_api::object(vec![
            (
                "resolveObject".into(),
                crate::host::capability(crate::registry::SPEC_URL_RESOLVE_OBJECT),
            ),
            (
                "resolve".into(),
                crate::host::capability(crate::registry::SPEC_URL_RESOLVE),
            ),
        ]);
        *slot.borrow_mut() = Some(value.clone());
        value
    })
}

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
    state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let vm_filename = execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "\0quench_vm_filename",
    );
    if !matches!(vm_filename, Value::String(ref path) if path.contains("node_modules")) {
        crate::modules::process::emit_warning(
            state,
            "DeprecationWarning",
            "`url.parse()` behavior is not standardized and prone to errors. Use the WHATWG URL API instead.",
            Some("DEP0169"),
            true,
        );
    }
    let raw_url =
        crate::modules::path::validate_string(args.first().unwrap_or(&Value::Undefined), "url")?
            .trim_matches(|character: char| character <= '\u{20}')
            .to_string();
    if let Some(error) = invalid_legacy_authority(&raw_url) {
        return Err(error);
    }
    let url = if let Some((head, fragment)) = raw_url.split_once('#') {
        format!("{}#{}", normalize_legacy_input(head), fragment)
    } else {
        normalize_legacy_input(&raw_url)
    };
    if url.starts_with('<') {
        let pathname = encode_path_component(&url);
        return Ok(legacy_object(vec![
            ("href".into(), Value::String(pathname.clone())),
            ("pathname".into(), Value::String(pathname.clone())),
            ("path".into(), Value::String(pathname)),
        ]));
    }
    if url.starts_with('[') && url.ends_with(']') {
        return Ok(legacy_object(vec![
            ("pathname".into(), Value::String(url.clone())),
            ("path".into(), Value::String(url.clone())),
            ("href".into(), Value::String(url)),
        ]));
    }
    if let Some(rest) = url.strip_prefix("//") {
        if !rest.contains('@') && !rest.contains(':') && !rest.contains('/') {
            return Ok(legacy_object(vec![
                ("href".into(), Value::String(url.clone())),
                ("pathname".into(), Value::String(url.clone())),
                ("path".into(), Value::String(url)),
            ]));
        }
    }
    if !url.contains(':') && !url.starts_with("//") {
        let (without_hash, hash) = url
            .split_once('#')
            .map_or((url.as_str(), None), |(path, hash)| (path, Some(hash)));
        let (pathname, query) = without_hash
            .split_once('?')
            .map_or((without_hash, None), |(path, query)| (path, Some(query)));
        let pathname = encode_path_component(pathname);
        let search = query.map(|value| format!("?{}", encode_query_component(value)));
        let hash = hash.map(|value| format!("#{}", encode_path_component(value)));
        let path = format!("{}{}", pathname, search.as_deref().unwrap_or_default());
        let href = format!("{}{}", path, hash.as_deref().unwrap_or_default());
        let mut entries = vec![
            ("pathname".into(), Value::String(pathname)),
            ("path".into(), Value::String(path)),
            ("href".into(), Value::String(href)),
        ];
        entries.push((
            "search".into(),
            search.clone().map_or(Value::Null, Value::String),
        ));
        let query_value = if args.get(1).is_some_and(execute::is_truthy) {
            crate::modules::querystring_parse::parse(
                state,
                None,
                &[Value::String(query.unwrap_or_default().to_string())],
            )?
        } else {
            query.map_or(Value::Null, |value| {
                Value::String(encode_query_component(value))
            })
        };
        entries.push(("query".into(), query_value));
        entries.push(("hash".into(), hash.map_or(Value::Null, Value::String)));
        return Ok(legacy_object(entries));
    }
    let mut parsed = legacy_parse_url(&url);
    for key in ["search", "query"] {
        if let Some(value) = parsed.get_mut(key) {
            *value = encode_query_component(value);
        }
    }
    if let Some(protocol) = parsed.get_mut("protocol") {
        *protocol = protocol.to_ascii_lowercase();
    }
    if matches!(
        parsed.get("protocol").map(String::as_str),
        Some("http:" | "https:" | "ftp:" | "coap:" | "ws:" | "wss:")
    ) {
        for key in ["host", "hostname"] {
            if let Some(value) = parsed.get_mut(key) {
                *value = value.to_ascii_lowercase();
            }
        }
        if let Some(hostname) = parsed.get_mut("hostname") {
            let ascii = idna::domain_to_ascii(hostname)
                .map_err(|_| legacy_url_error("ERR_INVALID_URL", &raw_url))?;
            *hostname = ascii;
        }
        let hostname_value = parsed.get("hostname").cloned();
        if let (Some(host), Some(hostname)) = (parsed.get_mut("host"), hostname_value) {
            if !host.starts_with('[') {
                if let Some(port) = host.rsplit_once(':').map(|(_, port)| port.to_string()) {
                    *host = format!("{hostname}:{port}");
                } else {
                    *host = hostname;
                }
            }
        }
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
    ) && parsed.get("host").is_some()
    {
        parsed.insert("slashes".into(), "true".into());
        if parsed
            .get("pathname")
            .is_none_or(|pathname| pathname.is_empty())
        {
            parsed.insert("pathname".into(), "/".into());
        }
    }
    if url.contains("://") && parsed.get("protocol").is_some() {
        parsed.insert("slashes".into(), "true".into());
        if parsed.get("host").is_none() {
            parsed.insert("host".into(), String::new());
            parsed.insert("hostname".into(), String::new());
        }
    }
    if url.starts_with("//") && parsed.get("host").is_some() {
        parsed.insert("slashes".into(), "true".into());
    }
    if !parsed.contains_key("href") {
        let protocol = parsed.get("protocol").cloned().unwrap_or_default();
        let auth = parsed.get("auth").cloned().unwrap_or_default();
        let host = parsed.get("host").cloned().unwrap_or_default();
        let pathname = parsed.get("pathname").cloned().unwrap_or_default();
        let search = parsed.get("search").cloned().unwrap_or_default();
        let hash = parsed.get("hash").cloned().unwrap_or_default();
        parsed.insert(
            "href".into(),
            assemble_url(
                &protocol,
                &auth,
                &host,
                &pathname,
                &search,
                &hash,
                url.contains("://") || url.starts_with("//"),
            ),
        );
    }
    if !parsed.contains_key("path")
        && (parsed.contains_key("pathname") || parsed.contains_key("search"))
    {
        let pathname = parsed.get("pathname").cloned().unwrap_or_default();
        let search = parsed.get("search").cloned().unwrap_or_default();
        parsed.insert("path".into(), format!("{pathname}{search}"));
    }
    let query_object = args
        .get(1)
        .is_some_and(execute::is_truthy)
        .then(|| {
            let query = parsed.get("query").cloned().unwrap_or_default();
            if query.is_empty() {
                let object = host_api::object(vec![("__query_empty".into(), Value::Undefined)]);
                execute::define_property(
                    object.clone(),
                    "__query_empty",
                    host_api::object(vec![("enumerable".into(), Value::Boolean(false))]),
                )?;
                let object = execute::delete_property(object, "__query_empty").0;
                Ok(execute::set_prototype_of(&object, &Value::Null)?)
            } else {
                crate::modules::querystring_parse::parse(state, None, &[Value::String(query)])
            }
        })
        .transpose()?;
    let mut out = Vec::new();
    for (k, v) in parsed {
        if k == "query" && query_object.is_some() {
            continue;
        }
        let value = if k == "slashes" && v == "true" {
            Value::Boolean(true)
        } else {
            Value::String(v)
        };
        out.push((k, value));
    }
    if let Some(query) = query_object.as_ref() {
        if !out.iter().any(|(key, _)| key == "search") {
            out.push(("search".into(), Value::Null));
        }
        out.push(("query".into(), query.clone()));
    }
    let result = legacy_object(out);
    if query_object.is_some()
        && matches!(
            execute::get_property_result(&result, "search"),
            Err(_) | Ok(Value::Undefined)
        )
    {
        return Ok(execute::set_property(result, "search", Value::Null));
    }
    Ok(result)
}

fn invalid_legacy_authority(input: &str) -> Option<VmError> {
    let (_, rest) = input.split_once("://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    if authority.contains('\0') {
        return Some(legacy_url_error("ERR_INVALID_URL", input));
    }
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    if let Some((auth, _)) = authority.rsplit_once('@') {
        if has_malformed_percent_encoding(auth) {
            return Some(uri_malformed_error());
        }
    }
    if host.starts_with('[') {
        let Some(end) = host.find(']') else {
            return Some(legacy_url_error("ERR_INVALID_URL", input));
        };
        let suffix = &host[end + 1..];
        if suffix.is_empty() {
            return None;
        }
        if let Some(port) = suffix.strip_prefix(':') {
            if port.is_empty() || port.chars().all(|character| character.is_ascii_digit()) {
                return None;
            }
            return Some(legacy_url_error("ERR_INVALID_ARG_VALUE", input));
        }
        return Some(legacy_url_error("ERR_INVALID_URL", input));
    }
    let Some((hostname, port)) = host.rsplit_once(':') else {
        return None;
    };
    if hostname.is_empty() || port.is_empty() {
        return None;
    }
    if !port.chars().all(|character| character.is_ascii_digit()) {
        return Some(legacy_url_error("ERR_INVALID_ARG_VALUE", input));
    }
    if port.parse::<u32>().ok().is_some_and(|value| value > 65_535) {
        return Some(legacy_url_error("ERR_INVALID_ARG_VALUE", input));
    }
    None
}

fn uri_malformed_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("URIError".into())),
        ("message".into(), Value::String("URI malformed".into())),
        (
            "constructor".into(),
            Value::Builtin(quench_runtime::ops::Builtin::URIError),
        ),
    ]))
}

fn legacy_url_error(code: &str, input: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        ("message".into(), Value::String("Invalid URL".into())),
        ("code".into(), Value::String(code.into())),
        ("input".into(), Value::String(input.into())),
    ]))
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

fn legacy_object(entries: Vec<(String, Value)>) -> Value {
    let object = legacy_plain_object(entries);
    execute::set_prototype_of(&object, &legacy_url_prototype()).unwrap_or(object)
}

fn legacy_plain_object(entries: Vec<(String, Value)>) -> Value {
    let mut object = host_api::object(
        [
            "protocol", "slashes", "auth", "host", "port", "hostname", "hash", "search", "query",
            "pathname", "path", "href",
        ]
        .into_iter()
        .map(|key| (key.to_string(), Value::Null))
        .collect(),
    );
    for (key, value) in entries {
        object = execute::set_property(object, &key, value);
    }
    object
}

pub fn resolve_object(
    state: &Rc<RefCell<crate::host::HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let method_base = receiver
        .and_then(|value| execute::get_property_result(value, "href").ok())
        .and_then(|value| match value {
            Value::String(value) if !value.is_empty() => Some(value),
            _ => None,
        });
    let argument_base = args.first().and_then(|value| {
        execute::get_property_result(value, "href")
            .ok()
            .and_then(|value| match value {
                Value::String(value) if !value.is_empty() => Some(value),
                _ => None,
            })
    });
    let (base, relative) = match (method_base, argument_base) {
        (Some(base), _) => (base, args.first().map(value_to_string).unwrap_or_default()),
        (None, Some(base)) => (base, args.get(1).map(value_to_string).unwrap_or_default()),
        (None, None) => (
            args.first().map(value_to_string).unwrap_or_default(),
            args.get(1).map(value_to_string).unwrap_or_default(),
        ),
    };
    if base.is_empty() {
        return Ok(Value::String(relative));
    }
    let resolved = legacy_resolve(&base, &relative);
    parse(state, &[Value::String(resolved)])
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
        || (!host.is_empty() && protocol_uses_authority_slashes(&protocol));
    // A plain object with `file:` defaults to an authority URL, while a
    // parsed legacy Url carries its original opaque `href` form.
    let has_original_href = !matches!(
        execute::get_property(&obj, "href"),
        Value::Undefined | Value::Null
    );
    let slashes = slashes
        || (protocol.trim_end_matches(':') == "file"
            && pathname.starts_with('/')
            && !has_original_href);
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
        if matches!(raw, Value::Undefined | Value::Null) {
            continue;
        }
        match *key {
            "protocol" => protocol = value,
            "auth" => auth = value,
            "host" => host = value,
            "hostname" => hostname = value,
            "port" => port = value,
            "pathname" => {
                pathname = if protocol == "javascript:" {
                    value
                } else {
                    encode_path_component(&value)
                }
            }
            "search" => query = value,
            "query"
                if query.is_empty()
                    && !value.is_empty()
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

fn encode_path_component(path: &str) -> String {
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

fn legacy_resolve(from: &str, to: &str) -> String {
    if from.is_empty() {
        return to.to_string();
    }
    if to.is_empty() {
        return from
            .split_once('#')
            .map_or_else(|| from.to_string(), |(value, _)| value.to_string());
    }
    if to.contains("://") {
        if let Some(value) = resolve_absolute_authority_edge(from, to) {
            return value;
        }
        return if to
            .split_once("://")
            .is_some_and(|(_, value)| !value.contains(['/', '?', '#']))
        {
            format!("{to}/")
        } else {
            to.to_string()
        };
    }
    if to.starts_with("//") {
        if let Some((protocol, _)) = from.split_once("://") {
            if to.starts_with("///") {
                return format!("{protocol}:///{}", to.trim_start_matches('/'));
            }
            let authority = to.trim_start_matches('/');
            return if protocol_is_known_authority(&format!("{protocol}://")) {
                if authority.contains('/') {
                    format!("{protocol}://{authority}")
                } else {
                    format!("{protocol}://{authority}/")
                }
            } else {
                format!("{protocol}://{authority}")
            };
        }
    }
    if to.to_ascii_lowercase().starts_with("javascript:") {
        return to.to_string();
    }
    if to.find(':').is_some_and(|index| {
        !to[..index].contains('/') && !to[index + 1..].starts_with(['/', '?', '#'])
    }) {
        let index = to.find(':').unwrap_or_default();
        let scheme = &to[..index];
        let rest = &to[index + 1..];
        let same_scheme = from
            .split_once("://")
            .is_some_and(|(value, _)| value.eq_ignore_ascii_case(scheme));
        return if same_scheme {
            legacy_resolve(from, rest)
        } else if matches!(rest, "." | "./") {
            format!("{scheme}:")
        } else {
            to.to_string()
        };
    }
    if let Some((scheme, rest)) = to.split_once(':') {
        let same_scheme = from
            .split_once("://")
            .is_some_and(|(value, _)| value.eq_ignore_ascii_case(scheme));
        if same_scheme && rest.starts_with('#') {
            return format!(
                "{}{}",
                from.split_once('#').map_or(from, |(value, _)| value),
                rest
            );
        }
        if same_scheme && rest.starts_with('?') {
            let base = from
                .split_once('?')
                .map_or(from, |(value, _)| value)
                .split_once('#')
                .map_or(from, |(value, _)| value);
            return format!("{base}{rest}");
        }
        if !scheme.is_empty() && (rest.starts_with('#') || rest.starts_with('?')) {
            return format!("{scheme}:///{rest}");
        }
        if !scheme.is_empty() && rest.starts_with('/') {
            let base_scheme = from.split_once("://").map(|(value, _)| value);
            if !base_scheme.is_some_and(|value| value.eq_ignore_ascii_case(scheme)) {
                return format!("{scheme}://{}", rest.trim_start_matches('/'));
            }
            if let Some((_, authority_path)) = from.split_once("://") {
                let authority = authority_path.split('/').next().unwrap_or_default();
                return format!("{scheme}://{authority}{rest}");
            }
        }
    }
    if to.starts_with('?') || to.starts_with('#') {
        let base = if to.starts_with('#') {
            from.split_once('#').map_or(from, |(value, _)| value)
        } else if let Some(i) = from.find('?') {
            &from[..i]
        } else if let Some(i) = from.find('#') {
            &from[..i]
        } else {
            from
        };
        return format!("{base}{to}");
    }
    if let Some(index) = to.find(['?', '#']) {
        let path = &to[..index];
        if !path.is_empty() {
            let resolved = legacy_resolve(from, path);
            return format!("{resolved}{}", &to[index..]);
        }
    }
    let (protocol, host_part) = split_protocol(from);
    if !protocol.is_empty() && !protocol.ends_with("//") {
        if to.starts_with(".//") {
            return format!("{protocol}//{}", &to[3..]);
        }
        let mut segments = host_part
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !host_part.ends_with('/') {
            segments.pop();
        }
        if to.starts_with('/') {
            segments.clear();
        }
        for segment in to.split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    segments.pop();
                }
                value => segments.push(value.into()),
            }
        }
        let path = segments.join("/");
        let path = if to.ends_with('/') && !path.ends_with('/') {
            format!("{path}/")
        } else {
            path
        };
        return if to.starts_with('/') || host_part.starts_with('/') {
            format!("{protocol}/{path}")
        } else {
            format!("{protocol}{path}")
        };
    }
    let (host, base_path) = if protocol.is_empty() && !from.starts_with("//") {
        (String::new(), from.to_string())
    } else {
        match host_part.find('/') {
            Some(i) => (host_part[..i].to_string(), host_part[i..].to_string()),
            None => (
                host_part
                    .split_once(['?', '#'])
                    .map_or(host_part, |(value, _)| value)
                    .to_string(),
                "/".to_string(),
            ),
        }
    };
    let base_path = base_path
        .split_once(['?', '#'])
        .map_or(base_path.clone(), |(value, _)| value.to_string());
    let base_dir = if base_path.ends_with('/') {
        base_path
    } else {
        let last = base_path.rfind('/').unwrap_or(0);
        base_path[..last + 1].to_string()
    };
    if protocol.is_empty() && !from.starts_with('/') {
        let mut segments = base_dir
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        for segment in to.split('/') {
            match segment {
                "" | "." => {}
                ".." if segments.last().is_some_and(|value| value != "..") => {
                    segments.pop();
                }
                ".." => segments.push("..".into()),
                value => segments.push(value.into()),
            }
        }
        return segments.join("/");
    }
    let path = if to.starts_with('/') {
        to.to_string()
    } else {
        format!("{base_dir}{to}")
    };
    let normalized_path = if protocol_is_known_authority(&protocol) && !host.is_empty() {
        normalize_path(&path)
    } else {
        normalize_path_preserving_empty(&path)
    };
    let normalized = format!("{}{}", host, normalized_path);
    let preserve_trailing = to.ends_with('/')
        || matches!(to.split('/').next_back(), Some("." | ".."))
        || matches!(to, "." | ".." | "/." | "/./" | "/.." | "/../");
    let normalized = if preserve_trailing && !normalized.ends_with('/') {
        format!("{normalized}/")
    } else {
        normalized
    };
    if protocol.is_empty() {
        return normalized;
    }
    format!("{protocol}{normalized}")
}

fn resolve_absolute_authority_edge(from: &str, to: &str) -> Option<String> {
    let (target_scheme, target_rest) = to.split_once("://")?;
    let target_end = target_rest
        .find(['/', '?', '#'])
        .unwrap_or(target_rest.len());
    let target_authority = &target_rest[..target_end];
    let target_host = target_authority
        .rsplit_once('@')
        .map_or(target_authority, |(_, value)| value)
        .split(':')
        .next()
        .unwrap_or_default();
    let (base_scheme, base_rest) = from.split_once("://")?;
    if !base_scheme.eq_ignore_ascii_case(target_scheme) {
        return None;
    }
    let base_end = base_rest.find(['/', '?', '#']).unwrap_or(base_rest.len());
    let base_authority = &base_rest[..base_end];
    let base_host = base_authority
        .rsplit_once('@')
        .map_or(base_authority, |(_, value)| value)
        .split(':')
        .next()
        .unwrap_or_default();
    let base_path = base_rest[base_end..]
        .split_once(['?', '#'])
        .map_or(&base_rest[base_end..], |(value, _)| value);
    if target_end == target_rest.len() {
        if target_host.eq_ignore_ascii_case(base_host) && !base_path.is_empty() && base_path != "/"
        {
            return Some(format!("{target_scheme}://{target_authority}{base_path}"));
        }
        return None;
    }
    if base_authority.contains('@')
        && !target_authority.contains('@')
        && target_host.eq_ignore_ascii_case(base_host)
    {
        let target_path = &target_rest[target_end..];
        return Some(format!("{base_scheme}://{base_authority}{target_path}"));
    }
    None
}

fn split_protocol(url: &str) -> (String, &str) {
    match url.find("://") {
        Some(i) => (url[..i + 3].to_string(), &url[i + 3..]),
        None => match url.find(':') {
            Some(i) if !url[..i].contains('/') => (url[..i + 1].to_string(), &url[i + 1..]),
            _ => (String::new(), url),
        },
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

fn normalize_path_preserving_empty(path: &str) -> String {
    let absolute = path.starts_with('/');
    let body = path.trim_start_matches('/');
    let mut parts = Vec::new();
    for part in body.split('/') {
        match part {
            "." => {}
            ".." => {
                if parts.last().is_some_and(String::is_empty) {
                    parts.pop();
                } else if let Some(index) =
                    parts.iter().rposition(|value: &String| !value.is_empty())
                {
                    parts.remove(index);
                }
            }
            value => parts.push(value.to_string()),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else {
        joined
    }
}

fn protocol_is_known_authority(protocol: &str) -> bool {
    matches!(
        protocol
            .trim_end_matches("://")
            .to_ascii_lowercase()
            .as_str(),
        "http" | "https" | "ftp" | "file" | "ws" | "wss"
    )
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
    let (native_url_class, _) = crate::modules::url_whatwg::url_class(state);
    let url_class = {
        let global = quench_runtime::vm::current_global_object();
        let value = execute::get_property(&global, "URL");
        if quench_runtime::is_callable(&value) {
            value
        } else {
            native_url_class
        }
    };
    let legacy_url = crate::host::capability(crate::registry::SPEC_URL_LEGACY_NEW);
    let _ = execute::set_callable_property(&legacy_url, "prototype", legacy_url_prototype());
    let url_pattern = {
        let global = quench_runtime::vm::current_global_object();
        let value = execute::get_property(&global, "__quenchURLPattern");
        if quench_runtime::is_callable(&value) {
            value
        } else {
            let constructor = crate::host::capability(crate::registry::SPEC_URL_PATTERN);
            let _ =
                execute::set_callable_property(&constructor, "prototype", url_pattern_prototype());
            constructor
        }
    };
    crate::host::namespace_object(vec![
        spec_fn("parse", "require:url:parse", 0x0500),
        spec_fn("format", "require:url:format", 0x0501),
        spec_fn("resolve", "require:url:resolve", 0x0502),
        spec_fn("resolveObject", "require:url:resolveObject", 0x0520),
        ("Url", legacy_url),
        ("URL", url_class),
        ("URLPattern", url_pattern),
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

/// Library-host URLPattern constructor. Keeps the fallback observable shape
/// (own components plus callable prototype descriptors) in one capability.
pub fn url_pattern_construct(
    _state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Undefined);
    if !matches!(
        first,
        Value::Undefined
            | Value::Null
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::Object(_)
            | Value::ObjectAlias(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The first argument must be a string or an object".into(),
        ));
    }
    if let Some(options) = args.get(1) {
        let valid = matches!(
            options,
            Value::Undefined
                | Value::Null
                | Value::String(_)
                | Value::StringUnits(_)
                | Value::Object(_)
                | Value::ObjectAlias(_)
        );
        if !valid {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The second argument must be a string or an object".into(),
            ));
        }
        if matches!(options, Value::Object(_) | Value::ObjectAlias(_)) {
            let _ = execute::get_property_result(options, "ignoreCase")?;
        }
    }
    if let Some(options) = args.get(2) {
        if !matches!(
            options,
            Value::Undefined | Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The third argument must be an object".into(),
            ));
        }
    }
    if args.len() >= 3
        && matches!(first, Value::String(_) | Value::StringUnits(_))
        && matches!(args.get(1), Some(Value::Null | Value::Undefined))
    {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "code".into(),
                Value::String("ERR_INVALID_URL_PATTERN".into()),
            ),
            (
                "message".into(),
                Value::String("Invalid URLPattern base URL".into()),
            ),
        ])));
    }
    let mut protocol = "*".to_string();
    let mut hostname = "*".to_string();
    let mut pathname = "*".to_string();
    match first {
        Value::String(value) => {
            if let Some((scheme, rest)) = value.split_once("://") {
                protocol = scheme.to_string();
                hostname = rest
                    .split(['/', '?', '#'])
                    .next()
                    .unwrap_or(rest)
                    .to_string();
            }
        }
        Value::StringUnits(units) => {
            let value = String::from_utf16_lossy(units);
            if let Some((scheme, rest)) = value.split_once("://") {
                protocol = scheme.to_string();
                hostname = rest
                    .split(['/', '?', '#'])
                    .next()
                    .unwrap_or(rest)
                    .to_string();
            }
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            for (name, slot) in [
                ("protocol", &mut protocol),
                ("hostname", &mut hostname),
                ("pathname", &mut pathname),
            ] {
                let value = execute::get_property_result(first, name)?;
                if let Value::String(value) = value {
                    *slot = value;
                }
            }
        }
        _ => {}
    }
    let prototype = url_pattern_prototype();
    let pattern = host_api::object(vec![
        ("\0quench:urlpattern:instance".into(), Value::Boolean(true)),
        ("protocol".into(), Value::String(protocol)),
        ("username".into(), Value::String("*".into())),
        ("password".into(), Value::String("*".into())),
        ("hostname".into(), Value::String(hostname)),
        ("port".into(), Value::String("*".into())),
        ("pathname".into(), Value::String(pathname)),
        ("search".into(), Value::String("*".into())),
        ("hash".into(), Value::String("*".into())),
        (
            "test".into(),
            crate::host::capability(crate::registry::SPEC_URL_PATTERN_TEST),
        ),
        (
            "exec".into(),
            crate::host::capability(crate::registry::SPEC_URL_PATTERN_EXEC),
        ),
    ]);
    execute::set_prototype_of(&pattern, &prototype)
}

pub fn url_pattern_call(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "message".into(),
            Value::String("Class constructor URLPattern cannot be invoked without 'new'".into()),
        ),
        (
            "code".into(),
            Value::String("ERR_CONSTRUCT_CALL_REQUIRED".into()),
        ),
    ])))
}

fn url_pattern_prototype() -> Value {
    let mut prototype = host_api::object(Vec::new());
    for name in [
        "protocol",
        "username",
        "password",
        "hostname",
        "port",
        "pathname",
        "search",
        "hash",
        "hasRegExpGroups",
    ] {
        prototype = match execute::define_property(
            prototype.clone(),
            name,
            host_api::object(vec![(
                "get".into(),
                crate::host::capability(crate::registry::SPEC_URL_PATTERN_GET),
            )]),
        ) {
            Ok(next) => next,
            Err(_) => break,
        };
    }
    prototype
}

pub fn url_pattern_get(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if !matches!(
        receiver.and_then(|v| execute::get_property_result(v, "\0quench:urlpattern:instance").ok()),
        Some(Value::Boolean(true))
    ) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("message".into(), Value::String("Illegal invocation".into())),
        ])));
    }
    Ok(Value::Undefined)
}

pub fn url_pattern_exec(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !matches!(
        receiver.and_then(|value| execute::get_property_result(
            value,
            "\0quench:urlpattern:instance"
        )
        .ok()),
        Some(Value::Boolean(true))
    ) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("message".into(), Value::String("Illegal invocation".into())),
        ])));
    }
    let input_value = args.first().unwrap_or(&Value::Undefined);
    if !matches!(
        input_value,
        Value::Undefined
            | Value::Null
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::Object(_)
            | Value::ObjectAlias(_)
    ) {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "The input argument must be a string or an object".into(),
        ));
    }
    if let Some(base) = args.get(1) {
        if !matches!(
            base,
            Value::Undefined | Value::Null | Value::String(_) | Value::StringUnits(_)
        ) {
            return Err(crate::modules::buffer_enc::invalid_arg_type(
                "The baseURL argument must be a string".into(),
            ));
        }
        if matches!(
            input_value,
            Value::Null | Value::Object(_) | Value::ObjectAlias(_)
        ) && matches!(base, Value::Null)
        {
            return Err(VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                ("code".into(), Value::String("ERR_OPERATION_FAILED".into())),
                (
                    "message".into(),
                    Value::String("Invalid URLPattern input".into()),
                ),
            ])));
        }
        if matches!(input_value, Value::String(_) | Value::StringUnits(_))
            && matches!(base, Value::Null)
        {
            return Ok(Value::Null);
        }
    }
    let input = args
        .first()
        .map(quench_runtime::to_string)
        .transpose()?
        .unwrap_or_default();
    let url = input
        .strip_prefix("https://")
        .or_else(|| input.strip_prefix("http://"))
        .unwrap_or(&input);
    let (host, path) = url.split_once('/').unwrap_or((url, ""));
    let path = format!("/{path}");
    let _pattern = receiver.ok_or_else(|| VmError::Thrown(Value::Undefined))?;
    let groups = host_api::object(vec![(
        "value".into(),
        Value::String(path.trim_start_matches('/').into()),
    )]);
    let pathname_result = host_api::object(vec![
        ("groups".into(), groups),
        ("input".into(), Value::String(path)),
    ]);
    Ok(host_api::object(vec![
        (
            "hash".into(),
            host_api::object(vec![
                ("input".into(), Value::String("".into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
        (
            "hostname".into(),
            host_api::object(vec![
                ("input".into(), Value::String(host.into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
        ("inputs".into(), host_api::array(args.to_vec())),
        (
            "password".into(),
            host_api::object(vec![
                ("input".into(), Value::String("".into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
        ("pathname".into(), pathname_result),
        (
            "port".into(),
            host_api::object(vec![
                ("input".into(), Value::String("".into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
        (
            "protocol".into(),
            host_api::object(vec![
                ("input".into(), Value::String("https".into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
        (
            "search".into(),
            host_api::object(vec![
                ("input".into(), Value::String("".into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
        (
            "username".into(),
            host_api::object(vec![
                ("input".into(), Value::String("".into())),
                ("groups".into(), host_api::object(Vec::new())),
            ]),
        ),
    ]))
}

pub fn url_pattern_test(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(!matches!(
        url_pattern_exec(state, receiver, args)?,
        Value::Null
    )))
}

fn spec_fn(name: &'static str, spec: &'static str, id: u16) -> (&'static str, Value) {
    (
        name,
        crate::host::capability(crate::registry::NodeSpec::new(spec, id)),
    )
}

// `url.pathToFileURL` lives in `url_file`; see `SPEC_URL_PATH_TO_FILE_URL`.
