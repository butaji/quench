//! WHATWG `URL` class backed by the `url` crate. Instances are ordinary
//! host objects carrying the serialized href in a `\0url` internal slot and
//! a shared `\0prototype` whose accessor descriptors invoke these getters.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry as specs;

/// A parsed URL: either a full `url` crate parse or, for inputs Node's
/// lenient parser accepts but the `url` crate rejects (e.g. invalid IDN
/// hosts on special schemes), the verbatim href treated as opaque.
pub enum Parsed {
    Url(url::Url),
    Opaque(String),
}

impl Parsed {
    pub fn parse(input: &str, base: Option<&str>) -> Result<Parsed, VmError> {
        let parsed = match base {
            Some(base) => url::Url::parse(base).and_then(|b| b.join(input)),
            None => url::Url::parse(input),
        };
        match parsed {
            Ok(url) => Ok(Parsed::Url(url)),
            Err(_) if input.contains(':') => Ok(Parsed::Opaque(input.to_string())),
            Err(_) => Err(invalid_url(input)),
        }
    }

    fn href(&self) -> String {
        match self {
            Parsed::Url(url) => url.as_str().to_string(),
            Parsed::Opaque(href) => href.clone(),
        }
    }

    pub fn get(&self, component: &str) -> String {
        if component == "href" {
            return self.href();
        }
        let Parsed::Url(url) = self else {
            return self.opaque_component(component);
        };
        match component {
            "protocol" => format!("{}:", url.scheme()),
            "username" => url.username().to_string(),
            "password" => url.password().unwrap_or_default().to_string(),
            "host" => host_string(url, true),
            "hostname" => url.host_str().unwrap_or_default().to_string(),
            "port" => url.port().map(|p| p.to_string()).unwrap_or_default(),
            "pathname" => url.path().to_string(),
            "search" => url.query().map(|q| format!("?{q}")).unwrap_or_default(),
            "hash" => url.fragment().map(|f| format!("#{f}")).unwrap_or_default(),
            "origin" => origin_string(url),
            _ => String::new(),
        }
    }

    fn opaque_component(&self, component: &str) -> String {
        let Parsed::Opaque(href) = self else {
            return String::new();
        };
        match component {
            "protocol" => href
                .split_once(':')
                .map(|(scheme, _)| format!("{scheme}:"))
                .unwrap_or_default(),
            "pathname" => href
                .split_once(':')
                .map(|(_, rest)| rest.to_string())
                .unwrap_or_default(),
            "origin" => "null".to_string(),
            _ => String::new(),
        }
    }
}

fn host_string(url: &url::Url, with_port: bool) -> String {
    let mut host = url.host_str().unwrap_or_default().to_string();
    if with_port {
        if let Some(port) = url.port() {
            host.push(':');
            host.push_str(&port.to_string());
        }
    }
    host
}

fn origin_string(url: &url::Url) -> String {
    match url.origin() {
        url::Origin::Tuple(scheme, host, port) => {
            let default = match scheme.as_str() {
                "http" | "ws" => 80,
                "https" | "wss" => 443,
                "ftp" => 21,
                _ => 0,
            };
            if port == default {
                format!("{scheme}://{host}")
            } else {
                format!("{scheme}://{host}:{port}")
            }
        }
        url::Origin::Opaque(_) => "null".to_string(),
    }
}

/// Coded `TypeError` (`ERR_INVALID_URL`) with the failing input attached.
pub fn invalid_url(input: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String("Invalid URL".to_string()),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_URL".to_string()),
        ),
        ("input".to_string(), Value::String(input.to_string())),
    ]))
}

fn brand_error() -> VmError {
    execute::type_error("Expected a URL instance")
}

/// Read and re-parse the receiver's `\0url` slot.
pub fn parsed_of(receiver: Option<&Value>) -> Result<Parsed, VmError> {
    let Some(receiver) = receiver else {
        return Err(brand_error());
    };
    match execute::get_property(receiver, "\0url") {
        Value::String(href) => Parsed::parse(&href, None).map_err(|_| brand_error()),
        _ => Err(brand_error()),
    }
}

/// Brand check used by `fileURLToPath`/`url.format` (`isURLInstance`).
pub fn is_url_instance(value: &Value) -> bool {
    !matches!(execute::get_property(value, "\0url"), Value::Undefined)
}

/// Rust host entry point for `internal/url`'s brand predicate.
pub fn is_url(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    Ok(Value::Boolean(args.first().is_some_and(is_url_instance)))
}

/// The shared (constructor, prototype) pair, built once per realm.
pub fn url_class(state: &Rc<RefCell<HostState>>) -> (Value, Value) {
    if let Some(pair) = &state.borrow().url_class {
        return pair.clone();
    }
    let pair = build_class();
    state.borrow_mut().url_class = Some(pair.clone());
    pair
}

fn build_class() -> (Value, Value) {
    let prototype = build_prototype();
    let constructor = crate::host::capability(specs::SPEC_URL_NEW);
    let _ = execute::set_callable_property(&constructor, "prototype", prototype.clone());
    let _ = execute::set_callable_property(
        &constructor,
        "createObjectURL",
        crate::host::capability(specs::SPEC_URL_CREATE_OBJECT_URL),
    );
    let _ = execute::set_callable_property(
        &constructor,
        "revokeObjectURL",
        crate::host::capability(specs::SPEC_URL_REVOKE_OBJECT_URL),
    );
    (constructor, prototype)
}

fn build_prototype() -> Value {
    let getters: [(&str, specs::NodeSpec); 11] = [
        ("href", specs::SPEC_URL_GET_HREF),
        ("protocol", specs::SPEC_URL_GET_PROTOCOL),
        ("username", specs::SPEC_URL_GET_USERNAME),
        ("password", specs::SPEC_URL_GET_PASSWORD),
        ("host", specs::SPEC_URL_GET_HOST),
        ("hostname", specs::SPEC_URL_GET_HOSTNAME),
        ("port", specs::SPEC_URL_GET_PORT),
        ("pathname", specs::SPEC_URL_GET_PATHNAME),
        ("search", specs::SPEC_URL_GET_SEARCH),
        ("hash", specs::SPEC_URL_GET_HASH),
        ("origin", specs::SPEC_URL_GET_ORIGIN),
    ];
    let mut entries: Vec<(String, Value)> = Vec::new();
    for (key, spec) in getters {
        entries.push((key.to_string(), Value::Undefined));
        entries.push((descriptor_key(key), accessor_descriptor(spec)));
    }
    for (key, spec) in [
        ("searchParams", specs::SPEC_URL_GET_SEARCH_PARAMS),
        ("toString", specs::SPEC_URL_TO_STRING),
        ("toJSON", specs::SPEC_URL_TO_JSON),
    ] {
        let value = crate::host::capability(spec);
        entries.push((key.to_string(), value.clone()));
        entries.push((descriptor_key(key), method_descriptor(value)));
    }
    host_api::object(entries)
}

fn descriptor_key(key: &str) -> String {
    format!("\0quench:descriptor:\0{key}")
}

fn accessor_descriptor(spec: specs::NodeSpec) -> Value {
    host_api::object(vec![
        ("get".to_string(), crate::host::capability(spec)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])
}

fn method_descriptor(value: Value) -> Value {
    host_api::object(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(true)),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])
}

/// Construct a URL instance from a parsed URL.
pub fn make_instance(state: &Rc<RefCell<HostState>>, parsed: &Parsed) -> Value {
    let (_, native_prototype) = url_class(state);
    let prototype = {
        let global = quench_runtime::vm::current_global_object();
        let constructor = execute::get_property(&global, "URL");
        let candidate = execute::get_property(&constructor, "prototype");
        if matches!(candidate, Value::Object(_)) {
            candidate
        } else {
            native_prototype
        }
    };
    let object = host_api::object(vec![("\0url".to_string(), Value::String(parsed.href()))]);
    execute::set_prototype_of(&object, &prototype).unwrap_or(object)
}

/// `new URL(input[, base])`.
pub fn new_url(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let Some(first) = args.first() else {
        return Err(missing_args());
    };
    let input = execute::to_js_string(first)?;
    let base = match args.get(1) {
        Some(Value::Undefined) | None => None,
        Some(value) => Some(execute::to_js_string(value)?),
    };
    let parsed = Parsed::parse(&input, base.as_deref())?;
    Ok(make_instance(state, &parsed))
}

fn missing_args() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String("The \"url\" argument must be specified".to_string()),
        ),
        (
            "code".to_string(),
            Value::String("ERR_MISSING_ARGS".to_string()),
        ),
    ]))
}

fn blob_instance(value: &Value) -> bool {
    let global = quench_runtime::vm::current_global_object();
    let constructor = execute::get_property(&global, "Blob");
    let expected = execute::get_property(&constructor, "prototype");
    let mut current = execute::get_prototype_of(value).ok();
    while let Some(candidate) = current {
        if candidate == expected {
            return true;
        }
        current = execute::get_prototype_of(&candidate).ok();
    }
    false
}

/// `URL.createObjectURL(blob)` stores one strong host root and returns its id.
pub fn create_object_url(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(blob) = args.first() else {
        return Err(execute::type_error(
            "The \"obj\" argument must be an instance of Blob",
        ));
    };
    if !blob_instance(blob) {
        return Err(VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into())),
            (
                "message".into(),
                Value::String("The \"obj\" argument must be an instance of Blob".into()),
            ),
        ])));
    }
    let mut guard = state.borrow_mut();
    let id = format!("blob:nodedata:quench-{}", guard.next_blob_url);
    guard.next_blob_url += 1;
    guard.blob_urls.insert(id.clone(), blob.clone());
    Ok(Value::String(id))
}

/// `URL.revokeObjectURL(url)` removes the host root when one is registered.
pub fn revoke_object_url(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.is_empty() {
        return Err(missing_args());
    }
    if let Some(Value::String(id)) = args.first() {
        state.borrow_mut().blob_urls.remove(id);
    }
    Ok(Value::Undefined)
}

fn get_component(receiver: Option<&Value>, component: &str) -> Result<Value, VmError> {
    Ok(Value::String(parsed_of(receiver)?.get(component)))
}

macro_rules! component_getter {
    ($name:ident, $component:literal) => {
        pub fn $name(
            _state: &Rc<RefCell<HostState>>,
            receiver: Option<&Value>,
            _args: &[Value],
        ) -> Result<Value, VmError> {
            get_component(receiver, $component)
        }
    };
}

component_getter!(get_href, "href");
component_getter!(get_protocol, "protocol");
component_getter!(get_username, "username");
component_getter!(get_password, "password");
component_getter!(get_host, "host");
component_getter!(get_hostname, "hostname");
component_getter!(get_port, "port");
component_getter!(get_pathname, "pathname");
component_getter!(get_search, "search");
component_getter!(get_hash, "hash");
component_getter!(get_origin, "origin");

pub fn get_search_params(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let search = parsed_of(receiver)?.get("search");
    crate::modules::url::new_search_params(state, &[Value::String(search)])
}

/// `url.format(urlObject)`'s WHATWG branch (`bindingUrl.format`).
pub fn format_href(href: &str, fragment: bool, unicode: bool, search: bool, auth: bool) -> String {
    let Ok(Parsed::Url(url)) = Parsed::parse(href, None) else {
        return href.to_string();
    };
    let mut out = format!("{}:", url.scheme());
    if url.host_str().is_some() {
        out.push_str("//");
        push_authority(&mut out, &url, auth, unicode);
    }
    out.push_str(url.path());
    if search {
        if let Some(query) = url.query() {
            out.push('?');
            out.push_str(query);
        }
    }
    if fragment {
        if let Some(fragment) = url.fragment() {
            out.push('#');
            out.push_str(fragment);
        }
    }
    out
}

fn push_authority(out: &mut String, url: &url::Url, auth: bool, unicode: bool) {
    if auth && (!url.username().is_empty() || !url.password().unwrap_or_default().is_empty()) {
        out.push_str(url.username());
        let password = url.password().unwrap_or_default();
        if !password.is_empty() {
            out.push(':');
            out.push_str(password);
        }
        out.push('@');
    }
    let host = url.host_str().unwrap_or_default();
    let needs_idna = host.bytes().any(|b| b > 0x7F) || host.to_lowercase().contains("xn--");
    if unicode && !host.starts_with('[') && needs_idna {
        out.push_str(&idna::domain_to_unicode(host).0);
    } else {
        out.push_str(host);
    }
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
}

/// `url.urlToHttpOptions(url)` — port of Node's converter; reads through
/// the accessor properties so plain copies degrade exactly like Node's.
pub fn url_to_http_options(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let url = args.first().cloned().unwrap_or(Value::Undefined);
    if !crate::modules::url::is_object_arg(&url) {
        return Err(invalid_url_object_arg(&url));
    }
    let get = |key: &str| execute::get_property(&url, key);
    let mut out = http_options_base(&url);
    let port = get("port");
    if !matches!(&port, Value::String(s) if s.is_empty()) {
        let text = execute::to_js_string(&port).unwrap_or_default();
        out.push((
            "port".to_string(),
            Value::Number(text.parse().unwrap_or(f64::NAN)),
        ));
    }
    let username = text_or_empty(&get("username"));
    let password = text_or_empty(&get("password"));
    if !username.is_empty() || !password.is_empty() {
        out.push((
            "auth".to_string(),
            Value::String(format!("{username}:{password}")),
        ));
    }
    let options = host_api::object(out);
    execute::set_prototype_of(&options, &Value::Null)
}

fn text_or_empty(value: &Value) -> String {
    match value {
        Value::Undefined | Value::Null => String::new(),
        other => execute::to_js_string(other).unwrap_or_default(),
    }
}

fn invalid_url_object_arg(value: &Value) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String(format!(
                "The \"url\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

fn http_options_base(url: &Value) -> Vec<(String, Value)> {
    let get = |key: &str| execute::get_property(url, key);
    let hostname_value = match &get("hostname") {
        Value::Undefined => Value::Undefined,
        other => {
            let hostname = text_or_empty(other);
            Value::String(
                hostname
                    .strip_prefix('[')
                    .and_then(|h| h.strip_suffix(']'))
                    .unwrap_or(&hostname)
                    .to_string(),
            )
        }
    };
    let pathname = get("pathname");
    let search = get("search");
    vec![
        ("protocol".to_string(), get("protocol")),
        ("hostname".to_string(), hostname_value),
        ("hash".to_string(), get("hash")),
        ("search".to_string(), search.clone()),
        ("pathname".to_string(), pathname.clone()),
        (
            "path".to_string(),
            Value::String(format!(
                "{}{}",
                text_or_empty(&pathname),
                text_or_empty(&search)
            )),
        ),
        ("href".to_string(), get("href")),
    ]
}
/// `url.domainToASCII(domain)` — UTS-46 to-ASCII; empty string on failure.
pub fn domain_to_ascii(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let domain =
        crate::modules::path::validate_string(args.first().unwrap_or(&Value::Undefined), "domain")?;
    if domain.is_empty() {
        return Ok(Value::String(String::new()));
    }
    Ok(Value::String(
        idna::domain_to_ascii(&domain).unwrap_or_default(),
    ))
}

/// `url.domainToUnicode(domain)` — UTS-46 to-Unicode; never fails.
pub fn domain_to_unicode(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let domain =
        crate::modules::path::validate_string(args.first().unwrap_or(&Value::Undefined), "domain")?;
    Ok(Value::String(idna::domain_to_unicode(&domain).0))
}
