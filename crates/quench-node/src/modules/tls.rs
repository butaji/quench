//! Rust-owned TLS API surface.
//!
//! Transport encryption is not available yet, but option validation and the
//! byte-level ALPN contract are ordinary Node semantics and belong here at
//! the host boundary rather than in a JavaScript shim.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::ops::Builtin;
use quench_runtime::value::Value;

use crate::host::HostState;

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
    if valid || matches!(value, Value::Undefined) {
        return Ok(());
    }
    Err(invalid_type(format!(
        "The \"options.{name}\" property must be of type {expected}.{}",
        crate::modules::util::invalid_arg_received(&value)
    )))
}

fn validate_options(options: &Value) -> Result<(), VmError> {
    for name in ["ciphers", "passphrase", "ecdhCurve"] {
        validate_type(options, name, "string")?;
    }
    for name in ["handshakeTimeout", "sessionTimeout"] {
        validate_type(options, name, "number")?;
    }
    let ticket_keys = option(options, "ticketKeys");
    if !matches!(ticket_keys, Value::Undefined) {
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
    Ok(())
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

pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    let net = crate::modules::net::build_with_state(Some(state));
    let socket = execute::get_property(&net, "Socket");
    let socket_proto = execute::get_property(&socket, "prototype");
    let tls_socket = execute::set_property(placeholder(Some(&socket)), "prototype", socket_proto);
    let base = placeholder(None);
    crate::host::namespace_object_from_pairs(vec![
        ("TLSSocket".into(), tls_socket),
        ("Server".into(), placeholder(Some(&base))),
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
            "getCertificateCompressionAlgorithms".into(),
            host_api::bound_builtin(Builtin::Array, Value::Undefined),
        ),
        ("rootCertificates".into(), host_api::array(Vec::new())),
        (
            "DEFAULT_MIN_VERSION".into(),
            Value::String("TLSv1.2".into()),
        ),
        (
            "DEFAULT_MAX_VERSION".into(),
            Value::String("TLSv1.3".into()),
        ),
    ])
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
    Ok(host_api::object(vec![(
        "context".into(),
        host_api::object(Vec::new()),
    )]))
}

pub fn create_server(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().unwrap_or(&Value::Undefined);
    if matches!(options, Value::Object(_)) {
        validate_options(options)?;
    }
    Err(unsupported("tls.createServer"))
}

pub fn connect(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(options) = args
        .first()
        .filter(|value| matches!(value, Value::Object(_)))
    {
        let check = option(options, "checkServerIdentity");
        if !matches!(
            check,
            Value::Undefined | Value::Null | Value::Function(_) | Value::BoundFunction(_)
        ) {
            return Err(invalid_type(
                "The \"options.checkServerIdentity\" property must be of type function".into(),
            ));
        }
    }
    Err(unsupported("tls.connect"))
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
