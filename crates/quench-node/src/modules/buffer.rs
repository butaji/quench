//! `Buffer` module — pure Rust Buffer atop Uint8Array semantics.
//!
//! Every Buffer is a `Value::Uint8Array` plus a marker property
//! (the well-known `Buffer.isBuffer` check). Encodings are
//! pure Rust; no JS shim.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::{ArrayBufferData, Uint8ArrayData, Value};
use quench_runtime::vm::get_property;

pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "from".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_FROM),
        ),
        (
            "alloc".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_ALLOC),
        ),
        (
            "byteLength".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_BYTELENGTH),
        ),
        (
            "isBuffer".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_ISBUFFER),
        ),
        (
            "concat".to_string(),
            crate::host::capability(crate::registry::SPEC_BUFFER_CONCAT),
        ),
    ]
}

pub fn from_handler(
    state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    from(state, args)
}

pub fn from(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let first = args.first().cloned().unwrap_or(Value::Undefined);
    let first = args.first().cloned().unwrap_or(Value::Undefined);
    match first {
        Value::Uint8Array(arr) => Ok(Value::Uint8Array(arr)),
        Value::String(s) => {
            let encoding = encoding_name(args.get(1));
            let bytes = encode(&s, &encoding);
            Ok(make_buffer(&bytes))
        }
        Value::Array(_) => {
            let mut bytes = Vec::new();
            for i in 0..u32::MAX {
                let key = i.to_string();
                let v = get_property(&first, &key);
                if matches!(v, Value::Undefined) {
                    break;
                }
                let n = to_number(&v);
                bytes.push(if n.is_nan() { 0 } else { n as u8 });
            }
            Ok(make_buffer(&bytes))
        }
        Value::ArrayBuffer(buf) => {
            let bytes = buf.bytes.borrow().clone();
            Ok(make_buffer(&bytes))
        }
        Value::Number(n) => Ok(make_buffer(&vec![0u8; n as usize])),
        _ => Ok(make_buffer(&[])),
    }
}

pub fn alloc(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let size = args.first().map(to_usize).unwrap_or(0);
    let fill = args.get(1).cloned().unwrap_or(Value::Number(0.0));
    let mut bytes = vec![0u8; size];
    let value = to_number(&fill) as u8;
    for b in bytes.iter_mut() {
        *b = value;
    }
    Ok(make_buffer(&bytes))
}

pub fn byte_length(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let v = args.first().cloned().unwrap_or(Value::Undefined);
    let n = match v {
        Value::String(s) => {
            let encoding = encoding_name(args.get(1));
            encode(&s, &encoding).len()
        }
        Value::Uint8Array(arr) => arr.length,
        Value::ArrayBuffer(buf) => buf.bytes.borrow().len(),
        _ => 0,
    };
    Ok(Value::Number(n as f64))
}

pub fn is_buffer(args: &[Value]) -> bool {
    matches!(
        args.first().cloned().unwrap_or(Value::Undefined),
        Value::Uint8Array(_)
    )
}

pub fn concat(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let list = args.first().cloned().unwrap_or(Value::Undefined);
    let total = args.get(1).map(to_usize);
    let mut all = Vec::new();
    if matches!(list, Value::Array(_)) {
        for i in 0..u32::MAX {
            let key = i.to_string();
            let v = get_property(&list, &key);
            if matches!(v, Value::Undefined) {
                break;
            }
            match v {
                Value::Uint8Array(arr) => {
                    let b = arr.buffer.bytes.borrow();
                    all.extend_from_slice(&b[arr.byte_offset..arr.byte_offset + arr.length]);
                }
                _ => return Ok(Value::Undefined),
            }
        }
    }
    if let Some(t) = total {
        all.truncate(t);
    }
    Ok(make_buffer(&all))
}

fn make_buffer(bytes: &[u8]) -> Value {
    let buf = Rc::new(ArrayBufferData::new(bytes.len()));
    buf.bytes.borrow_mut().copy_from_slice(bytes);
    let ba = Rc::new(buf.transfer_to_immutable());
    let view = Rc::new(Uint8ArrayData::new(ba, 0, bytes.len()));
    Value::Uint8Array(view)
}

fn encoding_name(arg: Option<&Value>) -> String {
    match arg {
        Some(Value::String(s)) => s.to_lowercase(),
        _ => "utf8".into(),
    }
}

fn encode(input: &str, encoding: &str) -> Vec<u8> {
    match encoding {
        "utf8" | "utf-8" | "" => input.as_bytes().to_vec(),
        "ascii" => input.bytes().map(|b| b & 0x7F).collect(),
        "latin1" | "binary" => input.as_bytes().to_vec(),
        "hex" => input.as_bytes().to_vec(),
        _ => input.as_bytes().to_vec(),
    }
}

fn to_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::String(s) => s.parse().unwrap_or(0.0),
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => 0.0,
    }
}

fn to_usize(value: &Value) -> usize {
    to_number(value).max(0.0) as usize
}

/// Build the `host_api::object` properties for the `Buffer` global.
pub fn build_object() -> Value {
    host_api::object(build())
}

/// Build the `node:buffer` module namespace.
pub fn build_module() -> Value {
    let mut module_props: Vec<(String, Value)> = build();
    // The constructor `Buffer` is itself a function with static
    // methods (`Buffer.from`, `Buffer.alloc`, …). Easiest path
    // for the host: store the constructor as the `Buffer` key on
    // the module, and put the static methods on the constructor
    // by using the same call identifiers.
    let buffer_constructor = crate::host::namespace_object_from_pairs(build());
    module_props.push(("Buffer".to_string(), buffer_constructor));
    crate::host::namespace_object_from_pairs(module_props)
}
