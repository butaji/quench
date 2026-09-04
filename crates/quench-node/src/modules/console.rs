//! `console` module — pure Rust log/info/warn/error/debug/trace.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::util::inspect;

/// Build the `console` namespace object.
pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "log".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_LOG),
        ),
        (
            "info".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_INFO),
        ),
        (
            "warn".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_WARN),
        ),
        (
            "error".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_ERROR),
        ),
        (
            "debug".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_DEBUG),
        ),
        (
            "trace".to_string(),
            crate::host::capability(crate::registry::SPEC_CONSOLE_TRACE),
        ),
    ]
}

pub fn build_value() -> Value {
    let mut module = quench_runtime::host_api::object(build());
    if let Ok(console) = eval_function(CONSOLE_CLASS) {
        if let Ok(prototype) = quench_runtime::execute::get_property_result(&console, "prototype") {
            let _ = quench_runtime::execute::set_prototype_of(&module, &prototype);
            for name in [
                "log",
                "info",
                "dir",
                "time",
                "timeEnd",
                "timeLog",
                "trace",
                "assert",
                "clear",
                "count",
                "countReset",
                "group",
                "groupEnd",
                "table",
                "debug",
                "dirxml",
                "error",
                "groupCollapsed",
            ] {
                if let Ok(method) = quench_runtime::execute::get_property_result(&prototype, name) {
                    module = quench_runtime::execute::set_property(module, name, method);
                }
            }
        }
        module = quench_runtime::execute::set_property(module, "Console", console);
    }
    module
}

const CONSOLE_CLASS: &str = r#"(const __quenchConsoleReceiver = (receiver) =>
  receiver !== null &&
  (typeof receiver === "object" || typeof receiver === "function")
    ? receiver
    : globalThis.console;
class Console {
  constructor(stdout, stderr) {
    const options = stdout && typeof stdout === "object" &&
      (stdout.stdout || stdout.stderr) ? stdout : null;
    this._stdout = options ? options.stdout : stdout;
    this._stderr = options ? options.stderr : stderr;
    if (!this._stdout) this._stdout = globalThis?.process?.stdout;
    if (!this._stderr) this._stderr = globalThis?.process?.stderr;
  }
  log(...args) {
    const receiver = __quenchConsoleReceiver(this);
    const output = receiver?._stdout || globalThis.process?.stdout;
    if (output && typeof output.write === "function") output.write(`${args.join(" ")}\n`);
    if (!receiver._tickPending) {
      receiver._tickPending = true;
      const tick = globalThis?.process?.nextTick;
      if (typeof tick === "function") tick(() => { receiver._tickPending = false; });
    }
  }
  info(...args) { return Console.prototype.log.call(__quenchConsoleReceiver(this), ...args); }
  dir(...args) { return Console.prototype.log.call(__quenchConsoleReceiver(this), ...args); }
  time(label = "default") { const receiver = __quenchConsoleReceiver(this); receiver._times ||= new Map(); if (!receiver._times.has(label)) receiver._times.set(label, Date.now()); }
  timeEnd(label = "default") { __quenchConsoleReceiver(this)._times?.delete(label); }
  timeLog(label = "default", ...args) { return Console.prototype.log.call(__quenchConsoleReceiver(this), ...args); }
  warn(...args) {
    const receiver = __quenchConsoleReceiver(this);
    const output = receiver?._stderr || globalThis.process?.stderr;
    if (output && typeof output.write === "function") output.write(`${args.join(" ")}\n`);
  }
  error(...args) { return Console.prototype.warn.call(__quenchConsoleReceiver(this), ...args); }
  trace(...args) { return Console.prototype.warn.call(__quenchConsoleReceiver(this), ...args); }
  assert(condition, ...args) { if (!condition) Console.prototype.warn.call(__quenchConsoleReceiver(this), ...args); }
  clear() {}
  count(label = "default") { const receiver = __quenchConsoleReceiver(this); receiver._counts ||= new Map(); receiver._counts.set(label, (receiver._counts.get(label) || 0) + 1); }
  countReset(label = "default") { __quenchConsoleReceiver(this)._counts?.delete(label); }
  group() {}
  groupEnd() {}
  table(...args) { return Console.prototype.log.call(__quenchConsoleReceiver(this), ...args); }
  debug(...args) { return Console.prototype.log.call(__quenchConsoleReceiver(this), ...args); }
  dirxml(...args) { return Console.prototype.log.call(__quenchConsoleReceiver(this), ...args); }
  groupCollapsed() {}
})"#;

pub fn log(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    is_error: bool,
) -> Result<Value, quench_runtime::execute::VmError> {
    let line = format_args(args);
    let state = state.borrow();
    if is_error {
        eprintln!("{line}");
    } else if let Some(sink) = &state.output {
        sink(&format!("{line}\n"));
    }
    Ok(Value::Undefined)
}

pub fn info(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, false)
}
pub fn warn(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, true)
}
pub fn error(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, true)
}
pub fn debug(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    log(state, args, false)
}

pub fn trace(
    state: &Rc<RefCell<HostState>>,
    _args: &[Value],
) -> Result<Value, quench_runtime::execute::VmError> {
    let line = "Trace".to_string();
    let state = state.borrow();
    if let Some(sink) = &state.output {
        sink(&format!("{line}\n"));
    }
    Ok(Value::Undefined)
}

fn format_args(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    if let Value::String(template) = &args[0] {
        crate::modules::util::format_template(template, args)
    } else {
        let mut out = String::new();
        for (i, arg) in args.iter().enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push_str(&inspect(arg));
        }
        out
    }
}

fn eval_function(source: &str) -> Result<Value, quench_runtime::execute::VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| quench_runtime::execute::VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}
