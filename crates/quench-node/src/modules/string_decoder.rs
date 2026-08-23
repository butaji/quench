//! `string_decoder` module — UTF-8 byte-to-string converter.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn new_decoder(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    let state = Rc::new(RefCell::new(StringDecoder::new()));
    let mut props = Vec::new();
    let _ = state;
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
    vec![(
        "StringDecoder".to_string(),
        crate::host::capability(crate::registry::SPEC_STRING_DECODER),
    )]
}
