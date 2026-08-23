//! `string_decoder` module — UTF-8 byte-to-string converter.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn new_decoder(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let mut props = Vec::new();
    props.push((
        "write".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new(
            "string_decoder:write",
            0x0D01,
        )),
    ));
    props.push((
        "end".to_string(),
        crate::host::capability(crate::registry::NodeSpec::new("string_decoder:end", 0x0D02)),
    ));
    Ok(host_api::object(props))
}

pub fn write(_state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let bytes = match args.first() {
        Some(Value::Uint8Array(view)) => {
            view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()
        }
        Some(Value::String(text)) => text.as_bytes().to_vec(),
        None => Vec::new(),
        _ => {
            return Err(VmError::EvalError(
                "StringDecoder.write expects bytes or string".into(),
            ))
        }
    };
    Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
}

pub fn end(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    if args.is_empty() {
        return Ok(Value::String(String::new()));
    }
    write(state, args)
}

pub struct StringDecoder {
    pub buffer: Vec<u8>,
}

impl Default for StringDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl StringDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }
}

pub fn build() -> Vec<(String, Value)> {
    let constructor = crate::host::capability(crate::registry::SPEC_STRING_DECODER);
    let prototype = host_api::object(vec![
        (
            "write".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new(
                "string_decoder:write",
                0x0D01,
            )),
        ),
        (
            "end".to_string(),
            crate::host::capability(crate::registry::NodeSpec::new("string_decoder:end", 0x0D02)),
        ),
    ]);
    let _ = quench_runtime::execute::set_callable_property(&constructor, "prototype", prototype);
    vec![("StringDecoder".to_string(), constructor)]
}
