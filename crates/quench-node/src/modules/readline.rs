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
    let mut iface = crate::modules::events::new_emitter_object(state)?;
    let mut stored = Vec::new();
    for line in &lines {
        stored.push(Value::String(line.clone()));
    }
    iface = execute::set_property(
        iface,
        "_readlineLines",
        quench_runtime::host_api::array(stored),
    );
    iface = execute::set_property(
        iface,
        "question",
        crate::host::capability(crate::registry::NodeSpec::new(
            "readline:question",
            0x1303,
        )),
    );
    iface = execute::set_property(
        iface,
        "write",
        crate::host::capability(crate::registry::NodeSpec::new(
            "readline:write",
            0x1304,
        )),
    );
    iface = execute::set_property(
        iface,
        "close",
        crate::host::capability(crate::registry::NodeSpec::new(
            "readline:close",
            0x1305,
        )),
    );
    let driver = crate::host::capability(crate::registry::NodeSpec::new("readline:driver", 0x1301));
    let done = crate::host::capability(crate::registry::NodeSpec::new("readline:done", 0x1302));
    for line in lines {
        state.borrow_mut().event_loop.queue_microtask(driver.clone(), vec![iface.clone(), Value::String(line)]);
    }
    state.borrow_mut().event_loop.queue_microtask(done, vec![iface.clone()]);
    Ok(iface)
}

pub fn question(
    state: &Rc<RefCell<HostState>>, receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let Some(rl) = receiver else { return Ok(Value::Undefined); };
    let lines = execute::get_property_result(rl, "_readlineLines")?;
    let line = execute::get_property_result(&lines, "0").unwrap_or(Value::String(String::new()));
    if let Some(cb) = args.get(1).or_else(|| args.get(0)) {
        if quench_runtime::is_callable(cb) {
            let cap = crate::host::capability(crate::registry::NodeSpec::new("readline:questionCallback", 0x1306));
            state.borrow_mut().event_loop.queue_microtask(cap, vec![cb.clone(), line]);
        }
    }
    Ok(Value::Undefined)
}

pub fn question_callback(
    _state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, args: &[Value],
) -> Result<Value, VmError> {
    let cb = args.first().cloned().unwrap_or(Value::Undefined);
    let line = args.get(1).cloned().unwrap_or(Value::String(String::new()));
    quench_runtime::vm::call_value(&cb, &Value::Undefined, &[line])
}

pub fn noop(_state: &Rc<RefCell<HostState>>, _receiver: Option<&Value>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
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
    crate::host::namespace_object(vec![
        ("createInterface", crate::host::capability(crate::registry::SPEC_READLINE)),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}

pub fn interface_methods(iface: Value) -> Value {
    execute::set_property(iface, "question", crate::host::capability(crate::registry::NodeSpec::new("readline:question", 0x1303)))
}
