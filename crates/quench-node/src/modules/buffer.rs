//! `Buffer` module — pure Rust Buffer atop Uint8Array semantics.
//!
//! Every Buffer is a `Value::Uint8Array` whose `\0prototype` is a
//! shared `Buffer.prototype` stand-in (itself inheriting from
//! `Uint8Array.prototype`). Static constructors and codecs are pure
//! Rust; no JS shim.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;
use quench_runtime::vm::get_property;

use crate::modules::buffer_enc as enc;

/// Node's `buffer.constants.MAX_LENGTH` on 64-bit platforms (2^53-1).
pub const MAX_LENGTH: f64 = 9_007_199_254_740_991.0;
/// Node's `buffer.constants.MAX_STRING_LENGTH`.
pub const MAX_STRING_LENGTH: f64 = 536_870_888.0;

/// Static capability methods of the `Buffer` constructor.
const STATIC_METHODS: &[(&str, crate::registry::NodeSpec)] = &[
    ("from", crate::registry::SPEC_BUFFER_FROM),
    ("alloc", crate::registry::SPEC_BUFFER_ALLOC),
    ("allocUnsafe", crate::registry::SPEC_BUFFER_ALLOC_UNSAFE),
    (
        "allocUnsafeSlow",
        crate::registry::SPEC_BUFFER_ALLOC_UNSAFE_SLOW,
    ),
    ("byteLength", crate::registry::SPEC_BUFFER_BYTELENGTH),
    ("isBuffer", crate::registry::SPEC_BUFFER_ISBUFFER),
    ("isEncoding", crate::registry::SPEC_BUFFER_ISENCODING),
    ("isUtf8", crate::registry::SPEC_BUFFER_ISUTF8),
    ("isAscii", crate::registry::SPEC_BUFFER_ISASCII),
    ("compare", crate::registry::SPEC_BUFFER_COMPARE_STATIC),
    ("concat", crate::registry::SPEC_BUFFER_CONCAT),
    (
        "copyBytesFrom",
        crate::registry::SPEC_BUFFER_COPY_BYTES_FROM,
    ),
];

/// The non-method static properties of the `Buffer` constructor.
fn static_pairs() -> Vec<(String, Value)> {
    let mut pairs: Vec<(String, Value)> = STATIC_METHODS
        .iter()
        .map(|(name, spec)| (name.to_string(), crate::host::capability(*spec)))
        .collect();
    // Buffer.of shares the same host capability as the legacy generated
    // constructor table; keep it in the single static-method surface.
    pairs.push(("of".to_string(), crate::host::scheduler_capability(2044)));
    pairs.push(("poolSize".to_string(), Value::Number(8192.0)));
    pairs.push(("kMaxLength".to_string(), Value::Number(MAX_LENGTH)));
    pairs.push((
        "prototype".to_string(),
        crate::modules::buffer_proto::buffer_prototype(),
    ));
    pairs
}

/// The `Buffer` constructor as a callable host function carrying the
/// static methods as own properties.
pub fn buffer_constructor() -> Value {
    let constructor = crate::host::capability(crate::registry::SPEC_BUFFER_NEW);
    for (key, value) in static_pairs() {
        quench_runtime::execute::set_property(constructor.clone(), &key, value);
    }
    constructor
}

/// Kept for `build_module` compatibility; pairs of the statics.
pub fn build() -> Vec<(String, Value)> {
    static_pairs()
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn btoa(args: &[Value]) -> String {
    let input: Vec<u8> = match args.first() {
        Some(Value::String(s)) => s.bytes().collect(),
        _ => Vec::new(),
    };
    enc::base64_encode(&input, true, false)
}

pub fn atob(args: &[Value]) -> Result<String, VmError> {
    let Some(Value::String(input)) = args.first() else {
        return Ok(String::new());
    };
    if input
        .bytes()
        .any(|b| !b.is_ascii_whitespace() && b != b'=' && !B64.contains(&b))
    {
        return Err(VmError::EvalError("InvalidCharacterError".into()));
    }
    Ok(enc::base64_decode(input.as_bytes())
        .into_iter()
        .map(|b| b as char)
        .collect())
}

/// Validate a Buffer size argument; returns the size as `usize`.
fn size_arg(value: Option<&Value>, name: &str) -> Result<usize, VmError> {
    let value = value.cloned().unwrap_or(Value::Undefined);
    let Value::Number(n) = value else {
        return Err(enc::invalid_arg_type(format!(
            "The \"{name}\" argument must be of type number.{}",
            crate::modules::util::invalid_arg_received(&value)
        )));
    };
    if !(0.0..=MAX_LENGTH).contains(&n) {
        return Err(enc::out_of_range(
            "size",
            &format!(">= 0 && <= {}", MAX_LENGTH as u64),
            &enc::fmt_num(n),
        ));
    }
    Ok(n.trunc() as usize)
}

/// `Buffer.alloc(size[, fill[, encoding]])` and `allocUnsafe*` fill-0
/// variants.
pub fn alloc(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    alloc_impl(args, true)
}

pub fn alloc_unsafe(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    alloc_impl(args, false)
}

fn alloc_impl(args: &[Value], zero_fill: bool) -> Result<Value, VmError> {
    let size = size_arg(args.first(), "size")?;
    // Node defers to the allocator; past a sanity bound, fail the way
    // an allocation failure surfaces instead of panicking in `vec!`.
    if size > 1 << 33 {
        return Err(VmError::EvalError(
            "Array buffer allocation failed".to_string(),
        ));
    }
    let mut bytes = vec![0u8; size];
    if zero_fill && args.get(1).is_some_and(|v| !matches!(v, Value::Undefined)) {
        apply_fill(&mut bytes, args)?;
    }
    Ok(crate::modules::buffer_proto::make_buffer(&bytes))
}

fn apply_fill(bytes: &mut [u8], args: &[Value]) -> Result<(), VmError> {
    let fill = args.get(1).cloned().unwrap_or(Value::Undefined);
    let pattern: Vec<u8> = match &fill {
        Value::Number(n) => vec![*n as i64 as u8],
        Value::String(_) | Value::StringUnits(_) => {
            let encoding = encoding_name(args.get(2))?;
            let encoded = enc::encode_value(&fill, &encoding)?;
            if encoded.is_empty() {
                if encoding == "hex"
                    && !matches!(&fill, Value::String(value) if value.is_empty())
                    && !matches!(&fill, Value::StringUnits(value) if value.is_empty())
                {
                    return Err(enc::invalid_arg_value(format!(
                        "The argument 'value' is invalid. Received {fill:?}"
                    )));
                }
                return Ok(());
            }
            encoded
        }
        Value::Uint8Array(view) => {
            if view.length == 0 {
                return Err(enc::invalid_arg_value(
                    "The argument 'value' is invalid. Received an empty buffer".to_string(),
                ));
            }
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        _ => Vec::new(),
    };
    if pattern.is_empty() {
        return Ok(());
    }
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = pattern[i % pattern.len()];
    }
    Ok(())
}

pub fn byte_length(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let v = args.first().cloned().unwrap_or(Value::Undefined);
    let n = match &v {
        Value::String(_) | Value::StringUnits(_) => {
            // byteLength treats unrecognized encodings as utf8.
            let encoding = match args.get(1) {
                Some(Value::String(s)) => enc::canonical_encoding(s).unwrap_or("utf8"),
                _ => "utf8",
            };
            enc::encode_value(&v, encoding)?.len()
        }
        Value::Uint8Array(arr) => arr.length,
        Value::ArrayBuffer(buf) => buf.bytes.borrow().len(),
        Value::DataView(view) => view.byte_length,
        Value::Int8Array(v) => v.length,
        Value::Uint8ClampedArray(v) => v.length,
        Value::Int16Array(v) => v.length * 2,
        Value::Uint16Array(v) => v.length * 2,
        Value::Int32Array(v) => v.length * 4,
        Value::Uint32Array(v) => v.length * 4,
        Value::Float32Array(v) => v.length * 4,
        Value::Float64Array(v) => v.length * 8,
        Value::BigInt64Array(v) => v.length * 8,
        Value::BigUint64Array(v) => v.length * 8,
        _ => {
            return Err(enc::invalid_arg_type(format!(
                "The \"string\" argument must be of type string or an instance of \
                 Buffer or ArrayBuffer.{}",
                crate::modules::util::invalid_arg_received(&v)
            )));
        }
    };
    Ok(Value::Number(n as f64))
}

pub fn is_buffer(args: &[Value]) -> bool {
    let Some(Value::Uint8Array(_)) = args.first() else {
        return false;
    };
    let value = args.first().expect("checked above");
    let prototype = quench_runtime::execute::get_prototype_of(value).unwrap_or(Value::Undefined);
    if quench_runtime::execute::same_value(
        &prototype,
        &crate::modules::buffer_proto::buffer_prototype(),
    ) {
        return true;
    }
    // Host-created Buffer views carry the canonical backing-store marker;
    // ordinary Uint8Array views do not. This preserves Buffer identity even
    // when a view is returned through a copy-on-write prototype path.
    matches!(
        quench_runtime::execute::get_property(value, "parent"),
        Value::ArrayBuffer(_)
    )
}

/// `Buffer.isEncoding(name)`.
pub fn is_encoding(args: &[Value]) -> bool {
    match args.first() {
        Some(Value::String(s)) => enc::canonical_encoding(s).is_some(),
        _ => false,
    }
}

pub fn concat(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().cloned().unwrap_or(Value::Undefined);
    if !matches!(list, Value::Array(_)) {
        // Node reports a Buffer passed where an Array is required through its
        // ordinary object classification, even though Buffer is a byte view.
        let received = if matches!(list, Value::Uint8Array(_)) {
            " Received an instance of Object".to_string()
        } else {
            crate::modules::util::invalid_arg_received(&list)
        };
        return Err(enc::invalid_arg_type(format!(
            "The \"list\" argument must be an instance of Array.{}",
            received
        )));
    }
    let total = match args.get(1) {
        None | Some(Value::Undefined) => None,
        Some(Value::Number(value)) if value.is_finite() && value.fract() == 0.0 => {
            if *value < 0.0 || *value > 9_007_199_254_740_991.0 {
                return Err(enc::out_of_range(
                    "length",
                    ">= 0 && <= 9007199254740991",
                    &enc::fmt_num(*value),
                ));
            }
            if matches!(&list, Value::Array(array) if array.logical_len() == 0) {
                return Ok(crate::modules::buffer_proto::make_buffer(&[]));
            }
            Some(*value as usize)
        }
        Some(value) => {
            return Err(enc::out_of_range(
                "length",
                "an integer",
                &match value {
                    Value::Number(number) => enc::fmt_num(*number),
                    _ => format!("{value:?}"),
                },
            ));
        }
    };
    let mut all = Vec::new();
    for i in 0..u32::MAX {
        let v = get_property(&list, &i.to_string());
        if matches!(v, Value::Undefined) {
            break;
        }
        match v {
            Value::Uint8Array(arr) => {
                let b = arr.buffer.bytes.borrow();
                all.extend_from_slice(&b[arr.byte_offset..arr.byte_offset + arr.length]);
            }
            other => {
                return Err(enc::invalid_arg_type(format!(
                    "The \"list[{i}]\" argument must be an instance of Buffer or Uint8Array.{}",
                    crate::modules::util::invalid_arg_received(&other)
                )));
            }
        }
    }
    if let Some(total) = total {
        all.truncate(total.min(all.len()));
        all.resize(total, 0);
    }
    Ok(crate::modules::buffer_proto::make_buffer(&all))
}

pub(crate) fn encoding_name(arg: Option<&Value>) -> Result<String, VmError> {
    match arg {
        None | Some(Value::Undefined) => Ok("utf8".into()),
        Some(Value::String(s)) => enc::canonical_encoding(s)
            .map(str::to_string)
            .ok_or_else(|| enc::unknown_encoding(s)),
        Some(other) => Err(enc::invalid_arg_type(format!(
            "The \"encoding\" argument must be of type string.{}",
            crate::modules::util::invalid_arg_received(other)
        ))),
    }
}

/// Build the `host_api::object` properties for the `Buffer` global.
pub fn build_object() -> Value {
    buffer_constructor()
}

/// Build the `node:buffer` module namespace.
pub fn build_module() -> Value {
    let module_props: Vec<(String, Value)> = vec![
        ("Buffer".to_string(), buffer_constructor()),
        (
            "atob".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_ATOB),
        ),
        (
            "btoa".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_BTOA),
        ),
        (
            "isAscii".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_ISASCII),
        ),
        (
            "isUtf8".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_ISUTF8),
        ),
        ("kMaxLength".to_string(), Value::Number(MAX_LENGTH)),
        (
            "kStringMaxLength".to_string(),
            Value::Number(MAX_STRING_LENGTH),
        ),
        ("INSPECT_MAX_BYTES".to_string(), Value::Number(50.0)),
        ("constants".to_string(), constants_object()),
    ];
    crate::host::namespace_object_from_pairs(module_props)
}

fn constants_object() -> Value {
    host_api::object(vec![
        ("MAX_LENGTH".to_string(), Value::Number(MAX_LENGTH)),
        (
            "MAX_STRING_LENGTH".to_string(),
            Value::Number(MAX_STRING_LENGTH),
        ),
    ])
}
