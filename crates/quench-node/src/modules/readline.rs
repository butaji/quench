//! `readline` module — minimal `createInterface` that reads lines from an
//! `input` (an array of lines or a newline-delimited string) into an
//! EventEmitter interface, emitting `'line'` per line then `'close'`.
//! Lines are deferred through the event loop (microtasks) so listeners
//! registered synchronously fire.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;

/// `readline.createInterface(options)` — reads `options.input`.
pub fn create_interface(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let options = args.first().cloned().unwrap_or(Value::Undefined);
    let input = execute::get_property_result(&options, "input")?;
    let lines = input_lines(&input)?;
    let iface = crate::modules::events::new_emitter_object(state)?;
    let driver = crate::host::capability(crate::registry::NodeSpec::new("readline:driver", 0x1301));
    let done = crate::host::capability(crate::registry::NodeSpec::new("readline:done", 0x1302));
    for line in lines {
        state
            .borrow_mut()
            .event_loop
            .queue_microtask(driver.clone(), vec![iface.clone(), Value::String(line)]);
    }
    state
        .borrow_mut()
        .event_loop
        .queue_microtask(done, vec![iface.clone()]);
    Ok(iface)
}

/// Emit one buffered `'line'` event.
pub fn driver_handler(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let iface = args.first().cloned().unwrap_or(Value::Undefined);
    let line = args
        .get(1)
        .map(execute::to_js_string)
        .transpose()?
        .unwrap_or_default();
    crate::modules::net::emit(state, &iface, "line", vec![Value::String(line)])?;
    Ok(Value::Undefined)
}

/// Emit the terminal `'close'` event.
pub fn done_handler(
    state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let iface = args.first().cloned().unwrap_or(Value::Undefined);
    crate::modules::net::emit(state, &iface, "close", Vec::new())?;
    Ok(Value::Undefined)
}

fn input_lines(input: &Value) -> Result<Vec<String>, VmError> {
    match input {
        Value::Array(array) => {
            let mut lines = Vec::new();
            let len = array.logical_len();
            for i in 0..len {
                if let Ok(item) = execute::get_property_result(input, &i.to_string()) {
                    lines.push(execute::to_js_string(&item)?);
                }
            }
            Ok(lines)
        }
        Value::String(s) => Ok(s.split('\n').map(str::to_string).collect()),
        _ => Ok(Vec::new()),
    }
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![(
        "createInterface",
        crate::host::capability(crate::registry::SPEC_READLINE),
    )])
    .unwrap_or_else(|_| Value::Undefined)
}
