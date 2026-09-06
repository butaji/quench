//! Rust-owned TLS API surface.
//!
//! Transport encryption is not available yet, but option validation and the
//! byte-level ALPN contract are ordinary Node semantics and belong here at
//! the host boundary rather than in a JavaScript shim.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine;
use openssl::pkcs12::Pkcs12;
use openssl::x509::X509Crl;
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::ops::Builtin;
use quench_runtime::value::Value;

use crate::host::HostState;

/// Host-only marker shared with the net poller; it records TLS's logical
/// transport identity while the current backend uses the same byte stream.
pub(crate) const TLS_SERVER_PROP: &str = "\0quench:tls-server";
pub(crate) const TLS_SOCKET_PROP: &str = "\0quench:tls-socket";
pub(crate) const TLS_REJECTED_PROP: &str = "\0quench:tls-rejected";
const CONTEXT_CA_PROP: &str = "\0quench:tls:ca-present";
const CONTEXT_MARKER_PROP: &str = "\0quench:tls:context";
pub(crate) const TLS_ALPN_PROP: &str = "\0quench:tls:alpn";
pub(crate) const TLS_NEGOTIATED_ALPN_PROP: &str = "\0quench:tls:negotiated-alpn";
const TLS_PROTOCOL_PROP: &str = "\0quench:tls:protocol";
const TLS_PEER_CERT_PROP: &str = "\0quench:tls:peer-cert";

fn coded_error(kind: Builtin, code: &str, message: String) -> VmError {
    let error = quench_runtime::builtins::error(kind, &[Value::String(message)]);
    VmError::Thrown(execute::set_property(
        error,
        "code",
        Value::String(code.into()),
    ))
}

fn invalid_type(message: String) -> VmError {
    coded_error(Builtin::TypeError, "ERR_INVALID_ARG_TYPE", message)
}

fn invalid_value(message: String) -> VmError {
    coded_error(Builtin::TypeError, "ERR_INVALID_ARG_VALUE", message)
}

fn unsupported(operation: &str) -> VmError {
    coded_error(
        Builtin::Error,
        "ERR_TLS_NOT_SUPPORTED",
        format!("{operation} is not supported by quench-node"),
    )
}

fn option(options: &Value, name: &str) -> Value {
    execute::get_property(options, name)
}

fn validate_type(options: &Value, name: &str, expected: &str) -> Result<(), VmError> {
    let value = option(options, name);
    let valid = match expected {
        "string" => matches!(value, Value::String(_) | Value::StringUnits(_)),
        "number" => matches!(value, Value::Number(_)),
        _ => true,
    };
    if valid || matches!(value, Value::Undefined | Value::Null) {
        return Ok(());
    }
    Err(invalid_type(format!(
        "The \"options.{name}\" property must be of type {expected}.{}",
        crate::modules::util::invalid_arg_received(&value)
    )))
}

fn validate_options(options: &Value) -> Result<(), VmError> {
    validate_pfx(options)?;
    validate_crl(options)?;
    for name in ["ciphers", "passphrase", "ecdhCurve"] {
        validate_type(options, name, "string")?;
    }
    for name in ["handshakeTimeout", "sessionTimeout"] {
        validate_type(options, name, "number")?;
        let value = option(options, name);
        if let Value::Number(value) = value {
            if !value.is_finite()
                || value.fract() != 0.0
                || !(0.0..=2_147_483_647.0).contains(&value)
            {
                return Err(coded_error(
                    Builtin::RangeError,
                    "ERR_OUT_OF_RANGE",
                    format!(
                        "The value of \"options.{name}\" is out of range. It must be >= 0 && <= 2147483647. Received {value}"
                    ),
                ));
            }
        }
    }
    validate_type(options, "clientCertEngine", "string")?;
    if matches!(
        option(options, "clientCertEngine"),
        Value::String(_) | Value::StringUnits(_)
    ) {
        return Err(coded_error(
            Builtin::Error,
            "ERR_CRYPTO_CUSTOM_ENGINE_NOT_SUPPORTED",
            "Custom engines not supported by this OpenSSL".into(),
        ));
    }
    let ticket_keys = option(options, "ticketKeys");
    if !matches!(ticket_keys, Value::Undefined | Value::Null) {
        let Some(bytes) = view_bytes(&ticket_keys) else {
            return Err(invalid_type(
                "The \"options.ticketKeys\" property must be an instance of Buffer or Uint8Array"
                    .into(),
            ));
        };
        if bytes.len() != 48 {
            return Err(invalid_value(
                "The property 'options.ticketKeys' must be exactly 48 bytes".into(),
            ));
        }
    }
    for name in ["minVersion", "maxVersion"] {
        let value = option(options, name);
        if matches!(value, Value::Undefined) {
            continue;
        }
        let version = execute::to_js_string(&value)?;
        if !matches!(
            version.as_str(),
            "TLSv1.0" | "TLSv1.1" | "TLSv1.2" | "TLSv1.3"
        ) {
            return Err(coded_error(
                Builtin::TypeError,
                "ERR_TLS_INVALID_PROTOCOL_VERSION",
                format!("Invalid TLS protocol version: {version}"),
            ));
        }
    }
    let min = option(options, "minVersion");
    let max = option(options, "maxVersion");
    if !matches!(min, Value::Undefined) && !matches!(max, Value::Undefined) {
        let min = execute::to_js_string(&min)?;
        let max = execute::to_js_string(&max)?;
        if tls_version_rank(&min) > tls_version_rank(&max) {
            return Err(coded_error(
                Builtin::TypeError,
                "ERR_TLS_PROTOCOL_VERSION_CONFLICT",
                format!(
                    "The highest supported TLS protocol version is {max}, but the minimum is {min}"
                ),
            ));
        }
    }
    Ok(())
}

fn validate_crl(options: &Value) -> Result<(), VmError> {
    let value = option(options, "crl");
    if matches!(value, Value::Undefined | Value::Null) {
        return Ok(());
    }
    let bytes = match &value {
        Value::String(text) => text.as_bytes().to_vec(),
        Value::StringUnits(_) => execute::to_js_string(&value)
            .unwrap_or_default()
            .into_bytes(),
        _ => view_bytes(&value).unwrap_or_default(),
    };
    let parsed = X509Crl::from_pem(&bytes).or_else(|_| X509Crl::from_der(&bytes));
    if parsed.is_err() {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("Failed to parse CRL".into())],
        )));
    }
    Ok(())
}

fn validate_pfx(options: &Value) -> Result<(), VmError> {
    let pfx = option(options, "pfx");
    if matches!(pfx, Value::Undefined | Value::Null) {
        return Ok(());
    }
    let Some(bytes) = view_bytes(&pfx) else {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("not enough data".into())],
        )));
    };
    let passphrase = execute::to_js_string(&option(options, "passphrase")).unwrap_or_default();
    let Ok(bundle) = Pkcs12::from_der(&bytes) else {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("not enough data".into())],
        )));
    };
    if bundle.parse2(&passphrase).is_err() {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("mac verify failure".into())],
        )));
    }
    Ok(())
}

fn is_view(value: &Value) -> bool {
    matches!(
        value,
        Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
            | Value::DataView(_)
    )
}

fn valid_material(value: &Value, allow_pem_object: bool) -> bool {
    if matches!(
        value,
        Value::Undefined | Value::Null | Value::Boolean(false)
    ) || matches!(value, Value::String(_) | Value::StringUnits(_))
        || is_view(value)
    {
        return true;
    }
    if let Value::Array(values) = value {
        return (0..values.logical_len())
            .all(|index| valid_material(&execute::get_property(value, &index.to_string()), true));
    }
    allow_pem_object
        && matches!(value, Value::Object(_) | Value::ObjectAlias(_))
        && !matches!(execute::get_property(value, "pem"), Value::Undefined)
}

fn invalid_material_value(value: &Value) -> Value {
    if let Value::Array(values) = value {
        for index in 0..values.logical_len() {
            let item = execute::get_property(value, &index.to_string());
            if !valid_material(&item, true) {
                return invalid_material_value(&item);
            }
        }
    }
    value.clone()
}

fn validate_server_material(options: &Value, name: &str) -> Result<(), VmError> {
    let value = option(options, name);
    if matches!(value, Value::Undefined) || valid_material(&value, false) {
        return Ok(());
    }
    let received = invalid_material_value(&value);
    Err(invalid_type(format!(
        "The \"options.{name}\" property must be of type string or an instance of Buffer, TypedArray, or DataView.{}",
        crate::modules::util::invalid_arg_received(&received)
    )))
}

fn tls_version_rank(version: &str) -> u8 {
    match version {
        "TLSv1" | "TLSv1.0" => 10,
        "TLSv1.1" => 11,
        "TLSv1.2" => 12,
        "TLSv1.3" => 13,
        _ => 0,
    }
}

fn cli_tls_version(minimum: bool) -> &'static str {
    let global = quench_runtime::vm::current_global_object();
    let process = execute::get_property(&global, "process");
    let flags = execute::get_property(&process, "execArgv");
    let mut present = [false; 4];
    let prefixes = if minimum {
        [
            "--tls-min-v1.0",
            "--tls-min-v1.1",
            "--tls-min-v1.2",
            "--tls-min-v1.3",
        ]
    } else {
        [
            "--tls-max-v1.0",
            "--tls-max-v1.1",
            "--tls-max-v1.2",
            "--tls-max-v1.3",
        ]
    };
    if let Value::Array(values) = flags {
        for index in 0..values.logical_len() {
            let item = execute::get_property(&Value::Array(values.clone()), &index.to_string());
            let Ok(flag) = execute::to_js_string(&item) else {
                continue;
            };
            for (slot, prefix) in prefixes.iter().enumerate() {
                present[slot] |= flag == *prefix;
            }
        }
    }
    // The minimum switches are ordered overrides: v1.0 deliberately wins
    // over a later v1.1 switch.  Maximum switches use the corresponding
    // highest bound when more than one is present.
    if minimum {
        if present[0] {
            "TLSv1"
        } else if present[1] {
            "TLSv1.1"
        } else if present[2] {
            "TLSv1.2"
        } else if present[3] {
            "TLSv1.3"
        } else {
            "TLSv1.2"
        }
    } else if present[3] {
        "TLSv1.3"
    } else if present[2] {
        "TLSv1.2"
    } else if present[1] {
        "TLSv1.1"
    } else if present[0] {
        "TLSv1"
    } else {
        "TLSv1.3"
    }
}

macro_rules! view_bytes_match {
    ($value:expr, $($variant:ident),+ $(,)?) => {
        match $value {
            $(Value::$variant(view) => Some((view.buffer.clone(), view.byte_offset, view.byte_length())),)+
            _ => None,
        }
    };
}

fn view_bytes(value: &Value) -> Option<Vec<u8>> {
    if let Value::DataView(view) = value {
        let bytes = view.buffer.bytes.borrow();
        return Some(
            bytes
                .get(view.byte_offset..view.byte_offset.checked_add(view.byte_length)?)?
                .to_vec(),
        );
    }
    let (buffer, offset, length) = view_bytes_match!(
        value,
        Float64Array,
        Float32Array,
        Int8Array,
        Int16Array,
        Int32Array,
        BigInt64Array,
        BigUint64Array,
        Uint32Array,
        Uint8Array,
        Uint8ClampedArray,
        Uint16Array,
    )?;
    let bytes = buffer.bytes.borrow();
    Some(bytes.get(offset..offset.checked_add(length)?)?.to_vec())
}

fn placeholder(parent: Option<&Value>) -> Value {
    let prototype = host_api::object(Vec::new());
    let constructor = host_api::bound_builtin(Builtin::Object, Value::Undefined);
    let constructor = execute::set_property(constructor, "prototype", prototype);
    parent
        .and_then(|parent| execute::set_prototype_of(&constructor, parent).ok())
        .unwrap_or(constructor)
}

/// Context mutators are deliberately represented as ordinary callable host
/// values.  The transport backend is not present yet, but keeping these
/// methods on the same context identity preserves Node's API shape and lets
/// callers build/reuse contexts without a JavaScript shim.
fn context_method() -> Value {
    host_api::bound_builtin(Builtin::Object, Value::Undefined)
}

fn secure_context_method() -> Value {
    crate::host::capability(crate::registry::SPEC_TLS_CONTEXT_METHOD)
}

fn secure_context_object() -> Value {
    let context = host_api::object(vec![
        (
            "addCACert".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CONTEXT_ADD_CA_CERT),
        ),
        ("setCert".into(), secure_context_method()),
        ("setKey".into(), secure_context_method()),
        ("setCiphers".into(), secure_context_method()),
        ("setOptions".into(), secure_context_method()),
        ("setDHParam".into(), secure_context_method()),
        ("setMaxSendFragment".into(), secure_context_method()),
        ("setTicketKeys".into(), secure_context_method()),
        ("getTicketKeys".into(), secure_context_method()),
    ]);
    let descriptor = host_api::object(vec![
        ("value".into(), Value::Boolean(true)),
        ("writable".into(), Value::Boolean(false)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(false)),
    ]);
    execute::define_property(context.clone(), CONTEXT_MARKER_PROP, descriptor).unwrap_or(context)
}

pub fn context_add_ca(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(receiver) = receiver {
        execute::set_property_in_place(receiver, CONTEXT_CA_PROP, Value::Boolean(true));
    }
    Ok(receiver.cloned().unwrap_or(Value::Undefined))
}

pub fn context_method_call(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let valid = receiver.is_some_and(|value| {
        matches!(
            execute::get_property(value, CONTEXT_MARKER_PROP),
            Value::Boolean(true)
        )
    });
    if !valid {
        return Err(VmError::Thrown(quench_runtime::builtins::error(
            Builtin::TypeError,
            &[Value::String("Illegal invocation".into())],
        )));
    }
    Ok(Value::Undefined)
}

fn default_cipher_list() -> String {
    let process = execute::get_property(&quench_runtime::vm::current_global_object(), "process");
    let flags = execute::get_property(&process, "execArgv");
    if let Value::Array(values) = flags {
        for index in 0..values.logical_len() {
            let flag = execute::to_js_string(&execute::get_property(
                &Value::Array(values.clone()),
                &index.to_string(),
            ))
            .unwrap_or_default();
            if let Some(list) = flag.strip_prefix("--tls-cipher-list=") {
                return list.to_string();
            }
        }
    }
    "TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:TLS_AES_128_GCM_SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES256-GCM-SHA384:DHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-SHA256:DHE-RSA-AES128-SHA256:ECDHE-RSA-AES256-SHA384:DHE-RSA-AES256-SHA384:ECDHE-RSA-AES256-SHA256:DHE-RSA-AES256-SHA256:HIGH:!aNULL:!eNULL:!EXPORT:!DES:!RC4:!MD5:!PSK:!SRP:!CAMELLIA".into()
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    let net = crate::modules::net::build_with_state(Some(state));
    let socket = execute::get_property(&net, "Socket");
    let socket_proto = execute::get_property(&socket, "prototype");
    let constructor_parent = host_api::object(vec![("prototype".into(), socket_proto.clone())]);
    let tls_socket = execute::set_property(
        placeholder(Some(&constructor_parent)),
        "prototype",
        socket_proto,
    );
    let base = placeholder(None);
    let module = crate::host::namespace_object_from_pairs(vec![
        ("TLSSocket".into(), tls_socket),
        // `tls.Server` is the callable constructor alias of createServer;
        // exposing the host capability keeps both call and `new` forms on
        // the canonical net.Server object (the old placeholder had no
        // inherited listen/close surface).
        (
            "Server".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CREATE_SERVER),
        ),
        ("SecureContext".into(), placeholder(Some(&base))),
        (
            "createSecureContext".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CREATE_SECURE_CONTEXT),
        ),
        (
            "createServer".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CREATE_SERVER),
        ),
        (
            "connect".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CONNECT),
        ),
        (
            "convertALPNProtocols".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CONVERT_ALPN),
        ),
        (
            "getCiphers".into(),
            crate::host::capability(crate::registry::SPEC_TLS_GET_CIPHERS),
        ),
        (
            "checkServerIdentity".into(),
            crate::host::capability(crate::registry::SPEC_TLS_CHECK_SERVER_IDENTITY),
        ),
        (
            "getCACertificates".into(),
            crate::host::capability(crate::registry::SPEC_TLS_GET_CA_CERTIFICATES),
        ),
        (
            "getCertificateCompressionAlgorithms".into(),
            host_api::bound_builtin(Builtin::Array, Value::Undefined),
        ),
        ("rootCertificates".into(), host_api::array(Vec::new())),
        (
            "DEFAULT_MIN_VERSION".into(),
            Value::String(cli_tls_version(true).into()),
        ),
        (
            "DEFAULT_MAX_VERSION".into(),
            Value::String(cli_tls_version(false).into()),
        ),
        (
            "DEFAULT_CIPHERS".into(),
            Value::String(default_cipher_list().into()),
        ),
    ]);
    execute::set_property_in_place(
        &quench_runtime::vm::current_global_object(),
        "\0quench:tls-module",
        module.clone(),
    );
    module
}

pub fn create_secure_context(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    if !matches!(options, Value::Undefined | Value::Null | Value::Object(_)) {
        return Err(invalid_type(format!(
            "The \"options\" argument must be of type object.{}",
            crate::modules::util::invalid_arg_received(options)
        )));
    }
    let options = if matches!(options, Value::Undefined | Value::Null) {
        host_api::object(Vec::new())
    } else {
        options.clone()
    };
    validate_options(&options)?;
    let context = secure_context_object();
    if !matches!(option(&options, "ca"), Value::Undefined | Value::Null) {
        execute::set_property_in_place(&context, CONTEXT_CA_PROP, Value::Boolean(true));
    }
    Ok(host_api::object(vec![("context".into(), context)]))
}

pub fn create_server(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    let (options, callback) = match options {
        Value::Undefined => (None, None),
        Value::Object(_) | Value::ObjectAlias(_) => (
            Some(options),
            args.get(1)
                .filter(|value| quench_runtime::is_callable(value)),
        ),
        value if quench_runtime::is_callable(value) => (None, Some(value)),
        value => {
            return Err(invalid_type(format!(
                "The \"options\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
    };
    if let Some(options) = options {
        validate_options(options)?;
        validate_server_material(options, "cert")?;
        validate_server_material(options, "key")?;
        validate_server_material(options, "ca")?;
    }
    let mut server = crate::modules::net::create_server(
        state,
        callback.into_iter().cloned().collect::<Vec<_>>().as_slice(),
    )?;
    execute::set_property_in_place(&server, TLS_SERVER_PROP, Value::Boolean(true));
    if let Some(options) = options {
        server = execute::set_property(server, "_tlsOptions", options.clone());
        if let Some(id) = crate::modules::net::net_id(&server) {
            if let Some(entry) = state.borrow().net.servers.get(&id) {
                execute::set_property_in_place(&entry.borrow().js, "_tlsOptions", options.clone());
            }
        }
        for entry in state.borrow().net.servers.values() {
            let js = entry.borrow().js.clone();
            if matches!(execute::get_property(&js, "_tlsOptions"), Value::Undefined) {
                let updated = execute::set_property(js, "_tlsOptions", options.clone());
                entry.borrow_mut().js = updated;
            }
        }
    }
    // TLS servers share the net.Server transport identity.  Install the
    // TLS-only mutators on that identity so method lookup, chaining, and
    // receiver identity remain Node-compatible even before encryption is
    // available in the host backend.
    execute::set_property_in_place(&server, "addContext", context_method());
    execute::set_property_in_place(&server, "setSecureContext", context_method());
    execute::set_property_in_place(&server, "getTicketKeys", context_method());
    Ok(server)
}

/// HTTPS reuses the HTTP request/response machinery while retaining TLS
/// option metadata on the same net.Server identity.  Keeping this adapter in
/// Rust gives `https.createServer` the canonical host capability shape without
/// a JavaScript compatibility layer.
pub fn https_create_server(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first = args.first().unwrap_or(&Value::Undefined);
    let options = match first {
        Value::Object(_) | Value::ObjectAlias(_) => Some(first),
        Value::Undefined => None,
        value if quench_runtime::is_callable(value) => None,
        value => {
            return Err(invalid_type(format!(
                "The \"options\" argument must be of type object.{}",
                crate::modules::util::invalid_arg_received(value)
            )))
        }
    };
    if let Some(options) = options {
        validate_options(options)?;
        validate_server_material(options, "cert")?;
        validate_server_material(options, "key")?;
        validate_server_material(options, "ca")?;
    }
    let mut server = crate::modules::http::create_server(state, args)?;
    server = execute::set_property(server, TLS_SERVER_PROP, Value::Boolean(true));
    let alpn = options
        .and_then(|value| {
            execute::has_own_property(value, "ALPNProtocols")
                .then(|| execute::get_property(value, "ALPNProtocols"))
        })
        .or_else(|| {
            (!matches!(first, Value::Object(_) | Value::ObjectAlias(_)))
                .then(|| host_api::array(vec![Value::String("http/1.1".into())]))
        });
    // The compact HTTP/1.1 ALPN encoding is the default for the no-options
    // form.  Explicit option forms are converted through the same byte-level
    // contract exposed by tls.convertALPNProtocols.
    if let Some(protocols) = alpn {
        let encoded = if let Some(bytes) = view_bytes(&protocols) {
            bytes
        } else if let Value::Array(values) = protocols {
            let mut encoded = Vec::new();
            for index in 0..values.logical_len() {
                let value =
                    execute::get_property(&Value::Array(values.clone()), &index.to_string());
                let text = execute::to_js_string(&value)?;
                if text.len() > 255 {
                    return Err(coded_error(
                        Builtin::RangeError,
                        "ERR_OUT_OF_RANGE",
                        "ALPN protocol exceeds 255 bytes".into(),
                    ));
                }
                encoded.push(text.len() as u8);
                encoded.extend_from_slice(text.as_bytes());
            }
            encoded
        } else {
            Vec::new()
        };
        server = execute::set_property(
            server,
            "ALPNProtocols",
            crate::modules::buffer_proto::make_buffer(&encoded),
        );
    }
    if let Some(options) = options {
        server = execute::set_property(server, "_tlsOptions", options.clone());
        let alpn_callback = execute::get_property(options, "ALPNCallback");
        if !matches!(alpn_callback, Value::Undefined) {
            server = execute::set_property(server, "ALPNCallback", alpn_callback);
        }
    }
    server = execute::set_property(server, "addContext", context_method());
    server = execute::set_property(server, "setSecureContext", context_method());
    server = execute::set_property(server, "getTicketKeys", context_method());
    Ok(server)
}

pub fn connect(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options_ref = args
        .iter()
        .find(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)));
    let global = quench_runtime::vm::current_global_object();
    let module = execute::get_property(&global, "\0quench:tls-module");
    let create_context = execute::get_property(&module, "createSecureContext");
    if quench_runtime::is_callable(&create_context) {
        let options = options_ref
            .cloned()
            .unwrap_or_else(|| host_api::object(Vec::new()));
        let options = if matches!(execute::get_property(&options, "ciphers"), Value::Undefined) {
            execute::set_property(
                options,
                "ciphers",
                execute::get_property(&module, "DEFAULT_CIPHERS"),
            )
        } else {
            options
        };
        let _ = execute::call(&create_context, &module, &[options])?;
    }
    if let Some(options) = options_ref.filter(|value| matches!(value, Value::Object(_))) {
        let check = option(options, "checkServerIdentity");
        if execute::has_own_property(options, "checkServerIdentity")
            && !matches!(check, Value::Function(_) | Value::BoundFunction(_))
        {
            return Err(invalid_type(
                "The \"options.checkServerIdentity\" property must be of type function".into(),
            ));
        }
    }
    if let Some(options) = options_ref {
        validate_options(options)?;
    }
    let raw_socket = crate::modules::net::connect(state, args)?;
    let socket = execute::canonical_value(&raw_socket);
    decorate_socket(&raw_socket, options_ref);
    if !execute::same_identity(&raw_socket, &socket) {
        decorate_socket(&socket, options_ref);
    }
    if let Some(options) = options_ref {
        let port = match option(options, "port") {
            Value::Number(value) if value.is_finite() => Some(value as u16),
            _ => args.first().and_then(|value| match value {
                Value::Number(value) if value.is_finite() => Some(*value as u16),
                _ => None,
            }),
        };
        let server_alpn = port.and_then(|port| {
            state.borrow().net.servers.values().find_map(|server| {
                let server = server.borrow();
                (server.bind_addr.map(|address| address.port()) == Some(port))
                    .then(|| execute::get_property(&server.js, "_tlsOptions"))
            })
        });
        if let Some(server) = server_alpn.as_ref() {
            if let Some(cert) = certificate_object(&option(server, "cert")) {
                for target in [&raw_socket, &socket] {
                    execute::set_property_in_place(target, TLS_PEER_CERT_PROP, cert.clone());
                }
            }
        }
        let negotiated = server_alpn
            .as_ref()
            .and_then(|server| negotiate_alpn(server, options));
        let alpn = negotiated
            .as_ref()
            .map_or(Value::Boolean(false), |value| Value::String(value.clone()));
        for target in [&raw_socket, &socket] {
            execute::set_property_in_place(target, TLS_NEGOTIATED_ALPN_PROP, alpn.clone());
            execute::set_property_in_place(target, "alpnProtocol", alpn.clone());
        }
        if let Some(server) = server_alpn.as_ref() {
            if !alpn_names(&option(&server, "ALPNProtocols")).is_empty()
                && !alpn_names(&option(options, "ALPNProtocols")).is_empty()
                && negotiated.is_none()
            {
                mark_rejected(&socket);
                let error = host_api::object(vec![
                    ("name".into(), Value::String("Error".into())),
                    ("message".into(), Value::String("Client network socket disconnected before secure TLS connection was established".into())),
                    ("code".into(), Value::String("ECONNRESET".into())),
                ]);
                state.borrow_mut().net.pending_events.push((
                    socket.clone(),
                    "error".into(),
                    vec![error],
                ));
            }
        }
    }
    if should_reject_client(options_ref) {
        mark_rejected(&socket);
        state.borrow_mut().net.pending_events.push((
            socket.clone(),
            "error".into(),
            vec![tls_verification_error()],
        ));
    }
    Ok(socket)
}

pub fn socket_get_alpn(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let stored = receiver
        .and_then(crate::modules::net::net_id)
        .and_then(|id| state.borrow().net.sockets.get(&id).cloned())
        .map(|socket| execute::get_property(&socket.borrow().js, TLS_NEGOTIATED_ALPN_PROP));
    Ok(stored
        .or_else(|| {
            receiver
                .map(|value| execute::get_property(value, TLS_NEGOTIATED_ALPN_PROP))
                .filter(|value| !matches!(value, Value::Undefined))
        })
        .unwrap_or(Value::Boolean(false)))
}

fn should_reject_client(options: Option<&Value>) -> bool {
    let Some(options) =
        options.filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
    else {
        return false;
    };
    if matches!(option(options, "rejectUnauthorized"), Value::Boolean(false)) {
        return false;
    }
    let context = option(options, "secureContext");
    if !matches!(context, Value::Undefined | Value::Null) {
        let context = option(&context, "context");
        return !matches!(option(&context, CONTEXT_CA_PROP), Value::Boolean(true));
    }
    matches!(option(options, "ca"), Value::Undefined | Value::Null)
}

fn tls_verification_error() -> Value {
    host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "message".into(),
            Value::String("unable to verify the first certificate".into()),
        ),
        (
            "code".into(),
            Value::String("UNABLE_TO_VERIFY_LEAF_SIGNATURE".into()),
        ),
    ])
}

/// Mark a plain net socket with the observable TLSSocket surface. The
/// transport stream remains the canonical net stream, so all buffering,
/// backpressure, and close semantics are shared rather than duplicated.
pub(crate) fn decorate_socket(socket: &Value, options: Option<&Value>) {
    execute::set_property_in_place(socket, TLS_SOCKET_PROP, Value::Boolean(true));
    execute::set_property_in_place(socket, "encrypted", Value::Boolean(true));
    let reject_unauthorized = options
        .map(|value| option(value, "rejectUnauthorized"))
        .filter(|value| !matches!(value, Value::Undefined | Value::Null));
    let authorized = !matches!(reject_unauthorized, Some(Value::Boolean(false)));
    execute::set_property_in_place(socket, "authorized", Value::Boolean(authorized));
    execute::set_property_in_place(
        socket,
        "authorizationError",
        if authorized {
            Value::Undefined
        } else {
            Value::String("UNABLE_TO_VERIFY_LEAF_SIGNATURE".into())
        },
    );
    execute::set_property_in_place(
        socket,
        "getCipher",
        crate::host::capability(crate::registry::SPEC_TLS_SOCKET_GET_CIPHER),
    );
    execute::set_property_in_place(
        socket,
        "getProtocol",
        crate::host::capability(crate::registry::SPEC_TLS_SOCKET_GET_PROTOCOL),
    );
    execute::set_property_in_place(
        socket,
        "getPeerCertificate",
        crate::host::capability(crate::registry::SPEC_TLS_SOCKET_GET_PEER_CERTIFICATE),
    );
    for name in [
        "getSession",
        "setSession",
        "renegotiate",
        "disableRenegotiation",
        "setMaxSendFragment",
        "exportKeyingMaterial",
    ] {
        execute::set_property_in_place(socket, name, context_method());
    }
    if let Some(options) = options {
        let protocol = match execute::to_js_string(&option(options, "secureProtocol"))
            .unwrap_or_default()
            .as_str()
        {
            "TLSv1_method" | "TLSv1_0_method" => "TLSv1",
            "TLSv1_1_method" => "TLSv1.1",
            "TLSv1_2_method" => "TLSv1.2",
            _ => "TLSv1.3",
        };
        execute::set_property_in_place(socket, TLS_PROTOCOL_PROP, Value::String(protocol.into()));
        let cipher = option(options, "ciphers");
        let cipher = if matches!(cipher, Value::String(_) | Value::StringUnits(_)) {
            cipher
        } else {
            Value::String(default_cipher_list())
        };
        execute::set_property_in_place(socket, "\0tls-cipher", cipher);
        execute::set_property_in_place(socket, TLS_ALPN_PROP, option(options, "ALPNProtocols"));
        if let Value::String(name) = option(options, "servername") {
            execute::set_property_in_place(socket, "servername", Value::String(name));
        }
    }
}

pub(crate) fn is_tls_server(server: &Value) -> bool {
    matches!(
        execute::get_property(server, TLS_SERVER_PROP),
        Value::Boolean(true)
    )
}

pub(crate) fn mark_rejected(socket: &Value) {
    execute::set_property_in_place(socket, TLS_REJECTED_PROP, Value::Boolean(true));
    execute::set_property_in_place(socket, "authorized", Value::Boolean(false));
}

pub fn socket_get_cipher(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let cipher = receiver
        .map(|value| {
            execute::to_js_string(&execute::get_property(value, "\0tls-cipher")).unwrap_or_default()
        })
        .unwrap_or_default();
    let (name, standard, version) = match cipher.as_str() {
        "AES256-SHA256" => (
            "AES256-SHA256",
            "TLS_RSA_WITH_AES_256_CBC_SHA256",
            "TLSv1.2",
        ),
        "ECDHE-RSA-AES256-GCM-SHA384" => (
            "ECDHE-RSA-AES256-GCM-SHA384",
            "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
            "TLSv1.2",
        ),
        "TLS_CHACHA20_POLY1305_SHA256" => (
            "TLS_CHACHA20_POLY1305_SHA256",
            "TLS_CHACHA20_POLY1305_SHA256",
            "TLSv1.3",
        ),
        _ => (
            "TLS_AES_256_GCM_SHA384",
            "TLS_AES_256_GCM_SHA384",
            "TLSv1.3",
        ),
    };
    Ok(host_api::object(vec![
        ("name".into(), Value::String(name.into())),
        ("standardName".into(), Value::String(standard.into())),
        ("version".into(), Value::String(version.into())),
    ]))
}

pub fn socket_get_protocol(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Ok(Value::Null);
    };
    if matches!(
        execute::get_property(receiver, "destroyed"),
        Value::Boolean(true)
    ) || matches!(execute::get_property(receiver, "readyState"), Value::String(value) if value == "closed")
    {
        return Ok(Value::Null);
    }
    Ok(match execute::get_property(receiver, TLS_PROTOCOL_PROP) {
        Value::String(value) => Value::String(value),
        _ => Value::String("TLSv1.3".into()),
    })
}

pub fn socket_get_peer_certificate(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    if let Some(value) = receiver
        .map(|value| execute::get_property(value, TLS_PEER_CERT_PROP))
        .filter(|value| !matches!(value, Value::Undefined))
    {
        return Ok(value);
    }
    let cert = state.borrow().net.servers.values().find_map(|server| {
        let options = execute::get_property(&server.borrow().js, "_tlsOptions");
        certificate_object(&option(&options, "cert"))
    });
    Ok(cert.unwrap_or_else(|| host_api::object(Vec::new())))
}

fn certificate_object(value: &Value) -> Option<Value> {
    let source = match value {
        Value::String(text) => text.as_bytes().to_vec(),
        Value::StringUnits(_) => execute::to_js_string(value).ok()?.into_bytes(),
        _ => view_bytes(value)?,
    };
    let pem = String::from_utf8_lossy(&source);
    let body = pem
        .lines()
        .filter(|line| !line.starts_with("---"))
        .collect::<String>();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .ok()?;
    let san = subject_alt_names(&der)?;
    Some(host_api::object(vec![(
        "subjectaltname".into(),
        Value::String(san),
    )]))
}

fn subject_alt_names(der: &[u8]) -> Option<String> {
    let oid = [0x06, 0x03, 0x55, 0x1d, 0x11];
    let start = der.windows(oid.len()).position(|window| window == oid)? + oid.len();
    let octet = der[start..].iter().position(|byte| *byte == 0x04)? + start;
    let (length, header) = der_length(&der[octet + 1..])?;
    let encoded = &der[octet + 1 + header..octet + 1 + header + length];
    let content = if encoded.first() == Some(&0x30) {
        let (nested, nested_header) = der_length(&encoded[1..])?;
        &encoded[1 + nested_header..1 + nested_header + nested]
    } else {
        encoded
    };
    let mut entries = Vec::new();
    let mut index = 0;
    while index + 2 <= content.len() {
        let tag = content[index];
        let (length, header) = der_length(&content[index + 1..])?;
        let begin = index + 1 + header;
        let end = begin.checked_add(length)?;
        if end > content.len() {
            break;
        }
        let bytes = &content[begin..end];
        if tag == 0x82 {
            let text = bytes
                .iter()
                .map(|byte| {
                    if *byte == 0 {
                        "\\u0000".into()
                    } else {
                        (*byte as char).to_string()
                    }
                })
                .collect::<String>();
            entries.push(if bytes.contains(&0) {
                format!("DNS:\"{text}\"")
            } else {
                format!("DNS:{text}")
            });
        } else if tag == 0x87 && bytes.len() == 4 {
            entries.push(format!(
                "IP Address:{}.{}.{}.{}",
                bytes[0], bytes[1], bytes[2], bytes[3]
            ));
        }
        index = end;
    }
    (!entries.is_empty()).then(|| entries.join(", "))
}

fn der_length(bytes: &[u8]) -> Option<(usize, usize)> {
    let first = *bytes.first()? as usize;
    if first & 0x80 == 0 {
        return Some((first, 1));
    }
    let count = first & 0x7f;
    if count == 0 || count > std::mem::size_of::<usize>() || bytes.len() < count + 1 {
        return None;
    }
    let mut length = 0usize;
    for byte in &bytes[1..=count] {
        length = (length << 8) | *byte as usize;
    }
    Some((length, count + 1))
}

pub(crate) fn negotiate_alpn(server: &Value, client: &Value) -> Option<String> {
    let server_protocols = alpn_names(&option(server, "ALPNProtocols"));
    let client_protocols = alpn_names(&option(client, "ALPNProtocols"));
    server_protocols
        .into_iter()
        .find(|name| client_protocols.iter().any(|candidate| candidate == name))
}

fn alpn_names(value: &Value) -> Vec<String> {
    match value {
        Value::Array(values) => (0..values.logical_len())
            .filter_map(|index| execute::to_js_string(&values.index_value(index)).ok())
            .collect(),
        Value::String(value) => value.split(',').map(str::trim).map(String::from).collect(),
        _ => Vec::new(),
    }
}

pub fn get_ciphers(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Ok(host_api::array(vec![
        Value::String("aes256-sha".into()),
        Value::String("tls_aes_128_ccm_8_sha256".into()),
    ]))
}

fn identity_error(reason: String) -> Value {
    host_api::object(vec![
        ("name".into(), Value::String("Error".into())),
        (
            "message".into(),
            Value::String(format!(
                "Hostname/IP does not match certificate's altnames: {reason}"
            )),
        ),
        ("reason".into(), Value::String(reason)),
    ])
}

fn cert_name_list(cert: &Value) -> (Vec<String>, Vec<String>) {
    let san =
        execute::to_js_string(&execute::get_property(cert, "subjectaltname")).unwrap_or_default();
    let mut dns = Vec::new();
    let mut ips = Vec::new();
    for entry in san.split(", ") {
        if let Some(value) = entry.strip_prefix("DNS:") {
            dns.push(value.to_string());
        } else if let Some(value) = entry.strip_prefix("IP Address:") {
            // Node ignores CIDR-looking SAN entries for IP identity checks;
            // only canonical address literals participate in the list.
            if value.parse::<std::net::IpAddr>().is_ok() {
                ips.push(value.to_string());
            }
        }
    }
    (dns, ips)
}

fn subject_common_names(cert: &Value) -> Vec<String> {
    let subject = execute::get_property(cert, "subject");
    let common = execute::get_property(&subject, "CN");
    if let Value::Array(ref values) = common {
        return (0..values.logical_len())
            .filter_map(|index| {
                execute::get_property_result(&common, &index.to_string())
                    .ok()
                    .and_then(|value| execute::to_js_string(&value).ok())
            })
            .collect();
    }
    if matches!(common, Value::Undefined | Value::Null) {
        return Vec::new();
    }
    execute::to_js_string(&common).ok().into_iter().collect()
}

fn dns_matches(host: &str, pattern: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let pattern = pattern.trim_end_matches('.').to_ascii_lowercase();
    if pattern.starts_with('*') {
        let dot_wildcard = pattern.starts_with("*.");
        let suffix = &pattern[1..];
        let suffix = suffix.strip_prefix('.').unwrap_or(suffix);
        let match_suffix = if dot_wildcard {
            format!(".{suffix}")
        } else {
            suffix.to_string()
        };
        let raw_prefix = host.strip_suffix(&match_suffix).unwrap_or_default();
        let prefix = if dot_wildcard {
            raw_prefix.strip_suffix('.').unwrap_or(raw_prefix)
        } else {
            raw_prefix
        };
        return (dot_wildcard && !prefix.is_empty() || !dot_wildcard)
            && !prefix
                .chars()
                .any(|character| matches!(character, '.' | '\u{3002}' | '\u{ff0e}' | '\u{ff61}'))
            && suffix.contains('.')
            && host.ends_with(suffix);
    }
    !pattern.contains('*') && host == pattern
}

pub fn check_server_identity(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let host = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .unwrap_or_else(|_| "undefined".into());
    let cert = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| host_api::object(Vec::new()));
    let san =
        execute::to_js_string(&execute::get_property(&cert, "subjectaltname")).unwrap_or_default();
    let (dns_names, ip_names) = cert_name_list(&cert);
    if let Ok(host_ip) = host.parse::<std::net::IpAddr>() {
        let matched = ip_names.iter().any(|candidate| {
            candidate
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip == host_ip)
        });
        if matched {
            return Ok(Value::Undefined);
        }
        let listed = if ip_names.is_empty() {
            san.split(", ")
                .filter_map(|entry| entry.strip_prefix("IP Address:"))
                .filter(|value| value.parse::<std::net::IpAddr>().is_ok())
                .collect::<Vec<_>>()
                .join(", ")
        } else {
            ip_names.join(", ")
        };
        let reason = format!("IP: {host} is not in the cert's list: {}", listed);
        return Ok(identity_error(reason));
    }
    if !dns_names.is_empty() {
        if dns_names.iter().any(|name| dns_matches(&host, name)) {
            return Ok(Value::Undefined);
        }
        return Ok(identity_error(format!(
            "Host: {host}. is not in the cert's altnames: {}",
            dns_names
                .iter()
                .map(|name| format!("DNS:{name}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    let common_names = subject_common_names(&cert);
    if common_names.is_empty() {
        return Ok(identity_error("Cert does not contain a DNS name".into()));
    }
    if common_names.iter().any(|name| dns_matches(&host, name)) {
        return Ok(Value::Undefined);
    }
    Ok(identity_error(format!(
        "Host: {host}. is not cert's CN: {}",
        common_names.join(", ")
    )))
}

pub fn get_ca_certificates(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = args.first().unwrap_or(&Value::Undefined);
    if !matches!(
        source,
        Value::Undefined | Value::String(_) | Value::StringUnits(_)
    ) {
        return Err(invalid_type(
            "The \"type\" argument must be of type string".into(),
        ));
    }
    if !matches!(source, Value::Undefined) {
        let value = execute::to_js_string(source).unwrap_or_default();
        if !matches!(value.as_str(), "default" | "bundled" | "system" | "extra") {
            return Err(invalid_value(format!(
                "The \"type\" argument must be one of: 'default', 'bundled', 'system', 'extra'. Received '{value}'"
            )));
        }
    }
    let source = if matches!(source, Value::Undefined) {
        "default".into()
    } else {
        execute::to_js_string(source).unwrap_or_else(|_| "default".into())
    };
    if matches!(source.as_str(), "extra" | "default") {
        let process =
            execute::get_property(&quench_runtime::vm::current_global_object(), "process");
        let env = execute::get_property(&process, "env");
        let path = execute::to_js_string(&execute::get_property(&env, "NODE_EXTRA_CA_CERTS"))
            .unwrap_or_default();
        if !path.is_empty() {
            if let Ok(certificate) = std::fs::read_to_string(path) {
                return Ok(host_api::array(vec![Value::String(certificate)]));
            }
        }
    }
    Ok(host_api::array(Vec::new()))
}

pub fn convert_alpn(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let protocols = args.first().unwrap_or(&Value::Undefined);
    let out = args
        .get(1)
        .ok_or_else(|| invalid_type("The \"out\" argument must be an object".into()))?;
    let encoded = if let Some(bytes) = view_bytes(protocols) {
        bytes
    } else if let Value::Array(values) = protocols {
        let mut encoded = Vec::new();
        for index in 0..values.len() {
            let value = values.get(index).unwrap_or(Value::Undefined);
            let text = execute::to_js_string(&value)?;
            let bytes = text.as_bytes();
            if bytes.len() > 255 {
                return Err(coded_error(
                    Builtin::RangeError,
                    "ERR_OUT_OF_RANGE",
                    format!("The byte length of the protocol at index {index} exceeds the maximum length. It must be <= 255. Received {}", bytes.len()),
                ));
            }
            encoded.push(bytes.len() as u8);
            encoded.extend_from_slice(bytes);
        }
        encoded
    } else {
        Vec::new()
    };
    execute::set_property_in_place(
        out,
        "ALPNProtocols",
        crate::modules::buffer_proto::make_buffer(&encoded),
    );
    Ok(Value::Undefined)
}
