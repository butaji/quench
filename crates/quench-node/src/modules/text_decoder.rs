//! Minimal WHATWG `TextDecoder` — `utf-8` plus the single-byte
//! `windows-1252` encoding (the WHATWG superset of Latin-1/ASCII).

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// `new TextDecoder([label])`.
pub fn new_text_decoder(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let label = match args.first() {
        None | Some(Value::Undefined) => "utf-8".to_string(),
        Some(value) => execute::to_js_string(value)?,
    };
    let encoding = normalize_label(&label).ok_or_else(|| {
        VmError::Thrown(host_api::object(vec![
            ("name".to_string(), Value::String("RangeError".to_string())),
            (
                "message".to_string(),
                Value::String(format!(
                    "The encoding label provided ('{label}') is invalid."
                )),
            ),
        ]))
    })?;
    let fatal = matches!(
        args.get(1).and_then(|options| match options {
            Value::Object(_) | Value::ObjectAlias(_) =>
                Some(execute::get_property(options, "fatal")),
            _ => None,
        }),
        Some(Value::Boolean(true))
    );
    let decode = crate::host::capability(crate::registry::SPEC_TEXT_DECODER_DECODE);
    Ok(host_api::object(vec![
        (
            "\0encoding".to_string(),
            Value::String(encoding.to_string()),
        ),
        ("encoding".to_string(), Value::String(encoding.to_string())),
        ("\0fatal".to_string(), Value::Boolean(fatal)),
        ("decode".to_string(), decode),
        (
            "\u{0}quench:descriptor:\u{0}decode".to_string(),
            host_api::object(vec![
                (
                    "value".to_string(),
                    crate::host::capability(crate::registry::SPEC_TEXT_DECODER_DECODE),
                ),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]),
        ),
    ]))
}

/// `decoder.decode([input])`.
pub fn decode(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let encoding = match receiver {
        Some(value) => match execute::get_property(value, "\0encoding") {
            Value::String(encoding) => encoding,
            _ => "utf-8".to_string(),
        },
        None => "utf-8".to_string(),
    };
    let bytes = match args.first() {
        None | Some(Value::Undefined) => Vec::new(),
        Some(Value::Uint8Array(view)) => {
            let buffer = view.buffer.bytes.borrow();
            buffer[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        Some(Value::ArrayBuffer(buffer)) => buffer.bytes.borrow().clone(),
        Some(other) => {
            return Err(execute::type_error(&format!(
                "The \"input\" argument must be an instance of ArrayBuffer or ArrayBufferView.{}",
                crate::modules::util::invalid_arg_received(other)
            )))
        }
    };
    let fatal = matches!(
        receiver.map(|value| execute::get_property(value, "\0fatal")),
        Some(Value::Boolean(true))
    );
    let text = if encoding == "windows-1252" {
        // WHATWG-canonical windows-1252 decoder (encoding_rs).
        encoding_rs::WINDOWS_1252.decode(&bytes).0.into_owned()
    } else if fatal {
        String::from_utf8(bytes).map_err(|_| {
            VmError::Thrown(host_api::object(vec![
                ("name".into(), Value::String("TypeError".into())),
                (
                    "message".into(),
                    Value::String("The encoded data was not valid UTF-8".into()),
                ),
            ]))
        })?
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    };
    Ok(Value::String(text))
}

fn normalize_label(label: &str) -> Option<&'static str> {
    match label.trim().to_ascii_lowercase().as_str() {
        "utf-8" | "utf8" | "unicode-1-1-utf-8" => Some("utf-8"),
        "windows-1252" | "latin1" | "iso-8859-1" | "us-ascii" | "ascii" => Some("windows-1252"),
        _ => None,
    }
}
