//! `string_decoder` module — UTF-8 byte-to-string converter.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn new_decoder(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let mut props = Vec::new();
    props.push(("encoding".to_string(), Value::String("utf8".into())));
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

pub fn write(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let input = args.first().ok_or(VmError::NotCallable)?;
    let key = receiver.object_identity().ok_or(VmError::NotCallable)?;
    let mut bytes = state
        .borrow()
        .string_decoder_pending
        .get(&key)
        .cloned()
        .unwrap_or_default();
    bytes.extend(input_bytes(input)?);
    let (text, pending) = match String::from_utf8(bytes.clone()) {
        Ok(text) => (text, Vec::new()),
        Err(error) if error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            (String::from_utf8_lossy(&bytes[..valid]).into_owned(), bytes[valid..].to_vec())
        }
        Err(_error) => (String::from_utf8_lossy(&bytes).into_owned(), Vec::new()),
    };
    state.borrow_mut().string_decoder_pending.insert(key, pending);
    Ok(Value::String(text))
}

fn input_bytes(value: &Value) -> Result<Vec<u8>, VmError> {
    match value {
        Value::Uint8Array(view) => Ok(view.buffer.bytes.borrow()[view.byte_offset..view.byte_offset + view.length].to_vec()),
        Value::ArrayBuffer(buffer) => Ok(buffer.bytes.borrow().clone()),
        _ => Err(VmError::Thrown(host_api::object(vec![("name".into(), Value::String("TypeError".into())), ("code".into(), Value::String("ERR_INVALID_ARG_TYPE".into()))]))),
    }
}

pub fn end(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    if !args.is_empty() { return write(state, receiver, args); }
    let receiver = receiver.ok_or(VmError::NotCallable)?;
    let key = receiver.object_identity().ok_or(VmError::NotCallable)?;
    let bytes = state.borrow_mut().string_decoder_pending.remove(&key).unwrap_or_default();
    Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned()))
}

pub fn call(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    let target = args.first().ok_or(VmError::NotCallable)?.clone();
    let object = new_decoder(state, &args[1..])?;
    quench_runtime::execute::replace_value(&target, &object);
    for key in ["encoding", "write", "end"] {
        if let Ok(value) = quench_runtime::execute::get_property_result(&object, key) {
            let _ = quench_runtime::execute::set_property(target.clone(), key, value);
        }
    }
    Ok(target)
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
    let call = crate::host::capability(crate::registry::NodeSpec::new("string_decoder:call", 0x0D03));
    let _ = quench_runtime::execute::set_host_capability_property(&constructor, "call", call);
    vec![("StringDecoder".to_string(), constructor)]
}
