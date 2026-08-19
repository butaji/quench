//! Minimal WHATWG `TextEncoder` — always `utf-8`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// `new TextEncoder()`.
pub fn new_text_encoder(
    _state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, VmError> {
    let non_enum = |spec: (crate::registry::NodeSpec, &str)| {
        (
            format!("\u{0}quench:descriptor:\u{0}{}", spec.1),
            host_api::object(vec![
                ("value".to_string(), crate::host::capability(spec.0)),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]),
        )
    };
    let encode = (crate::registry::SPEC_TEXT_ENCODER_ENCODE, "encode");
    let encode_into = (crate::registry::SPEC_TEXT_ENCODER_ENCODE_INTO, "encodeInto");
    Ok(host_api::object(vec![
        ("encoding".to_string(), Value::String("utf-8".to_string())),
        ("encode".to_string(), crate::host::capability(encode.0)),
        (
            "encodeInto".to_string(),
            crate::host::capability(encode_into.0),
        ),
        non_enum(encode),
        non_enum(encode_into),
    ]))
}

/// `encoder.encode(string)` — fresh `Uint8Array` of UTF-8 bytes.
pub fn encode(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let text = match args.first() {
        None | Some(Value::Undefined) => String::new(),
        Some(value) => execute::to_js_string(value)?,
    };
    let bytes = text.into_bytes();
    let buffer = Rc::new(quench_runtime::value::ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(&bytes);
    Ok(Value::Uint8Array(Rc::new(
        quench_runtime::value::Uint8ArrayData::new(buffer, 0, bytes.len()),
    )))
}

/// `encoder.encodeInto(string, destination)` — `{read, written}`.
pub fn encode_into(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let text = match args.first() {
        None | Some(Value::Undefined) => String::new(),
        Some(value) => execute::to_js_string(value)?,
    };
    let Some(Value::Uint8Array(dest)) = args.get(1) else {
        return Err(execute::type_error(
            "The \"destination\" argument must be an instance of Uint8Array.",
        ));
    };
    let (read, written) = fill_destination(&text, dest);
    Ok(host_api::object(vec![
        ("read".to_string(), Value::Number(read as f64)),
        ("written".to_string(), Value::Number(written as f64)),
    ]))
}

fn fill_destination(text: &str, dest: &quench_runtime::value::Uint8ArrayData) -> (usize, usize) {
    let mut target = dest.buffer.bytes.borrow_mut();
    let capacity = dest.length;
    let mut read = 0;
    let mut written = 0;
    for ch in text.chars() {
        let len = ch.len_utf8();
        if written + len > capacity {
            break;
        }
        let mut buf = [0u8; 4];
        target[dest.byte_offset + written..dest.byte_offset + written + len]
            .copy_from_slice(ch.encode_utf8(&mut buf).as_bytes());
        read += ch.len_utf16();
        written += len;
    }
    (read, written)
}
