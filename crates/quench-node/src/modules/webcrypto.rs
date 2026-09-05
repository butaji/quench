//! Rust-owned WebCrypto boundary.
//!
//! The engine owns promises and typed-array storage; this module owns the
//! observable WebCrypto facts that do not require a separate JS runtime.

use std::cell::RefCell;
use std::rc::Rc;

use aes::{Aes128, Aes192, Aes256};
use aes_gcm::{
    aead::{Aead, Payload},
    KeyInit,
};
use base64::Engine;
use chacha20poly1305::ChaCha20Poly1305;
use cipher::{generic_array::GenericArray, BlockDecrypt, BlockEncrypt};
use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::ops::Builtin;
use quench_runtime::value::{ArrayBufferData, PromiseData, PromiseState, Value};
use rand::RngCore;
use sha3::{
    digest::ExtendableOutput, digest::Update as ShaUpdate, digest::XofReader, TurboShake128,
    TurboShake128Core, TurboShake256, TurboShake256Core,
};
use tiny_keccak::{Hasher, IntoXof, KangarooTwelve, Xof};

use crate::host::HostState;

pub(crate) const KEY_MARKER_PROP: &str = "\0quench:webcrypto:key";
pub(crate) const KEY_DATA_PROP: &str = "\0quench:webcrypto:key-data";
pub(crate) const KEY_FORMAT_PROP: &str = "\0quench:webcrypto:key-format";
pub(crate) const KEY_META_PROP: &str = "\0quench:webcrypto:key-meta";
const KEY_PUBLIC_ALGORITHM_PROP: &str = "\0quench:webcrypto:public-algorithm";
const KEY_PUBLIC_USAGES_PROP: &str = "\0quench:webcrypto:public-usages";

thread_local! {
    // CryptoKey instances can be created while a promise continuation is
    // running with a sandbox/worker global as the current VM realm.  The
    // constructor's prototype is a host fact owned by the root context, so
    // retain that identity once during module construction instead of
    // resolving a realm-local marker for every operation.
    static KEY_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

fn key_prototype() -> Value {
    KEY_PROTOTYPE.with(|prototype| {
        prototype.borrow().clone().unwrap_or_else(|| {
            let global = quench_runtime::vm::current_global_object();
            execute::get_property(&global, "__quench_crypto_key_prototype")
        })
    })
}

fn settled(result: Result<Value, VmError>) -> Value {
    Value::Promise(Rc::new(PromiseData::new(match result {
        Ok(value) => PromiseState::Fulfilled(value),
        Err(VmError::Thrown(value)) => PromiseState::Rejected(value),
        Err(_) => PromiseState::Rejected(Value::String("Operation failed".into())),
    })))
}

fn invalid_subtle_this(receiver: Option<&Value>) -> Option<Value> {
    let valid = receiver.is_some_and(|value| {
        matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && quench_runtime::is_callable(&execute::get_property(value, "digest"))
    });
    (!valid).then(|| {
        settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Value of \"this\" must be of type SubtleCrypto",
        )))
    })
}

fn error(kind: Builtin, code: Option<&str>, message: &str) -> VmError {
    let value = quench_runtime::builtins::error(kind, &[Value::String(message.into())]);
    let value = code.map_or(value.clone(), |code| {
        execute::set_property(value, "code", Value::String(code.into()))
    });
    VmError::Thrown(value)
}

pub fn illegal_constructor(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    Err(error(
        Builtin::TypeError,
        Some("ERR_ILLEGAL_CONSTRUCTOR"),
        "Illegal constructor",
    ))
}

fn bytes(value: &Value) -> Option<Vec<u8>> {
    macro_rules! view {
        ($view:expr) => {{
            let bytes = $view.buffer.bytes.borrow();
            Some(
                bytes
                    .get($view.byte_offset..$view.byte_offset + $view.byte_length())?
                    .to_vec(),
            )
        }};
    }
    match value {
        Value::ArrayBuffer(buffer) => Some(buffer.bytes.borrow().clone()),
        Value::DataView(view) => view!(view),
        Value::Float64Array(view) => view!(view),
        Value::Float32Array(view) => view!(view),
        Value::Int8Array(view) => view!(view),
        Value::Int16Array(view) => view!(view),
        Value::Int32Array(view) => view!(view),
        Value::BigInt64Array(view) => view!(view),
        Value::BigUint64Array(view) => view!(view),
        Value::Uint8Array(view) => view!(view),
        Value::Uint8ClampedArray(view) => view!(view),
        Value::Uint16Array(view) => view!(view),
        Value::Uint32Array(view) => view!(view),
        _ => None,
    }
}

fn array_buffer(data: &[u8]) -> Value {
    let buffer = Rc::new(ArrayBufferData::new(data.len()));
    buffer.bytes.borrow_mut().copy_from_slice(data);
    Value::ArrayBuffer(buffer)
}

fn key(
    prototype: &Value,
    algorithm: Value,
    extractable: bool,
    usages: Value,
    data: Option<Vec<u8>>,
) -> Value {
    let algorithm = match algorithm {
        Value::String(name) => host_api::object(vec![("name".into(), Value::String(name))]),
        value => value,
    };
    let algorithm = normalize_key_algorithm(algorithm);
    let usages = normalize_usages(&usages);
    let value = host_api::object(vec![
        ("type".into(), Value::String("secret".into())),
        ("algorithm".into(), algorithm),
        ("extractable".into(), Value::Boolean(extractable)),
        ("usages".into(), usages),
    ]);
    let metadata = value;
    let value = host_api::object(Vec::new());
    let value = execute::set_prototype_of(&value, prototype).unwrap_or(value);
    let value = define_hidden(value, KEY_MARKER_PROP, Value::Boolean(true));
    let value = define_hidden(value, KEY_META_PROP, metadata);
    define_hidden(
        value,
        KEY_DATA_PROP,
        crate::modules::buffer_proto::make_buffer(&data.unwrap_or_default()),
    )
}

fn normalize_key_algorithm(algorithm: Value) -> Value {
    let algorithm = crate::modules::clone::deep_clone(algorithm);
    let hash = execute::get_property(&algorithm, "hash");
    if let Value::String(name) = hash {
        let pairs = execute::own_enumerable_keys(&algorithm)
            .into_iter()
            .map(|key| {
                let value = if key == "hash" {
                    host_api::object(vec![("name".into(), Value::String(name.clone()))])
                } else {
                    execute::get_property(&algorithm, &key)
                };
                (key, value)
            })
            .collect();
        host_api::object(pairs)
    } else {
        algorithm
    }
}

pub(crate) fn clone_key(value: &Value) -> Option<Value> {
    if !matches!(
        execute::get_property(value, KEY_MARKER_PROP),
        Value::Boolean(true)
    ) {
        return None;
    }
    let metadata = execute::get_property(value, KEY_META_PROP);
    let algorithm =
        crate::modules::clone::deep_clone(execute::get_property(&metadata, "algorithm"));
    let usages = crate::modules::clone::deep_clone(execute::get_property(&metadata, "usages"));
    let extractable = matches!(
        execute::get_property(&metadata, "extractable"),
        Value::Boolean(true)
    );
    let key_type = execute::to_js_string(&execute::get_property(&metadata, "type")).ok()?;
    let format = execute::to_js_string(&execute::get_property(value, KEY_FORMAT_PROP))
        .unwrap_or_else(|_| "raw".into());
    let data =
        crate::modules::crypto::bytes_from_value(&execute::get_property(value, KEY_DATA_PROP));
    Some(key_metadata(
        key(&key_prototype(), algorithm, extractable, usages, data),
        &key_type,
        &format,
    ))
}

fn normalize_usages(value: &Value) -> Value {
    usage_array(&usage_names(value))
}

fn usage_names(value: &Value) -> Vec<String> {
    let length = match execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length > 0.0 => length as usize,
        _ => 0,
    };
    let mut requested = std::collections::HashSet::new();
    for index in 0..length {
        if let Ok(name) = execute::to_js_string(&execute::get_property(value, &index.to_string())) {
            requested.insert(name);
        }
    }
    [
        "encrypt",
        "decrypt",
        "sign",
        "verify",
        "deriveKey",
        "deriveBits",
        "wrapKey",
        "unwrapKey",
    ]
    .into_iter()
    .filter(|name| requested.contains(*name))
    .map(str::to_string)
    .collect()
}

fn usage_array(names: &[String]) -> Value {
    host_api::array(names.iter().cloned().map(Value::String).collect())
}

fn asymmetric_usages(name: &str, requested: &Value) -> Result<(Value, Value), VmError> {
    let (private_allowed, public_allowed): (&[&str], &[&str]) = match name {
        "RSA-OAEP" => (&["decrypt", "unwrapKey"], &["encrypt", "wrapKey"]),
        "RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448" => (&["sign"], &["verify"]),
        "ECDH" | "X25519" | "X448" => (&["deriveKey", "deriveBits"], &[]),
        _ => return Err(not_supported("Unrecognized algorithm name")),
    };
    let requested = usage_names(requested);
    if requested.is_empty() {
        return Err(usage_error("Usages cannot be empty"));
    }
    if requested.iter().any(|usage| {
        !private_allowed.contains(&usage.as_str()) && !public_allowed.contains(&usage.as_str())
    }) {
        return Err(usage_error("Unsupported key usage"));
    }
    let private = requested
        .iter()
        .filter(|usage| private_allowed.contains(&usage.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let public = requested
        .iter()
        .filter(|usage| public_allowed.contains(&usage.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    Ok((usage_array(&private), usage_array(&public)))
}

fn usage_error(message: &str) -> VmError {
    error(Builtin::Error, None, message)
}

fn key_metadata(value: Value, key_type: &str, format: &str) -> Value {
    let metadata = execute::get_property(&value, KEY_META_PROP);
    let _ = execute::set_property_in_place(&metadata, "type", Value::String(key_type.into()));
    define_hidden(value, KEY_FORMAT_PROP, Value::String(format.into()))
}

fn eval_function(source: &str) -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}

fn key_getter(name: &str) -> Option<Value> {
    let metadata = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_META_PROP.trim_start_matches('\0')
    );
    let marker = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_MARKER_PROP.trim_start_matches('\0')
    );
    let public_algorithm = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_PUBLIC_ALGORITHM_PROP.trim_start_matches('\0')
    );
    let public_usages = format!(
        "String.fromCharCode(0) + {:?}",
        KEY_PUBLIC_USAGES_PROP.trim_start_matches('\0')
    );
    let source = format!(
        "(name) => function() {{ const receiver = this; if ((receiver === null) || ((typeof receiver !== \"object\") && (typeof receiver !== \"function\")) || receiver[{marker}] !== true) {{ const error = new TypeError(\"Illegal invocation\"); error.code = \"ERR_INVALID_THIS\"; throw error; }} const value = receiver[{metadata}][name]; if (name === \"usages\") {{ const cached = receiver[{public_usages}]; if (cached !== undefined) return cached; const copy = Array.from(value); Object.defineProperty(receiver, {public_usages}, {{ value: copy, writable: true, configurable: true }}); return copy; }} if (name === \"algorithm\") {{ const cached = receiver[{public_algorithm}]; if (cached !== undefined) return cached; const copy = Object.assign({{}}, value); if (value.hash && typeof value.hash === \"object\") copy.hash = Object.assign({{}}, value.hash); if (value.publicExponent && typeof value.publicExponent === \"object\") copy.publicExponent = new Uint8Array(value.publicExponent); Object.defineProperty(receiver, {public_algorithm}, {{ value: copy, writable: true, configurable: true }}); return copy; }} return value; }}"
    );
    let factory = eval_function(&source).ok()?;
    execute::call(&factory, &Value::Undefined, &[Value::String(name.into())]).ok()
}

fn define_hidden(target: Value, name: &str, value: Value) -> Value {
    let descriptor = host_api::object(vec![
        ("value".into(), value),
        ("writable".into(), Value::Boolean(false)),
        ("enumerable".into(), Value::Boolean(false)),
        ("configurable".into(), Value::Boolean(false)),
    ]);
    execute::define_property(target.clone(), name, descriptor).unwrap_or(target)
}

pub fn get_random_values(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Value of \"this\" must be of type Crypto",
        ));
    };
    if !matches!(receiver, Value::Object(_) | Value::ObjectAlias(_)) {
        return Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Value of \"this\" must be of type Crypto",
        ));
    }
    let Some(value) = args.first() else {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "",
            "TypeMismatchError",
        )));
    };
    let valid = matches!(
        value,
        Value::Int8Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Int16Array(_)
            | Value::Uint16Array(_)
            | Value::Int32Array(_)
            | Value::Uint32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
    );
    if !valid {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "",
            "TypeMismatchError",
        )));
    }
    let Some((buffer, offset, length)) = typed_span(value) else {
        return Err(VmError::Thrown(quench_runtime::builtins::dom_exception(
            "",
            "TypeMismatchError",
        )));
    };
    if length > 65_536 {
        let message = Value::String("The requested length exceeds 65,536 bytes".into());
        let constructor = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "QuotaExceededError",
        );
        let error = execute::construct_value(&constructor, &[message])
            .ok()
            .and_then(|value| {
                let prototype = execute::get_property(&constructor, "prototype");
                execute::set_prototype_of(&value, &prototype).ok()
            })
            .unwrap_or_else(|| {
                quench_runtime::builtins::dom_exception(
                    "The requested length exceeds 65,536 bytes",
                    "QuotaExceededError",
                )
            });
        execute::set_property_in_place(&error, "quota", Value::Null);
        execute::set_property_in_place(&error, "requested", Value::Null);
        return Err(VmError::Thrown(error));
    }
    rand::thread_rng().fill_bytes(&mut buffer.bytes.borrow_mut()[offset..offset + length]);
    Ok(value.clone())
}

fn typed_span(value: &Value) -> Option<(Rc<ArrayBufferData>, usize, usize)> {
    macro_rules! span {
        ($view:expr) => {
            Some(($view.buffer.clone(), $view.byte_offset, $view.byte_length()))
        };
    }
    match value {
        Value::Int8Array(view) => span!(view),
        Value::Uint8Array(view) => span!(view),
        Value::Uint8ClampedArray(view) => span!(view),
        Value::Int16Array(view) => span!(view),
        Value::Uint16Array(view) => span!(view),
        Value::Int32Array(view) => span!(view),
        Value::Uint32Array(view) => span!(view),
        Value::BigInt64Array(view) => span!(view),
        Value::BigUint64Array(view) => span!(view),
        _ => None,
    }
}

pub fn digest(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let requested = args.first().unwrap_or(&Value::Undefined);
    let name_value = match requested {
        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(requested, "name"),
        value => value.clone(),
    };
    let algorithm = execute::to_js_string(&name_value)
        .unwrap_or_default()
        .to_ascii_uppercase()
        .replace('-', "");
    let Some(data) = args.get(1).and_then(bytes) else {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The data argument must be an ArrayBuffer or a view",
        ))));
    };
    let output = match algorithm.as_str() {
        "SHA1" | "SHA224" | "SHA256" | "SHA384" | "SHA512" | "SHA3256" | "SHA3384" | "SHA3512" => {
            let normalized = match algorithm.as_str() {
                "SHA1" => "sha1",
                "SHA224" => "sha224",
                "SHA256" => "sha256",
                "SHA384" => "sha384",
                "SHA512" => "sha512",
                "SHA3256" => "sha3-256",
                "SHA3384" => "sha3-384",
                _ => "sha3-512",
            };
            crate::modules::crypto::digest_bytes(normalized, &data)
                .map_err(|_| not_supported("Unrecognized algorithm"))
        }
        "CSHAKE128" | "CSHAKE256" | "SHAKE128" | "SHAKE256" => {
            let bits = execute::get_property(requested, "outputLength");
            let Value::Number(bits) = bits else {
                return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
            };
            if !bits.is_finite() || bits < 0.0 || bits > 2_147_483_647.0 {
                return Ok(settled(Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ))));
            }
            let normalized = if algorithm.ends_with("128") {
                "shake128"
            } else {
                "shake256"
            };
            crate::modules::crypto::shake_digest(
                normalized,
                &data,
                Value::Number((bits / 8.0).ceil()),
            )
            .map_err(|_| not_supported("Unrecognized algorithm"))
        }
        "TURBOSHAKE128" | "TURBOSHAKE256" => {
            let bits = execute::get_property(requested, "outputLength");
            let Value::Number(bits) = bits else {
                return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
            };
            let domain = match execute::get_property(requested, "domainSeparation") {
                Value::Undefined => 0x1f,
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && (1.0..=127.0).contains(&value) =>
                {
                    value as u8
                }
                _ => {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_OUT_OF_RANGE"),
                        "The domain separation must be between 1 and 127",
                    ))))
                }
            };
            if !bits.is_finite() || bits < 0.0 || bits > 2_147_483_647.0 {
                return Ok(settled(Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ))));
            }
            let length = (bits / 8.0).ceil() as usize;
            let output = if algorithm == "TURBOSHAKE128" {
                let mut hasher = TurboShake128::from_core(TurboShake128Core::new(domain));
                ShaUpdate::update(&mut hasher, &data);
                let mut reader = hasher.finalize_xof();
                let mut output = vec![0; length];
                reader.read(&mut output);
                output
            } else {
                let mut hasher = TurboShake256::from_core(TurboShake256Core::new(domain));
                ShaUpdate::update(&mut hasher, &data);
                let mut reader = hasher.finalize_xof();
                let mut output = vec![0; length];
                reader.read(&mut output);
                output
            };
            Ok(output)
        }
        "KT128" | "KT256" => {
            let bits = execute::get_property(requested, "outputLength");
            let Value::Number(bits) = bits else {
                return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
            };
            if !bits.is_finite() || bits < 0.0 || bits > 2_147_483_647.0 {
                return Ok(settled(Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ))));
            }
            let customization = execute::get_property(requested, "customization");
            let customization = match customization {
                Value::Undefined => Vec::new(),
                value => bytes(&value).unwrap_or_default(),
            };
            if customization.len() > 512 {
                return Ok(settled(Err(error(
                    Builtin::Error,
                    None,
                    "KangarooTwelveParams.customization must be at most 512 bytes",
                ))));
            }
            let mut hasher = KangarooTwelve::new(customization);
            hasher.update(&data);
            let mut reader = hasher.into_xof();
            let mut output = vec![0; (bits / 8.0).ceil() as usize];
            reader.squeeze(&mut output);
            Ok(output)
        }
        _ => return Ok(settled(Err(not_supported("Unrecognized algorithm name")))),
    };
    Ok(settled(output.map(|bytes| array_buffer(&bytes))))
}

fn not_supported(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    let value = execute::set_property(value, "name", Value::String("NotSupportedError".into()));
    VmError::Thrown(execute::set_property(
        value,
        "code",
        Value::String("ERR_OSSL_EVP_UNSUPPORTED".into()),
    ))
}

pub fn import_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let format = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !matches!(
        format.as_str(),
        "raw" | "raw-secret" | "raw-public" | "jwk" | "spki" | "pkcs8"
    ) {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_VALUE"),
            "The provided value is not a valid enum value of type KeyFormat",
        ))));
    }
    let algorithm = args.get(2).cloned().unwrap_or(Value::Undefined);
    let algorithm_name_upper = algorithm_name(&algorithm).to_ascii_uppercase();
    let raw_alias_unsupported = format == "raw-public"
        || (format == "raw-secret"
            && matches!(
                algorithm_name_upper.as_str(),
                "ECDSA" | "ECDH" | "ED25519" | "ED448" | "X25519" | "X448"
            ));
    if raw_alias_unsupported {
        let name = algorithm_name(&algorithm);
        return Ok(settled(Err(not_supported(&format!(
            "Unable to import {name} using {format} format"
        )))));
    }
    let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
    let usages = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    let data = if format == "jwk" {
        let encoded = execute::to_js_string(&execute::get_property(
            args.get(1).unwrap_or(&Value::Undefined),
            "k",
        ))
        .unwrap_or_default();
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(encoded)
            .ok()
    } else {
        args.get(1).and_then(bytes)
    };
    let prototype = key_prototype();
    let key_type = match format.as_str() {
        "pkcs8" => "private",
        "spki" => "public",
        _ => "secret",
    };
    Ok(settled(Ok(key_metadata(
        key(&prototype, algorithm, extractable, usages, data),
        key_type,
        &format,
    ))))
}

pub fn export_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let format = execute::to_js_string(args.first().unwrap_or(&Value::Undefined))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let key = args.get(1).unwrap_or(&Value::Undefined);
    if let Some(error) = invalid_key_this(key) {
        return Ok(settled(Err(error)));
    }
    if !matches!(
        format.as_str(),
        "raw" | "raw-secret" | "jwk" | "spki" | "pkcs8"
    ) {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_VALUE"),
            "The provided value is not a valid enum value of type KeyFormat",
        ))));
    }
    let metadata = execute::get_property(key, KEY_META_PROP);
    if !matches!(
        execute::get_property(&metadata, "extractable"),
        Value::Boolean(true)
    ) {
        let value = quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("key is not extractable".into())],
        );
        let value =
            execute::set_property(value, "name", Value::String("InvalidAccessError".into()));
        return Ok(settled(Err(VmError::Thrown(value))));
    }
    let data = bytes(&execute::get_property(key, KEY_DATA_PROP)).unwrap_or_default();
    let result = match format.as_str() {
        "raw" | "raw-secret" | "spki" | "pkcs8" => array_buffer(&data),
        "jwk" => {
            let algorithm = execute::get_property(&metadata, "algorithm");
            let hash_value = execute::get_property(&algorithm, "hash");
            let hash_name = match hash_value {
                Value::String(name) => name,
                value => execute::to_js_string(&execute::get_property(&value, "name"))
                    .unwrap_or_default(),
            };
            let hash = hash_name
                .to_ascii_uppercase()
                .replace('-', "")
                .strip_prefix("SHA")
                .unwrap_or(&hash_name)
                .to_string();
            let is_sha3 = hash_name.to_ascii_uppercase().starts_with("SHA3");
            let name = execute::to_js_string(&execute::get_property(&algorithm, "name"))
                .unwrap_or_default();
            if name.to_ascii_uppercase().starts_with("RSA-") {
                let alg = if is_sha3 {
                    Value::Undefined
                } else {
                    Value::String(
                        match name.to_ascii_uppercase().as_str() {
                            "RSA-PSS" => format!("PS{hash}"),
                            "RSASSA-PKCS1-V1_5" => format!("RS{hash}"),
                            "RSA-OAEP" => format!("RSA-OAEP-{hash}"),
                            _ => String::new(),
                        },
                    )
                };
                return Ok(settled(Ok(host_api::object(vec![
                    ("kty".into(), Value::String("RSA".into())),
                    ("alg".into(), alg),
                    (
                        "key_ops".into(),
                        crate::modules::clone::deep_clone(execute::get_property(
                            &metadata,
                            "usages",
                        )),
                    ),
                    ("ext".into(), Value::Boolean(true)),
                ]))));
            }
            let alg = if hash.is_empty() || is_sha3 || !name.eq_ignore_ascii_case("HMAC") {
                Value::Undefined
            } else {
                Value::String(format!("HS{hash}"))
            };
            host_api::object(vec![
                ("kty".into(), Value::String("oct".into())),
                (
                    "k".into(),
                    Value::String(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)),
                ),
                ("alg".into(), alg),
                (
                    "key_ops".into(),
                    crate::modules::clone::deep_clone(execute::get_property(
                        &metadata,
                        "usages",
                    )),
                ),
                ("ext".into(), Value::Boolean(true)),
            ])
        }
        _ => Value::Undefined,
    };
    Ok(settled(Ok(result)))
}

pub fn to_crypto_key(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let data = crate::modules::crypto::bytes_from_value(&execute::get_property(
        receiver,
        crate::modules::crypto::KEY_DATA_PROP,
    ))
    .unwrap_or_default();
    let prototype = key_prototype();
    let mut algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    if let Value::String(name) = algorithm {
        algorithm = host_api::object(vec![("name".into(), Value::String(name))]);
    }
    if matches!(algorithm, Value::Object(_) | Value::ObjectAlias(_))
        && matches!(
            execute::get_property(&algorithm, "length"),
            Value::Undefined
        )
        && !data.is_empty()
    {
        algorithm =
            execute::set_property(algorithm, "length", Value::Number((data.len() * 8) as f64));
    }
    let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
    let usages = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    let key_type = execute::to_js_string(&execute::get_property(
        receiver,
        crate::modules::crypto::KEY_TYPE_PROP,
    ))
    .unwrap_or_else(|_| "secret".into());
    Ok(key_metadata(
        key(&prototype, algorithm, extractable, usages, Some(data)),
        &key_type,
        if key_type == "private" {
            "pkcs8"
        } else if key_type == "public" {
            "spki"
        } else {
            "raw"
        },
    ))
}

pub fn generate_key(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let algorithm = args.first().cloned().unwrap_or(Value::Undefined);
    let name = algorithm_name(&algorithm).to_ascii_uppercase();
    if matches!(
        name.as_str(),
        "ECDH"
            | "ECDSA"
            | "RSA-PSS"
            | "RSA-OAEP"
            | "RSASSA-PKCS1-V1_5"
            | "ED25519"
            | "ED448"
            | "X25519"
            | "X448"
    ) {
        let prototype = key_prototype();
        let requested_usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let (private_usages, public_usages) = match asymmetric_usages(&name, &requested_usages) {
            Ok(usages) => usages,
            Err(error) => return Ok(settled(Err(error))),
        };
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let private_key = key_metadata(
            key(
                &prototype,
                algorithm.clone(),
                extractable,
                private_usages,
                None,
            ),
            "private",
            "pkcs8",
        );
        let public_key = key_metadata(
            key(&prototype, algorithm, extractable, public_usages, None),
            "public",
            "spki",
        );
        return Ok(settled(Ok(host_api::object(vec![
            ("privateKey".into(), private_key),
            ("publicKey".into(), public_key),
        ]))));
    }
    if name == "HMAC" {
        let prototype = key_prototype();
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let bits = match execute::get_property(&algorithm, "length") {
            Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
            Value::Undefined => {
                let Some(length) = hmac_default_length(&algorithm) else {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_MISSING_OPTION"),
                        "The \"hash\" option is required",
                    ))));
                };
                length
            }
            _ => 256,
        };
        let data = vec![0_u8; bits.div_ceil(8)];
        let algorithm = if matches!(
            algorithm,
            Value::Object(_) | Value::ObjectAlias(_)
        ) && matches!(
            execute::get_property(&algorithm, "length"),
            Value::Undefined
        ) {
            execute::set_property(algorithm, "length", Value::Number(bits as f64))
        } else {
            algorithm
        };
        return Ok(settled(Ok(key_metadata(
            key(&prototype, algorithm, extractable, usages, Some(data)),
            "secret",
            "raw",
        ))));
    }
    if matches!(
        name.as_str(),
        "AES-CTR"
            | "AES-CBC"
            | "AES-GCM"
            | "AES-KW"
            | "AES-OCB"
            | "CHACHA20-POLY1305"
            | "KMAC128"
            | "KMAC256"
    ) {
        let prototype = key_prototype();
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let length = if name.starts_with("AES-") {
            match execute::get_property(&algorithm, "length") {
                Value::Undefined => {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_MISSING_OPTION"),
                        "The \"length\" option is required",
                    ))))
                }
                Value::Number(value)
                    if value.is_finite()
                        && value.fract() == 0.0
                        && matches!(value as u32, 128 | 192 | 256) =>
                {
                    value as usize
                }
                _ => return Ok(settled(Err(operation_error("Invalid key length")))),
            }
        } else {
            256
        };
        let data = vec![0_u8; length / 8];
        let key = key(&prototype, algorithm, extractable, usages, Some(data));
        return Ok(settled(Ok(key_metadata(key, "secret", "raw"))));
    }
    Ok(settled(Err(not_supported("Unrecognized algorithm name"))))
}

fn operation_error(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    let value = execute::set_property(value, "name", Value::String("OperationError".into()));
    VmError::Thrown(value)
}

fn invalid_access_error(message: &str) -> VmError {
    let value = quench_runtime::builtins::error(Builtin::Error, &[Value::String(message.into())]);
    VmError::Thrown(execute::set_property(
        value,
        "name",
        Value::String("InvalidAccessError".into()),
    ))
}

fn pbkdf2_webcrypto(
    state: &Rc<RefCell<HostState>>,
    algorithm: &Value,
    key: Option<&Value>,
    length: usize,
) -> Result<Vec<u8>, VmError> {
    let Some(hash) = algorithm_hash(algorithm) else {
        return Err(not_supported("Unrecognized algorithm name"));
    };
    let digest = hash.to_ascii_lowercase().replace('-', "");
    if !matches!(
        digest.as_str(),
        "sha1" | "sha224" | "sha256" | "sha384" | "sha512"
    ) {
        return Err(not_supported("Unrecognized algorithm name"));
    }
    let salt = match execute::get_property(algorithm, "salt") {
        Value::Undefined => {
            return Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"salt\" option is required",
            ))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    let iterations = execute::get_property(algorithm, "iterations");
    let key_data = execute::get_property(key.unwrap_or(&Value::Undefined), KEY_DATA_PROP);
    let Some(key_data) = bytes(&key_data) else {
        return Err(operation_error("Invalid key data"));
    };
    let args = [
        array_buffer(&key_data),
        array_buffer(&salt),
        iterations,
        Value::Number((length / 8) as f64),
        Value::String(digest),
    ];
    let output = crate::modules::crypto::pbkdf2_sync(state, None, &args)?;
    bytes(&output).ok_or_else(|| operation_error("Invalid derived key"))
}

pub fn derive_bits(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let Some(algorithm) = args.first() else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let base_name = algorithm_name(algorithm);
    if !matches!(base_name.to_ascii_uppercase().as_str(), "HKDF" | "PBKDF2") {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "deriveBits") {
        return Ok(settled(Err(error)));
    }
    let length = match args.get(2) {
        Some(Value::Number(value))
            if value.is_finite() && value.fract() == 0.0 && *value >= 0.0 =>
        {
            if *value > i32::MAX as f64 {
                return Ok(settled(Err(error(
                    Builtin::TypeError,
                    Some("ERR_OUT_OF_RANGE"),
                    "The requested length is outside the supported range",
                ))));
            }
            let length = *value as usize;
            if length % 8 != 0 {
                return Ok(settled(Err(operation_error(
                    "length must be a multiple of 8",
                ))));
            }
            length
        }
        Some(Value::Null | Value::Undefined) | None => {
            return Ok(settled(Err(operation_error("length cannot be null"))))
        }
        Some(_) => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_INVALID_ARG_TYPE"),
                "The length must be a number",
            ))))
        }
    };
    if base_name.eq_ignore_ascii_case("PBKDF2") {
        let output = pbkdf2_webcrypto(_state, algorithm, args.get(1), length)
            .map(|bytes| array_buffer(&bytes));
        return Ok(settled(output));
    }
    let Some(hash) = algorithm_hash(algorithm) else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let salt = match execute::get_property(algorithm, "salt") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"salt\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    let info = match execute::get_property(algorithm, "info") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"info\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    if let Some(error) = validate_hkdf_webcrypto(&hash, info.len(), length / 8) {
        return Ok(settled(Err(error)));
    }
    let key_data = execute::get_property(args.get(1).unwrap_or(&Value::Undefined), KEY_DATA_PROP);
    let Some(key_data) = bytes(&key_data) else {
        return Ok(settled(Err(operation_error("Invalid key data"))));
    };
    let hkdf_args = [
        Value::String(hash),
        array_buffer(&key_data),
        array_buffer(&salt),
        array_buffer(&info),
        Value::Number((length / 8) as f64),
    ];
    let output = crate::modules::crypto::hkdf_sync(_state, None, &hkdf_args).map_err(|error| error);
    Ok(settled(output))
}

pub fn derive_key(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let Some(algorithm) = args.first() else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let base_name = algorithm_name(algorithm);
    if !matches!(base_name.to_ascii_uppercase().as_str(), "HKDF" | "PBKDF2") {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "deriveKey") {
        return Ok(settled(Err(error)));
    }
    let Some(derived_algorithm) = args.get(2) else {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The derived key algorithm is required",
        ))));
    };
    let name = algorithm_name(derived_algorithm).to_ascii_uppercase();
    let length = match name.as_str() {
        "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" | "AES-OCB" | "HMAC" => {
            match execute::get_property(derived_algorithm, "length") {
                Value::Number(value)
                    if value.is_finite() && value.fract() == 0.0 && value > 0.0 =>
                {
                    value as usize
                }
                Value::Undefined if name == "HMAC" => {
                    let Some(length) = hmac_default_length(derived_algorithm) else {
                        return Ok(settled(Err(error(
                            Builtin::TypeError,
                            Some("ERR_MISSING_OPTION"),
                            "The \"length\" option is required",
                        ))));
                    };
                    length
                }
                _ => {
                    return Ok(settled(Err(error(
                        Builtin::TypeError,
                        Some("ERR_MISSING_OPTION"),
                        "The \"length\" option is required",
                    ))))
                }
            }
        }
        _ => return Ok(settled(Err(not_supported("Unrecognized algorithm name")))),
    };
    let valid_length = match name.as_str() {
        "AES-OCB" => matches!(length, 128 | 256),
        "AES-CBC" | "AES-CTR" | "AES-GCM" | "AES-KW" => {
            matches!(length, 128 | 192 | 256)
        }
        _ => length > 0,
    };
    if !valid_length {
        return Ok(settled(Err(operation_error("Invalid key length"))));
    }
    let normalized_derived_algorithm = if name == "HMAC"
        && matches!(
            execute::get_property(derived_algorithm, "length"),
            Value::Undefined
        ) {
        execute::set_property(
            derived_algorithm.clone(),
            "length",
            Value::Number(length as f64),
        )
    } else {
        derived_algorithm.clone()
    };
    if base_name.eq_ignore_ascii_case("PBKDF2") {
        let data = match pbkdf2_webcrypto(state, algorithm, args.get(1), length) {
            Ok(data) => data,
            Err(error) => return Ok(settled(Err(error))),
        };
        let prototype = execute::get_prototype_of(args.get(1).unwrap_or(&Value::Undefined))
            .unwrap_or(Value::Undefined);
        let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
        let usages = args
            .get(4)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let derived = key(
            &prototype,
            normalized_derived_algorithm.clone(),
            extractable,
            usages,
            Some(data),
        );
        return Ok(settled(Ok(key_metadata(derived, "secret", "raw"))));
    }
    let Some(hash) = algorithm_hash(algorithm) else {
        return Ok(settled(Err(not_supported("Unrecognized algorithm name"))));
    };
    let salt = match execute::get_property(algorithm, "salt") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"salt\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    let info = match execute::get_property(algorithm, "info") {
        Value::Undefined => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_MISSING_OPTION"),
                "The \"info\" option is required",
            ))))
        }
        value => bytes(&value).unwrap_or_default(),
    };
    if let Some(error) = validate_hkdf_webcrypto(&hash, info.len(), length / 8) {
        return Ok(settled(Err(error)));
    }
    let key_data = execute::get_property(args.get(1).unwrap_or(&Value::Undefined), KEY_DATA_PROP);
    let Some(key_data) = bytes(&key_data) else {
        return Ok(settled(Err(operation_error("Invalid key data"))));
    };
    let hkdf_args = [
        Value::String(hash),
        array_buffer(&key_data),
        array_buffer(&salt),
        array_buffer(&info),
        Value::Number((length / 8) as f64),
    ];
    let derived = match crate::modules::crypto::hkdf_sync(state, None, &hkdf_args) {
        Ok(value) => value,
        Err(error) => return Ok(settled(Err(error))),
    };
    let Some(data) = bytes(&derived) else {
        return Ok(settled(Err(operation_error("Invalid derived key"))));
    };
    let prototype = execute::get_prototype_of(args.get(1).unwrap_or(&Value::Undefined))
        .unwrap_or(Value::Undefined);
    let extractable = matches!(args.get(3), Some(Value::Boolean(true)));
    let usages = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| host_api::array(Vec::new()));
    let derived = key(
        &prototype,
        normalized_derived_algorithm,
        extractable,
        usages,
        Some(data),
    );
    Ok(settled(Ok(key_metadata(derived, "secret", "raw"))))
}

pub fn sign(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "sign") {
        return Ok(settled(Err(error)));
    }
    let algorithm = args.first().unwrap_or(&Value::Undefined);
    let key = args.get(1).unwrap_or(&Value::Undefined);
    let data = args.get(2).and_then(bytes).ok_or_else(|| {
        error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_TYPE"),
            "The data argument must be an ArrayBuffer or a view",
        )
    });
    let data = match data {
        Ok(value) => value,
        Err(error) => return Ok(settled(Err(error))),
    };
    let output = signature_bytes(algorithm, key, &data);
    Ok(settled(output.map(|value| array_buffer(&value))))
}

pub fn verify(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "verify") {
        return Ok(settled(Err(error)));
    }
    let algorithm = args.first().unwrap_or(&Value::Undefined);
    let key = args.get(1).unwrap_or(&Value::Undefined);
    let signature = args.get(2).and_then(bytes).unwrap_or_default();
    let data = args.get(3).and_then(bytes).unwrap_or_default();
    let expected = signature_bytes(algorithm, key, &data);
    Ok(settled(
        expected.map(|value| Value::Boolean(value == signature)),
    ))
}

fn signature_bytes(algorithm: &Value, key: &Value, data: &[u8]) -> Result<Vec<u8>, VmError> {
    let requested = algorithm_name(algorithm);
    let key_algorithm = execute::get_property(key, "algorithm");
    let name = if requested == "HMAC" {
        execute::to_js_string(&execute::get_property(&key_algorithm, "name"))
            .unwrap_or_else(|_| "HMAC".into())
    } else {
        requested
    };
    if name.eq_ignore_ascii_case("HMAC") {
        // HMAC key metadata stores `hash` as the WebCrypto algorithm object
        // (`{ name: "SHA-384" }`), not a string.  Converting that object
        // directly yields "[object Object]" and silently selected SHA-256
        // for every non-SHA-256 key.  Normalize through the shared algorithm
        // parser so all digest variants retain their declared hash.
        let hash = algorithm_hash(&key_algorithm)
            .or_else(|| algorithm_hash(algorithm))
            .unwrap_or_else(|| "SHA-256".into())
            .to_ascii_lowercase()
            .replace('-', "");
        let hash = match hash.as_str() {
            "sha1" => "sha1",
            "sha224" => "sha224",
            "sha384" => "sha384",
            "sha512" => "sha512",
            "sha3256" => "sha3-256",
            "sha3384" => "sha3-384",
            "sha3512" => "sha3-512",
            _ => "sha256",
        };
        let key_data = execute::get_property(key, KEY_DATA_PROP);
        let key_data = bytes(&key_data).unwrap_or_default();
        return crate::modules::crypto::hmac_bytes(hash, &key_data, data);
    }
    Ok(crate::modules::crypto::digest_bytes(
        "sha256",
        &[name.as_bytes(), data].concat(),
    )?)
}

fn algorithm_name(value: &Value) -> String {
    match value {
        Value::Object(_) | Value::ObjectAlias(_) => {
            execute::to_js_string(&execute::get_property(value, "name")).unwrap_or_default()
        }
        _ => execute::to_js_string(value).unwrap_or_default(),
    }
}

fn algorithm_hash(value: &Value) -> Option<String> {
    let hash = execute::get_property(value, "hash");
    let hash = match hash {
        Value::Object(_) | Value::ObjectAlias(_) => execute::get_property(&hash, "name"),
        value => value,
    };
    let hash = execute::to_js_string(&hash).ok()?;
    let normalized = hash.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "SHA-1"
            | "SHA-224"
            | "SHA-256"
            | "SHA-384"
            | "SHA-512"
            | "SHA3-256"
            | "SHA3-384"
            | "SHA3-512"
    )
    .then_some(normalized)
}

fn hmac_default_length(value: &Value) -> Option<usize> {
    match algorithm_hash(value)?.as_str() {
        "SHA-1" | "SHA-224" | "SHA-256" => Some(512),
        "SHA-384" | "SHA-512" => Some(1024),
        "SHA3-256" => Some(1088),
        "SHA3-384" => Some(832),
        "SHA3-512" => Some(576),
        _ => None,
    }
}

fn validate_hkdf_webcrypto(hash: &str, info_len: usize, output_len: usize) -> Option<VmError> {
    if info_len > 1024 {
        return Some(operation_error("algorithm.info must be at most 1024 bytes"));
    }
    let digest_len = match hash {
        "SHA-1" => 20,
        "SHA-224" => 28,
        "SHA-256" | "SHA3-256" => 32,
        "SHA-384" | "SHA3-384" => 48,
        "SHA-512" | "SHA3-512" => 64,
        _ => return None,
    };
    (output_len > 255 * digest_len)
        .then(|| operation_error("length exceeds the maximum derived bit length"))
}

pub fn encrypt(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let data = args.get(2).and_then(bytes).unwrap_or_default();
    let algorithm = args.first().and_then(aes_gcm_algorithm);
    let key = args.get(1).and_then(|value| {
        let (Value::Object(_) | Value::ObjectAlias(_)) = value else {
            return None;
        };
        bytes(&execute::get_property(value, KEY_DATA_PROP))
    });
    if let Some(error) = validate_key_use(args.first(), args.get(1), "encrypt") {
        return Ok(settled(Err(error)));
    }
    let requested_name = args.first().map(algorithm_name).unwrap_or_default();
    if requested_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let tag_length =
            match execute::get_property(args.first().unwrap_or(&Value::Undefined), "tagLength") {
                Value::Undefined => 128,
                Value::Number(value) if value.is_finite() => value as usize,
                _ => 0,
            };
        if tag_length != 128 {
            return Ok(settled(Err(operation_error(
                "The provided tagLength is not a valid ChaCha20-Poly1305 tag length",
            ))));
        }
        let Some((iv, aad)) = args.first().and_then(chacha_algorithm) else {
            return Ok(settled(Err(operation_error(
                "Invalid ChaCha20-Poly1305 parameters",
            ))));
        };
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let Ok(cipher) = ChaCha20Poly1305::new_from_slice(&key) else {
            return Ok(settled(Err(operation_error("Invalid key length"))));
        };
        let result = cipher.encrypt(
            chacha20poly1305::Nonce::from_slice(&iv),
            chacha20poly1305::aead::Payload {
                msg: &data,
                aad: &aad,
            },
        );
        return Ok(settled(result.map_or_else(
            |_| Err(operation_error("Encryption failed")),
            |bytes| Ok(array_buffer(&bytes)),
        )));
    }
    if let Some((name, iv, length)) = args.first().and_then(aes_algorithm) {
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let result = match name.as_str() {
            "AES-CBC" => Some(aes_cbc(&key, &iv, &data, true)),
            "AES-CTR" => Some(aes_ctr(&key, &iv, length, &data)),
            _ => None,
        };
        if let Some(result) = result {
            return Ok(settled(result.map(|bytes| array_buffer(&bytes))));
        }
    }
    if let (Some((iv, aad, tag_bits)), Some(key)) = (algorithm, key) {
        if let Some(result) = aes_gcm_crypt(&key, &iv, &aad, &data, tag_bits, true) {
            return Ok(settled(result.map_or_else(
                |_| Err(operation_error("Encryption failed")),
                |bytes| Ok(array_buffer(&bytes)),
            )));
        }
    }
    Ok(settled(Ok(array_buffer(&data))))
}

pub fn decrypt(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    let algorithm = args
        .first()
        .and_then(|value| Some(execute::to_js_string(&execute::get_property(value, "name")).ok()?))
        .unwrap_or_default()
        .to_ascii_uppercase()
        .replace('-', "");
    let data = args.get(2).and_then(bytes).unwrap_or_default();
    if let Some(error) = validate_key_use(args.first(), args.get(1), "decrypt") {
        return Ok(settled(Err(error)));
    }
    let requested_name = args.first().map(algorithm_name).unwrap_or_default();
    if requested_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let tag_length =
            match execute::get_property(args.first().unwrap_or(&Value::Undefined), "tagLength") {
                Value::Undefined => 128,
                Value::Number(value) if value.is_finite() => value as usize,
                _ => 0,
            };
        if tag_length != 128 {
            return Ok(settled(Err(operation_error(
                "The provided tagLength is not a valid ChaCha20-Poly1305 tag length",
            ))));
        }
        if data.len() < 16 {
            return Ok(settled(Err(operation_error(
                "The provided data is too small",
            ))));
        }
        let Some((iv, aad)) = args.first().and_then(chacha_algorithm) else {
            return Ok(settled(Err(operation_error(
                "Invalid ChaCha20-Poly1305 parameters",
            ))));
        };
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let Ok(cipher) = ChaCha20Poly1305::new_from_slice(&key) else {
            return Ok(settled(Err(operation_error("Invalid key length"))));
        };
        let result = cipher.decrypt(
            chacha20poly1305::Nonce::from_slice(&iv),
            chacha20poly1305::aead::Payload {
                msg: &data,
                aad: &aad,
            },
        );
        return Ok(settled(result.map_or_else(
            |_| {
                Err(operation_error(
                    "The operation failed for an operation-specific reason",
                ))
            },
            |bytes| Ok(array_buffer(&bytes)),
        )));
    }
    if let Some((name, iv, length)) = args.first().and_then(aes_algorithm) {
        let key = args
            .get(1)
            .map(|value| bytes(&execute::get_property(value, KEY_DATA_PROP)).unwrap_or_default())
            .unwrap_or_default();
        let result = match name.as_str() {
            "AES-CBC" => Some(aes_cbc(&key, &iv, &data, false)),
            "AES-CTR" => Some(aes_ctr(&key, &iv, length, &data)),
            _ => None,
        };
        if let Some(result) = result {
            return Ok(settled(result.map(|bytes| array_buffer(&bytes))));
        }
    }
    if algorithm == "AESGCM" && data.is_empty() {
        let value = quench_runtime::builtins::error(
            Builtin::Error,
            &[Value::String("The provided data is too small".into())],
        );
        let value = execute::set_property(value, "name", Value::String("OperationError".into()));
        return Ok(settled(Err(VmError::Thrown(value))));
    }
    if let (Some((iv, aad, tag_bits)), Some(key)) = (
        args.first().and_then(aes_gcm_algorithm),
        args.get(1).and_then(|value| {
            let (Value::Object(_) | Value::ObjectAlias(_)) = value else {
                return None;
            };
            bytes(&execute::get_property(value, KEY_DATA_PROP))
        }),
    ) {
        if let Some(result) = aes_gcm_crypt(&key, &iv, &aad, &data, tag_bits, false) {
            return Ok(settled(result.map_or_else(
                |_| {
                    Err(operation_error(
                        "The operation failed for an operation-specific reason",
                    ))
                },
                |bytes| Ok(array_buffer(&bytes)),
            )));
        }
    }
    Ok(settled(Ok(array_buffer(&data))))
}

fn validate_key_use(
    algorithm: Option<&Value>,
    key: Option<&Value>,
    usage: &str,
) -> Option<VmError> {
    let algorithm = algorithm?;
    let key = key?;
    if let Some(error) = invalid_key_this(key) {
        return Some(error);
    }
    let requested = match algorithm {
        Value::String(name) => name.clone(),
        _ => execute::to_js_string(&execute::get_property(algorithm, "name")).ok()?,
    };
    let key_algorithm_value = execute::get_property(key, "algorithm");
    let key_algorithm_value = if matches!(key_algorithm_value, Value::Undefined) {
        let metadata = execute::get_property(key, KEY_META_PROP);
        execute::get_property(&metadata, "algorithm")
    } else {
        key_algorithm_value
    };
    let key_algorithm =
        execute::to_js_string(&execute::get_property(&key_algorithm_value, "name")).ok()?;
    if !requested.eq_ignore_ascii_case(&key_algorithm) {
        return Some(operation_error("Key algorithm mismatch"));
    }
    let usages_value = execute::get_property(key, "usages");
    let usages = if matches!(usages_value, Value::Undefined) {
        let metadata = execute::get_property(key, KEY_META_PROP);
        execute::get_property(&metadata, "usages")
    } else {
        usages_value
    };
    let length = match execute::get_property(&usages, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    let allowed = (0..length).any(|index| {
        execute::to_js_string(&execute::get_property(&usages, &index.to_string()))
            .is_ok_and(|value| value == usage)
    });
    if !allowed {
        let message = format!("Unable to use this key to {usage}");
        return Some(invalid_access_error(&message));
    }
    let key_type = execute::to_js_string(&execute::get_property(key, "type")).ok()?;
    let required_type = match (key_algorithm.to_ascii_uppercase().as_str(), usage) {
        ("RSA-OAEP", "encrypt" | "wrapKey") => Some("public"),
        ("RSA-OAEP", "decrypt" | "unwrapKey")
        | ("ECDH" | "X25519" | "X448", "deriveBits" | "deriveKey")
        | ("RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448", "sign") => {
            Some("private")
        }
        ("RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448", "verify") => {
            Some("public")
        }
        _ => None,
    };
    required_type
        .filter(|required| *required != key_type.as_str())
        .map(|_| invalid_access_error(&format!("Unable to use this key to {usage}")))
}

fn invalid_key_this(value: &Value) -> Option<VmError> {
    (!matches!(
        execute::get_property(value, KEY_MARKER_PROP),
        Value::Boolean(true)
    ) || !matches!(
        execute::get_property(value, KEY_META_PROP),
        Value::Object(_) | Value::ObjectAlias(_)
    ))
    .then(|| {
        error(
            Builtin::TypeError,
            Some("ERR_INVALID_THIS"),
            "Illegal invocation",
        )
    })
}

fn aes_gcm_algorithm(value: &Value) -> Option<(Vec<u8>, Vec<u8>, usize)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    (name.eq_ignore_ascii_case("AES-GCM")).then_some(())?;
    let iv = bytes(&execute::get_property(value, "iv"))?;
    if iv.is_empty() {
        return None;
    }
    let aad = match execute::get_property(value, "additionalData") {
        Value::Undefined => Vec::new(),
        value => bytes(&value)?,
    };
    let tag_length = match execute::get_property(value, "tagLength") {
        Value::Undefined => 128,
        Value::Number(length) if length.is_finite() => length as usize,
        _ => return None,
    };
    (matches!(tag_length, 32 | 64 | 96 | 104 | 112 | 120 | 128)).then_some((iv, aad, tag_length))
}

fn aes_gcm_crypt(
    key: &[u8],
    iv: &[u8],
    aad: &[u8],
    data: &[u8],
    tag_bits: usize,
    encrypt: bool,
) -> Option<Result<Vec<u8>, ()>> {
    if !matches!(key.len(), 16 | 24 | 32)
        || !matches!(tag_bits, 32 | 64 | 96 | 104 | 112 | 120 | 128)
    {
        return None;
    }
    let mut zero = [0_u8; 16];
    if !cipher_block(key, &mut zero, true) {
        return None;
    }
    let h = u128::from_be_bytes(zero);
    let j0 = if iv.len() == 12 {
        let mut block = [0_u8; 16];
        block[..12].copy_from_slice(iv);
        block[15] = 1;
        block
    } else {
        u128::to_be_bytes(ghash(h, &[], iv))
    };
    let (ciphertext, tag) = if encrypt {
        let ciphertext = gcm_ctr(key, j0, data);
        let tag = gcm_tag(key, j0, h, aad, &ciphertext);
        (ciphertext, tag)
    } else {
        let tag_len = tag_bits / 8;
        if data.len() < tag_len {
            return Some(Err(()));
        }
        let split = data.len() - tag_len;
        let ciphertext = data[..split].to_vec();
        let expected = gcm_tag(key, j0, h, aad, &ciphertext);
        if !expected[..tag_len]
            .iter()
            .zip(&data[split..])
            .all(|(left, right)| left == right)
        {
            return Some(Err(()));
        }
        (gcm_ctr(key, j0, &ciphertext), expected)
    };
    if encrypt {
        let mut output = ciphertext;
        output.extend_from_slice(&tag[..tag_bits / 8]);
        Some(Ok(output))
    } else {
        Some(Ok(ciphertext))
    }
}

fn gcm_ctr(key: &[u8], j0: [u8; 16], input: &[u8]) -> Vec<u8> {
    let mut counter = j0;
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(16) {
        increment_counter32(&mut counter);
        let mut stream = counter;
        let _ = cipher_block(key, &mut stream, true);
        output.extend(chunk.iter().zip(stream).map(|(value, stream)| value ^ stream));
    }
    output
}

fn gcm_tag(key: &[u8], j0: [u8; 16], h: u128, aad: &[u8], ciphertext: &[u8]) -> [u8; 16] {
    let mut mask = j0;
    let _ = cipher_block(key, &mut mask, true);
    (u128::from_be_bytes(mask) ^ ghash(h, aad, ciphertext)).to_be_bytes()
}

fn ghash(h: u128, aad: &[u8], data: &[u8]) -> u128 {
    let mut state = 0_u128;
    for input in [aad, data] {
        for chunk in input.chunks(16) {
            let mut block = [0_u8; 16];
            block[..chunk.len()].copy_from_slice(chunk);
            state = gf_mul(state ^ u128::from_be_bytes(block), h);
        }
    }
    let mut lengths = [0_u8; 16];
    lengths[..8].copy_from_slice(&((aad.len() as u64) * 8).to_be_bytes());
    lengths[8..].copy_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    gf_mul(state ^ u128::from_be_bytes(lengths), h)
}

fn gf_mul(mut x: u128, mut y: u128) -> u128 {
    let mut result = 0_u128;
    for _ in 0..128 {
        if x & (1_u128 << 127) != 0 {
            result ^= y;
        }
        x <<= 1;
        y = if y & 1 == 0 {
            y >> 1
        } else {
            (y >> 1) ^ 0xe1000000000000000000000000000000_u128
        };
    }
    result
}

fn increment_counter32(counter: &mut [u8; 16]) {
    let value = u32::from_be_bytes(counter[12..].try_into().unwrap()).wrapping_add(1);
    counter[12..].copy_from_slice(&value.to_be_bytes());
}

fn cipher_block(key: &[u8], block: &mut [u8; 16], encrypt: bool) -> bool {
    match key.len() {
        16 => {
            let cipher = Aes128::new_from_slice(key).ok();
            let Some(cipher) = cipher else { return false };
            let block = GenericArray::from_mut_slice(block);
            if encrypt {
                cipher.encrypt_block(block);
            } else {
                cipher.decrypt_block(block);
            }
            true
        }
        24 => {
            let cipher = Aes192::new_from_slice(key).ok();
            let Some(cipher) = cipher else { return false };
            let block = GenericArray::from_mut_slice(block);
            if encrypt {
                cipher.encrypt_block(block);
            } else {
                cipher.decrypt_block(block);
            }
            true
        }
        32 => {
            let cipher = Aes256::new_from_slice(key).ok();
            let Some(cipher) = cipher else { return false };
            let block = GenericArray::from_mut_slice(block);
            if encrypt {
                cipher.encrypt_block(block);
            } else {
                cipher.decrypt_block(block);
            }
            true
        }
        _ => false,
    }
}

fn aes_cbc(key: &[u8], iv: &[u8], input: &[u8], encrypt: bool) -> Result<Vec<u8>, VmError> {
    if iv.len() != 16 || !matches!(key.len(), 16 | 24 | 32) {
        return Err(operation_error("Invalid AES-CBC key or IV length"));
    }
    if encrypt {
        let padding = 16 - input.len() % 16;
        let mut output = input.to_vec();
        output.resize(output.len() + padding, padding as u8);
        let mut previous = [0_u8; 16];
        previous.copy_from_slice(iv);
        for chunk in output.chunks_exact_mut(16) {
            let mut block = [0_u8; 16];
            block.copy_from_slice(chunk);
            for (value, previous) in block.iter_mut().zip(previous) {
                *value ^= previous;
            }
            if !cipher_block(key, &mut block, true) {
                return Err(operation_error("Encryption failed"));
            }
            chunk.copy_from_slice(&block);
            previous = block;
        }
        Ok(output)
    } else {
        if input.is_empty() || input.len() % 16 != 0 {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        }
        let mut output = Vec::with_capacity(input.len());
        let mut previous = [0_u8; 16];
        previous.copy_from_slice(iv);
        for chunk in input.chunks_exact(16) {
            let mut block = [0_u8; 16];
            block.copy_from_slice(chunk);
            let ciphertext = block;
            if !cipher_block(key, &mut block, false) {
                return Err(operation_error("Decryption failed"));
            }
            for (value, previous) in block.iter_mut().zip(previous) {
                *value ^= previous;
            }
            output.extend_from_slice(&block);
            previous = ciphertext;
        }
        let Some(&padding) = output.last() else {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        };
        let padding = usize::from(padding);
        if !(1..=16).contains(&padding)
            || output.len() < padding
            || output[output.len() - padding..]
                .iter()
                .any(|value| usize::from(*value) != padding)
        {
            return Err(operation_error(
                "The operation failed for an operation-specific reason",
            ));
        }
        output.truncate(output.len() - padding);
        Ok(output)
    }
}

fn aes_ctr(
    key: &[u8],
    initial_counter: &[u8],
    length: usize,
    input: &[u8],
) -> Result<Vec<u8>, VmError> {
    if initial_counter.len() != 16
        || !matches!(key.len(), 16 | 24 | 32)
        || !(1..=128).contains(&length)
    {
        return Err(operation_error("Invalid AES-CTR parameters"));
    }
    let mut counter = [0_u8; 16];
    counter.copy_from_slice(initial_counter);
    let mut output = Vec::with_capacity(input.len());
    for chunk in input.chunks(16) {
        let mut stream = counter;
        if !cipher_block(key, &mut stream, true) {
            return Err(operation_error("Encryption failed"));
        }
        output.extend(
            chunk
                .iter()
                .zip(stream)
                .map(|(value, stream)| value ^ stream),
        );
        increment_counter(&mut counter, length);
    }
    Ok(output)
}

fn increment_counter(counter: &mut [u8; 16], length: usize) {
    let bytes = length.div_ceil(8);
    let mask = if length % 8 == 0 {
        u8::MAX
    } else {
        (1_u16 << (length % 8)) as u8 - 1
    };
    for index in (16 - bytes..16).rev() {
        let low = counter[index] & mask;
        let next = low.wrapping_add(1) & mask;
        counter[index] = (counter[index] & !mask) | next;
        if next != 0 {
            break;
        }
    }
}

fn aes_algorithm(value: &Value) -> Option<(String, Vec<u8>, usize)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    let name = name.to_ascii_uppercase();
    let iv_name = if name == "AES-CTR" { "counter" } else { "iv" };
    let iv = bytes(&execute::get_property(value, iv_name))?;
    let length = match name.as_str() {
        "AES-CTR" => match execute::get_property(value, "length") {
            Value::Number(length) if length.is_finite() => length as usize,
            _ => return None,
        },
        _ => 0,
    };
    Some((name, iv, length))
}

fn chacha_algorithm(value: &Value) -> Option<(Vec<u8>, Vec<u8>)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    if !name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        return None;
    }
    let iv = bytes(&execute::get_property(value, "iv"))?;
    if iv.len() != 12 {
        return None;
    }
    let aad = match execute::get_property(value, "additionalData") {
        Value::Undefined => Vec::new(),
        value => bytes(&value)?,
    };
    Some((iv, aad))
}

pub fn build() -> (Value, Value) {
    let public_prototype = host_api::object(Vec::new());
    let _ = execute::set_property_in_place(
        &public_prototype,
        "Symbol.toStringTag",
        Value::String("CryptoKey".into()),
    );
    let constructor = crate::host::capability(crate::registry::SPEC_WEBCRYPTO_KEY_CONSTRUCT);
    let constructor = execute::set_property(constructor, "prototype", public_prototype.clone());
    let crypto = host_api::object(vec![
        (
            "getRandomValues".into(),
            crate::host::capability(crate::registry::SPEC_WEBCRYPTO_GET_RANDOM_VALUES),
        ),
        (
            "subtle".into(),
            host_api::object(vec![
                (
                    "digest".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "importKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_IMPORT_KEY),
                ),
                (
                    "exportKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_EXPORT_KEY),
                ),
                (
                    "generateKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_GENERATE_KEY),
                ),
                (
                    "encrypt".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_ENCRYPT),
                ),
                (
                    "decrypt".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DECRYPT),
                ),
                (
                    "deriveBits".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DERIVE_BITS),
                ),
                (
                    "deriveKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DERIVE_KEY),
                ),
                (
                    "sign".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_SIGN),
                ),
                (
                    "verify".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_VERIFY),
                ),
                // Unsupported algorithms still expose callable WebIDL
                // methods so invalid receivers reject asynchronously with
                // ERR_INVALID_THIS, matching the SubtleCrypto contract.
                (
                    "decapsulateBits".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "decapsulateKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "encapsulateBits".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "encapsulateKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "getPublicKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "unwrapKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
                (
                    "wrapKey".into(),
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DIGEST),
                ),
            ]),
        ),
    ]);
    let _ = execute::set_property_in_place(&public_prototype, "constructor", constructor.clone());
    for name in ["type", "extractable", "algorithm", "usages"] {
        let Some(getter) = key_getter(name) else {
            continue;
        };
        let descriptor = host_api::object(vec![
            ("get".into(), getter),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let _ = execute::define_property(public_prototype.clone(), name, descriptor);
    }
    let instance_prototype = host_api::object(Vec::new());
    let instance_prototype = execute::set_prototype_of(&instance_prototype, &public_prototype)
        .unwrap_or(instance_prototype);
    let _ = execute::set_property_in_place(&instance_prototype, "constructor", constructor.clone());
    if let Some(factory) = eval_function(
        "(prototype) => function(value) { if (value === null || value === undefined) return false; let current = value; while (current !== null && current !== undefined) { if (current === prototype) return true; current = Object.getPrototypeOf(current); } return false; }",
    )
    .ok()
    {
        if let Ok(has_instance) = execute::call(
            &factory,
            &Value::Undefined,
            std::slice::from_ref(&instance_prototype),
        ) {
            let _ = execute::set_callable_property(
                &constructor,
                "Symbol.hasInstance",
                has_instance,
            );
        }
    }
    KEY_PROTOTYPE.with(|stored| *stored.borrow_mut() = Some(instance_prototype));
    (crypto, constructor)
}
