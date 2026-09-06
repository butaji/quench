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
    if let Ok(create_task) = eval_function(
        r#"(name) => ({ name: String(name || ""), run: (callback, ...args) => callback(...args) })"#,
    ) {
        module = quench_runtime::execute::set_property(module, "createTask", create_task);
    }
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
                "_format",
            ] {
                if let Ok(method) = quench_runtime::execute::get_property_result(&prototype, name) {
                    module = quench_runtime::execute::set_property(module, name, method);
                }
            }
            for (name, spec) in [
                ("log", crate::registry::SPEC_CONSOLE_LOG),
                ("info", crate::registry::SPEC_CONSOLE_INFO),
                ("debug", crate::registry::SPEC_CONSOLE_DEBUG),
                ("warn", crate::registry::SPEC_CONSOLE_WARN),
                ("error", crate::registry::SPEC_CONSOLE_ERROR),
            ] {
                module = quench_runtime::execute::set_property(
                    module,
                    name,
                    crate::host::capability(spec),
                );
            }
        }
        module = quench_runtime::execute::set_property(module, "Console", console);
    }
    module
}

const CONSOLE_CLASS: &str = r#"(class Console {
  constructor(stdout, stderr) {
    const options = stdout && typeof stdout === "object" &&
      (stdout.stdout || stdout.stderr) ? stdout : null;
    this._stdout = options ? options.stdout : stdout;
    this._stderr = options ? options.stderr : stderr;
    this._inspectOptions = options ? options.inspectOptions : undefined;
    if (!this._stdout) this._stdout = globalThis?.process?.stdout;
    if (!this._stderr) this._stderr = globalThis?.process?.stderr;
  }
  _format(output, args) {
    const util = (typeof require === "function"
        ? require("util")
        : undefined);
    const format = util?.format;
    const formatWithOptions = util?.formatWithOptions;
    const configured = this._inspectOptions &&
      typeof this._inspectOptions.get === "function"
      ? this._inspectOptions.get(output)
      : this._inspectOptions;
    if (configured && typeof formatWithOptions === "function") {
      return formatWithOptions(configured, ...args);
    }
    return typeof format === "function" ? format(...args) : args.join(" ");
  }
  log(...args) {
    const output = this._stdout || process?.stdout;
    if (output && typeof output.write === "function") output.write(`${this._format(output, args)}\n`);
    if (!this._tickPending) {
      this._tickPending = true;
      const tick = globalThis?.process?.nextTick;
      if (typeof tick === "function") tick(() => { this._tickPending = false; });
    }
  }
  info(...args) { this.log(...args); }
  dir(...args) { this.log(...args); }
  time(label = "default") { if (typeof label === "symbol") throw new TypeError("Invalid console label"); this._times ||= new Map(); if (!this._times.has(label)) this._times.set(label, Date.now()); }
  timeEnd(label = "default") { if (typeof label === "symbol") throw new TypeError("Invalid console label"); this._times?.delete(label); }
  timeLog(label = "default", ...args) { this.log(...args); }
  warn(...args) {
    const output = this._stderr || process?.stderr;
    if (output && typeof output.write === "function") output.write(`${this._format(output, args)}\n`);
  }
  error(...args) { this.warn(...args); }
  trace(...args) { this.error(...args); }
  assert(condition, ...args) { if (!condition) this.error(...args); }
  clear() {}
  count(label = "default") { this._counts ||= new Map(); this._counts.set(label, (this._counts.get(label) || 0) + 1); }
  countReset(label = "default") { this._counts?.delete(label); }
  group() {}
  groupEnd() {}
  table(...args) { this.log(...args); }
  debug(...args) { this.log(...args); }
  dirxml(...args) { this.log(...args); }
  groupCollapsed() {}
})"#;

pub fn log(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    is_error: bool,
) -> Result<Value, quench_runtime::execute::VmError> {
    log_named(
        state,
        args,
        is_error,
        if is_error {
            "console.error"
        } else {
            "console.log"
        },
    )
}

pub fn log_named(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    is_error: bool,
    channel_name: &str,
) -> Result<Value, quench_runtime::execute::VmError> {
    // Console callbacks can be invoked from inside an EventEmitter dispatch
    // that still owns a HostState borrow. Diagnostics publication is
    // best-effort in that re-entrant window; defer it rather than panicking
    // before the console's primary output side effect.
    if state.try_borrow_mut().is_ok() {
        let channel = crate::modules::diagnostics_channel::channel(
            state,
            None,
            &[Value::String(channel_name.into())],
        )?;
        let message = quench_runtime::host_api::array(args.to_vec());
        crate::modules::diagnostics_channel::publish(state, Some(&channel), &[message])?;
    }
    let line = format_args(args);
    let process = state
        .borrow()
        .process_module
        .clone()
        .unwrap_or_else(|| quench_runtime::vm::current_global_object());
    let stream_name = if is_error { "stderr" } else { "stdout" };
    let stream = quench_runtime::execute::get_property(&process, stream_name);
    let write = quench_runtime::execute::get_property(&stream, "write");
    if quench_runtime::is_callable(&write) {
        let _ =
            quench_runtime::execute::call(&write, &stream, &[Value::String(format!("{line}\n"))]);
    } else if is_error {
        eprintln!("{line}");
    } else if let Some(sink) = &state.borrow().output {
        sink(&line);
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
        sink(&line);
    }
    Ok(Value::Undefined)
}

fn format_args(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    match &args[0] {
        Value::String(template) => crate::modules::util::format_template(template, args),
        Value::StringUnits(units) => {
            let template = String::from_utf16_lossy(units);
            crate::modules::util::format_template(&template, args)
        }
        _ => {
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
}

fn eval_function(source: &str) -> Result<Value, quench_runtime::execute::VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(source)
        .map_err(|errors| quench_runtime::execute::VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
}
