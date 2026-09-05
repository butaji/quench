//! Rust-owned WebCrypto boundary.
//!
//! The engine owns promises and typed-array storage; this module owns the
//! observable WebCrypto facts that do not require a separate JS runtime.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine;
use aes_gcm::{aead::{Aead, Payload}, Aes128Gcm, Aes256Gcm, KeyInit, Nonce};
use aes::{Aes128, Aes192, Aes256};
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
const KEY_META_PROP: &str = "\0quench:webcrypto:key-meta";

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
        "RSASSA-PKCS1-V1_5" | "RSA-PSS" | "ECDSA" | "ED25519" | "ED448" => {
            (&["sign"], &["verify"])
        }
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
    let source = format!(
        "(name) => function() {{ const value = this[{metadata}][name]; return name === \"usages\" ? Array.from(value) : value; }}"
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
        "raw" | "raw-secret" | "jwk" | "spki" | "pkcs8"
    ) {
        return Ok(settled(Err(error(
            Builtin::TypeError,
            Some("ERR_INVALID_ARG_VALUE"),
            "The provided value is not a valid enum value of type KeyFormat",
        ))));
    }
    let algorithm = args.get(2).cloned().unwrap_or(Value::Undefined);
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
    let prototype = quench_runtime::vm::current_global_object();
    let prototype = execute::get_property(&prototype, "__quench_crypto_key_prototype");
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
    if !matches!(
        execute::get_property(key, "extractable"),
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
            let algorithm = execute::get_property(key, "algorithm");
            let hash = execute::to_js_string(&execute::get_property(&algorithm, "hash"))
                .unwrap_or_default()
                .to_ascii_uppercase()
                .replace('-', "");
            let name = execute::to_js_string(&execute::get_property(&algorithm, "name"))
                .unwrap_or_default();
            let alg = if hash.is_empty() || !name.eq_ignore_ascii_case("HMAC") {
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
                ("key_ops".into(), execute::get_property(key, "usages")),
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
    let prototype = execute::get_property(
        &quench_runtime::vm::current_global_object(),
        "__quench_crypto_key_prototype",
    );
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
    let name = execute::to_js_string(&execute::get_property(&algorithm, "name"))
        .unwrap_or_default()
        .to_ascii_uppercase();
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
        let prototype = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__quench_crypto_key_prototype",
        );
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
        let prototype = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__quench_crypto_key_prototype",
        );
        let extractable = matches!(args.get(1), Some(Value::Boolean(true)));
        let usages = args
            .get(2)
            .cloned()
            .unwrap_or_else(|| host_api::array(Vec::new()));
        let bits = match execute::get_property(&algorithm, "length") {
            Value::Number(value) if value.is_finite() && value > 0.0 => value as usize,
            _ => 256,
        };
        let data = vec![0_u8; bits.div_ceil(8)];
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
        let prototype = execute::get_property(
            &quench_runtime::vm::current_global_object(),
            "__quench_crypto_key_prototype",
        );
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

pub fn derive_bits(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if let Some(result) = invalid_subtle_this(receiver) {
        return Ok(result);
    }
    if let Some(error) = validate_key_use(args.first(), args.get(1), "deriveBits") {
        return Ok(settled(Err(error)));
    }
    let length = match args.get(2) {
        Some(Value::Number(value)) if value.is_finite() && *value >= 0.0 => *value as usize,
        Some(Value::Null) | None => 128,
        Some(_) => {
            return Ok(settled(Err(error(
                Builtin::TypeError,
                Some("ERR_INVALID_ARG_TYPE"),
                "The length must be a number",
            ))))
        }
    };
    Ok(settled(Ok(array_buffer(&vec![0; length.div_ceil(8)]))))
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
        let hash = execute::to_js_string(&execute::get_property(&key_algorithm, "hash"))
            .or_else(|_| execute::to_js_string(&execute::get_property(algorithm, "hash")))
            .unwrap_or_else(|_| "SHA-256".into())
            .to_ascii_lowercase()
            .replace('-', "");
        let hash = match hash.as_str() {
            "sha1" => "sha1",
            "sha384" => "sha384",
            "sha512" => "sha512",
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
        let (Value::Object(_) | Value::ObjectAlias(_)) = value else { return None };
        bytes(&execute::get_property(value, KEY_DATA_PROP))
    });
    if let Some(error) = validate_key_use(args.first(), args.get(1), "encrypt") {
        return Ok(settled(Err(error)));
    }
    let requested_name = args.first().map(algorithm_name).unwrap_or_default();
    if requested_name.eq_ignore_ascii_case("ChaCha20-Poly1305") {
        let tag_length = match execute::get_property(
            args.first().unwrap_or(&Value::Undefined),
            "tagLength",
        ) {
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
            return Ok(settled(Err(operation_error("Invalid ChaCha20-Poly1305 parameters"))));
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
            chacha20poly1305::aead::Payload { msg: &data, aad: &aad },
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
    if let (Some((iv, aad)), Some(key)) = (algorithm, key) {
        let result = match key.len() {
            16 => Aes128Gcm::new_from_slice(&key)
                .expect("validated AES-128 key length")
                .encrypt(Nonce::from_slice(&iv), Payload { msg: &data, aad: &aad }),
            32 => Aes256Gcm::new_from_slice(&key)
                .expect("validated AES-256 key length")
                .encrypt(Nonce::from_slice(&iv), Payload { msg: &data, aad: &aad }),
            _ => return Ok(settled(Ok(array_buffer(&data)))),
        };
        return Ok(settled(result.map_or_else(
            |_| Err(operation_error("Encryption failed")),
            |bytes| Ok(array_buffer(&bytes)),
        )));
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
        let tag_length = match execute::get_property(
            args.first().unwrap_or(&Value::Undefined),
            "tagLength",
        ) {
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
            return Ok(settled(Err(operation_error("The provided data is too small"))));
        }
        let Some((iv, aad)) = args.first().and_then(chacha_algorithm) else {
            return Ok(settled(Err(operation_error("Invalid ChaCha20-Poly1305 parameters"))));
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
            chacha20poly1305::aead::Payload { msg: &data, aad: &aad },
        );
        return Ok(settled(result.map_or_else(
            |_| Err(operation_error("The operation failed for an operation-specific reason")),
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
    if let (Some((iv, aad)), Some(key)) = (
        args.first().and_then(aes_gcm_algorithm),
        args.get(1).and_then(|value| {
            let (Value::Object(_) | Value::ObjectAlias(_)) = value else { return None };
            bytes(&execute::get_property(value, KEY_DATA_PROP))
        }),
    ) {
        let result = match key.len() {
            16 => Aes128Gcm::new_from_slice(&key)
                .expect("validated AES-128 key length")
                .decrypt(Nonce::from_slice(&iv), Payload { msg: &data, aad: &aad }),
            32 => Aes256Gcm::new_from_slice(&key)
                .expect("validated AES-256 key length")
                .decrypt(Nonce::from_slice(&iv), Payload { msg: &data, aad: &aad }),
            _ => return Ok(settled(Ok(array_buffer(&data)))),
        };
        return Ok(settled(result.map_or_else(
            |_| Err(operation_error("The operation failed for an operation-specific reason")),
            |bytes| Ok(array_buffer(&bytes)),
        )));
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
    let requested = match algorithm {
        Value::String(name) => name.clone(),
        _ => execute::to_js_string(&execute::get_property(algorithm, "name")).ok()?,
    };
    let key_algorithm = execute::to_js_string(&execute::get_property(
        &execute::get_property(key, "algorithm"),
        "name",
    ))
    .ok()?;
    if !requested.eq_ignore_ascii_case(&key_algorithm) {
        return Some(operation_error("Key algorithm mismatch"));
    }
    let usages = execute::get_property(key, "usages");
    let length = match execute::get_property(&usages, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    let allowed = (0..length).any(|index| {
        execute::to_js_string(&execute::get_property(&usages, &index.to_string()))
            .is_ok_and(|value| value == usage)
    });
    if !allowed {
        return Some(invalid_access_error(&format!("Unable to use this key to {usage}")));
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

fn aes_gcm_algorithm(value: &Value) -> Option<(Vec<u8>, Vec<u8>)> {
    let name = execute::to_js_string(&execute::get_property(value, "name")).ok()?;
    (name.eq_ignore_ascii_case("AES-GCM")).then_some(())?;
    let iv = bytes(&execute::get_property(value, "iv"))?;
    (iv.len() == 12).then_some(())?;
    let aad = match execute::get_property(value, "additionalData") {
        Value::Undefined => Vec::new(),
        value => bytes(&value)?,
    };
    Some((iv, aad))
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
            return Err(operation_error("The operation failed for an operation-specific reason"));
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
            return Err(operation_error("The operation failed for an operation-specific reason"));
        };
        let padding = usize::from(padding);
        if !(1..=16).contains(&padding)
            || output.len() < padding
            || output[output.len() - padding..]
                .iter()
                .any(|value| usize::from(*value) != padding)
        {
            return Err(operation_error("The operation failed for an operation-specific reason"));
        }
        output.truncate(output.len() - padding);
        Ok(output)
    }
}

fn aes_ctr(key: &[u8], initial_counter: &[u8], length: usize, input: &[u8]) -> Result<Vec<u8>, VmError> {
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
    let prototype = host_api::object(Vec::new());
    let constructor = crate::host::capability(crate::registry::SPEC_WEBCRYPTO_KEY_CONSTRUCT);
    let constructor = execute::set_property(constructor, "prototype", prototype.clone());
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
                    crate::host::capability(crate::registry::SPEC_WEBCRYPTO_DERIVE_BITS),
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
    let _ = execute::set_property_in_place(&prototype, "constructor", constructor.clone());
    for name in ["type", "extractable", "algorithm", "usages"] {
        let Some(getter) = key_getter(name) else {
            continue;
        };
        let descriptor = host_api::object(vec![
            ("get".into(), getter),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ]);
        let _ = execute::define_property(prototype.clone(), name, descriptor);
    }
    (crypto, constructor)
}
