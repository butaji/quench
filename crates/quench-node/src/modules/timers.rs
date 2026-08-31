//! `timers` module — Timeout objects + the host event-loop pump.
//!
//! `setTimeout`/`setInterval`/`setImmediate` return JS objects that
//! carry a hidden timer id; `unref`/`ref`/`hasRef`/`refresh` are
//! capability methods dispatched with the object as receiver. The
//! pump (`run_event_loop`) drives nextTick, promise jobs, timers,
//! and immediates until no referenced work remains, then runs
//! `beforeExit`/`exit` handlers like Node does at end of run.

use std::cell::Cell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

/// Hidden own property storing the host-side timer id on the JS
/// Timeout/Immediate object.
const TIMER_ID_PROP: &str = "\0quench:timer:id";
/// Node's `TIMEOUT_MAX` (2^31 - 1); larger delays clamp to 1ms.
const TIMEOUT_MAX: f64 = 2_147_483_647.0;
const PROMISES_PRELUDE: &str = include_str!("timers_promises.js");
thread_local! { static MOCK_TIMER_NOW: Cell<Option<u64>> = const { Cell::new(None) }; }

pub fn set_mock_timer_now(value: Option<u64>) {
    MOCK_TIMER_NOW.with(|now| now.set(value));
}
pub fn mock_timer_now() -> Option<u64> {
    MOCK_TIMER_NOW.with(Cell::get)
}

pub enum TimerKind {
    Timeout,
    Interval,
    Immediate,
}

pub struct Timer {
    pub kind: TimerKind,
    pub fire_at: u64,
    pub period: u64,
    pub callback: Value,
    pub args: Vec<Value>,
    pub object: Value,
    pub async_resource: Value,
    pub destroyed: Rc<quench_runtime::value::BindingCell>,
    pub referenced: bool,
    pub active: bool,
    pub domain: Option<Value>,
}

pub struct TimerRegistry {
    pub next_id: u64,
    pub timers: HashMap<u64, Timer>,
}

impl Default for TimerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerRegistry {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            timers: HashMap::new(),
        }
    }

    fn allocate(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

pub fn set_timeout(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    schedule(state, args, TimerKind::Timeout)
}

pub fn set_interval(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    schedule(state, args, TimerKind::Interval)
}

pub fn set_immediate(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    schedule(state, args, TimerKind::Immediate)
}

fn schedule(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    kind: TimerKind,
) -> Result<Value, VmError> {
    let mut cb = args.first().cloned().unwrap_or(Value::Undefined);
    if !quench_runtime::is_callable(&cb) {
        return Err(invalid_callback_error());
    }
    // Capture the JS async context at registration time. The wrapper restores
    // `__nodeCurrentAsyncResource` around invocation, which is the canonical
    // source consumed by AsyncLocalStorage.getStore().
    let global = quench_runtime::vm::current_global_object();
    let capture = quench_runtime::execute::get_property(&global, "__nodeCaptureAsyncCallback");
    let host_capability_callback = matches!(
        &cb,
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(quench_runtime::ops::Builtin::HostCapability(_))
            )
    );
    if quench_runtime::is_callable(&capture) && !host_capability_callback {
        let current = quench_runtime::execute::get_property(&global, "__nodeCurrentAsyncResource");
        cb = quench_runtime::execute::call(&capture, &Value::Undefined, &[cb, current])?;
    }
    let domain = crate::modules::domain::current(state);
    // `setTimeout(cb, delay, ...args)`; `setImmediate(cb, ...args)`.
    let (delay, rest) = match kind {
        TimerKind::Immediate => (0, args.get(1..).unwrap_or(&[]).to_vec()),
        _ => (
            normalize_delay(state, args.get(1)),
            args.get(2..).unwrap_or(&[]).to_vec(),
        ),
    };
    let id = state.borrow_mut().timers.allocate();
    let fire_at = monotonic_ms().saturating_add(delay);
    let destroyed = quench_runtime::value::BindingCell::new(Value::Boolean(false));
    let object = timer_object(id, &destroyed, &kind)?;
    let resource_type = match kind {
        TimerKind::Immediate => "Immediate",
        TimerKind::Timeout | TimerKind::Interval => "Timeout",
    };
    let async_resource =
        crate::modules::async_hooks::attach_resource(state, object.clone(), resource_type)?;
    // AsyncLocalStorage state is carried by the JS resource object. Capture
    // the current resource's store map when the timer is created so the
    // callback observes the same context after resource_before switches to
    // this timer.
    let global = quench_runtime::vm::current_global_object();
    let current = quench_runtime::execute::get_property(&global, "__nodeCurrentAsyncResource");
    let stores = quench_runtime::execute::get_property(&current, "__nodeAsyncStores");
    let async_resource = if matches!(stores, Value::Undefined) {
        async_resource
    } else {
        quench_runtime::execute::set_property(async_resource, "__nodeAsyncStores", stores)
    };
    state.borrow_mut().timers.timers.insert(
        id,
        Timer {
            kind,
            fire_at,
            period: delay,
            callback: cb,
            args: rest,
            object: object.clone(),
            async_resource,
            destroyed,
            referenced: true,
            active: true,
            domain,
        },
    );
    Ok(object)
}

/// Build the JS Timeout/Immediate object: hidden id plus the
/// `unref`/`ref`/`hasRef`/`refresh` capability methods.
fn timer_object(
    id: u64,
    destroyed: &Rc<quench_runtime::value::BindingCell>,
    kind: &TimerKind,
) -> Result<Value, VmError> {
    let constructor_name = match kind {
        TimerKind::Immediate => "Immediate",
        TimerKind::Timeout | TimerKind::Interval => "Timeout",
    };
    let object = crate::host::namespace_object_from_pairs(vec![
        (TIMER_ID_PROP.to_string(), Value::Number(id as f64)),
        (
            "_destroyed".to_string(),
            Value::BindingCell(Rc::clone(destroyed)),
        ),
        (
            "constructor".to_string(),
            host_api::object(vec![(
                "name".to_string(),
                Value::String(constructor_name.to_string()),
            )]),
        ),
    ]);
    let methods: Vec<(&str, crate::registry::NodeSpec)> = vec![
        ("unref", crate::registry::SPEC_TIMERS_UNREF),
        ("ref", crate::registry::SPEC_TIMERS_REF),
        ("hasRef", crate::registry::SPEC_TIMERS_HASREF),
        ("refresh", crate::registry::SPEC_TIMERS_REFRESH),
        ("close", crate::registry::SPEC_TIMERS_CLOSE),
        ("Symbol.dispose", crate::registry::SPEC_TIMERS_CLOSE),
        (
            "Symbol.toPrimitive",
            crate::registry::SPEC_TIMERS_TO_PRIMITIVE,
        ),
    ];
    let mut object = object;
    for (key, spec) in methods {
        let descriptor = host_api::object(vec![
            ("value".to_string(), crate::host::capability(spec)),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]);
        object = quench_runtime::execute::define_property(object, key, descriptor)?;
    }
    Ok(object)
}

/// `clearTimeout`/`clearInterval` cancel Timeout/Interval entries.
pub fn clear_timeout(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    clear_matching(state, args, false)
}

/// `clearImmediate` cancels only Immediate entries.
pub fn clear_immediate(state: &Rc<RefCell<HostState>>, args: &[Value]) -> Result<Value, VmError> {
    clear_matching(state, args, true)
}

fn clear_matching(
    state: &Rc<RefCell<HostState>>,
    args: &[Value],
    immediate: bool,
) -> Result<Value, VmError> {
    let Some(id) = args.first().and_then(value_to_id) else {
        return Ok(Value::Undefined);
    };
    let matches = state
        .borrow()
        .timers
        .timers
        .get(&id)
        .is_some_and(|t| matches!(t.kind, TimerKind::Immediate) == immediate);
    if matches {
        let timer = state.borrow_mut().timers.timers.remove(&id);
        if let Some(timer) = timer {
            mark_destroyed(&timer);
            crate::modules::async_hooks::resource_destroy(state, Some(&timer.async_resource), &[])?;
        }
    }
    Ok(Value::Undefined)
}

pub(crate) fn mark_destroyed(timer: &Timer) {
    *timer.destroyed.borrow_mut() = Value::Boolean(true);
}

fn timer_id_of(receiver: Option<&Value>) -> Option<u64> {
    receiver.and_then(value_to_id)
}

pub fn method_unref(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Value {
    if let Some(id) = timer_id_of(receiver) {
        if let Some(timer) = state.borrow_mut().timers.timers.get_mut(&id) {
            timer.referenced = false;
        }
    }
    receiver.cloned().unwrap_or(Value::Undefined)
}

pub fn method_ref(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Value {
    if let Some(id) = timer_id_of(receiver) {
        if let Some(timer) = state.borrow_mut().timers.timers.get_mut(&id) {
            timer.referenced = true;
        }
    }
    receiver.cloned().unwrap_or(Value::Undefined)
}

pub fn method_has_ref(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Value {
    let referenced = timer_id_of(receiver)
        .and_then(|id| state.borrow().timers.timers.get(&id).map(|t| t.referenced))
        .unwrap_or(false);
    Value::Boolean(referenced)
}

/// `timer.close()` — alias for clearing the timer (used by
/// `Symbol.dispose` and legacy unenroll paths in Node).
pub fn method_close(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Value {
    if let Some(id) = timer_id_of(receiver) {
        if let Some(timer) = state.borrow_mut().timers.timers.remove(&id) {
            mark_destroyed(&timer);
        }
    }
    receiver.cloned().unwrap_or(Value::Undefined)
}

pub fn method_refresh(state: &Rc<RefCell<HostState>>, receiver: Option<&Value>) -> Value {
    if let Some(id) = timer_id_of(receiver) {
        if let Some(timer) = state.borrow_mut().timers.timers.get_mut(&id) {
            timer.fire_at = monotonic_ms().saturating_add(timer.period.max(1));
            timer.active = true;
            *timer.destroyed.borrow_mut() = Value::Boolean(false);
        }
    }
    receiver.cloned().unwrap_or(Value::Undefined)
}

pub fn method_to_primitive(receiver: Option<&Value>) -> Value {
    Value::Number(timer_id_of(receiver).unwrap_or(0) as f64)
}

/// Handler lists whose `once` entries must be consumed on fire.
#[derive(Clone, Copy)]
pub(crate) enum HandlerKind {
    UncaughtException,
    Warning,
}

/// Snapshot the handler list, dropping fired `once` handlers.
pub(crate) fn take_once_handlers(state: &Rc<RefCell<HostState>>, kind: HandlerKind) -> Vec<Value> {
    let mut guard = state.borrow_mut();
    let list = match kind {
        HandlerKind::UncaughtException => &mut guard.process.uncaught_exception_handlers,
        HandlerKind::Warning => &mut guard.process.warning_handlers,
    };
    let handlers: Vec<Value> = list.iter().map(|(handler, _)| handler.clone()).collect();
    list.retain(|(_, once)| !once);
    handlers
}

fn invalid_callback_error() -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".to_string(), Value::String("TypeError".to_string())),
        (
            "message".to_string(),
            Value::String("The callback argument must be of type function".to_string()),
        ),
        (
            "code".to_string(),
            Value::String("ERR_INVALID_ARG_TYPE".to_string()),
        ),
    ]))
}

pub(crate) fn monotonic_ms() -> u64 {
    if let Some(value) = mock_timer_now() {
        return value;
    }
    if quench_runtime::date::mock_enabled() {
        return quench_runtime::date::current_time_ms().max(0.0) as u64;
    }
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis() as u64
}

/// Node delay normalization with duration warnings: NaN, negative,
/// and >2^31-1 delays clamp to 1ms and emit a process warning.
fn normalize_delay(state: &Rc<RefCell<HostState>>, value: Option<&Value>) -> u64 {
    let Some(Value::Number(n)) = value else {
        return match value {
            None => 0,
            Some(_) => 1,
        };
    };
    let n = *n;
    if n.is_finite() && (0.0..=1.0).contains(&n) {
        return if n >= 1.0 { 1 } else { 0 };
    }
    if n.is_finite() && (1.0..=TIMEOUT_MAX).contains(&n) {
        return n as u64;
    }
    let (name, first_line) = if n.is_nan() {
        ("TimeoutNaNWarning", format!("{n} is not a number."))
    } else if n.is_finite() && n <= TIMEOUT_MAX {
        (
            "TimeoutNegativeWarning",
            format!("{n} is a negative number."),
        )
    } else {
        (
            "TimeoutOverflowWarning",
            format!("{n} does not fit into a 32-bit signed integer."),
        )
    };
    emit_warning(
        state,
        name,
        &format!("{first_line}\nTimeout duration was set to 1."),
    );
    1
}

/// Queue a process `warning` event for registered handlers.
fn emit_warning(state: &Rc<RefCell<HostState>>, name: &str, message: &str) {
    // NaN/negative duration warnings fire once per process; overflow
    // warnings fire per call (Node semantics).
    let once_per_process = matches!(name, "TimeoutNaNWarning" | "TimeoutNegativeWarning");
    crate::modules::process::emit_warning(state, name, message, None, once_per_process);
}

fn value_to_id(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) if n.is_finite() && *n >= 0.0 => Some(*n as u64),
        Value::String(value) => value.parse::<u64>().ok(),
        Value::Object(_) => match quench_runtime::vm::get_property(value, TIMER_ID_PROP) {
            Value::Number(n) if n.is_finite() && n >= 0.0 => Some(n as u64),
            _ => None,
        },
        _ => None,
    }
}

pub fn tick(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::Undefined)
}

/// Build the `timers` namespace bindings.
pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "setTimeout".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_SETTIMEOUT),
        ),
        (
            "clearTimeout".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_CLEARTIMEOUT),
        ),
        (
            "setInterval".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_SETINTERVAL),
        ),
        (
            "clearInterval".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_CLEARINTERVAL),
        ),
        (
            "setImmediate".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_SETIMMEDIATE),
        ),
        (
            "clearImmediate".to_string(),
            crate::host::capability(crate::registry::SPEC_TIMERS_CLEARIMMEDIATE),
        ),
    ]
}

/// Build the Promise-returning timer namespace from the same timer
/// capabilities as the callback API.
pub fn build_promises() -> Result<Value, VmError> {
    let program = quench_runtime::reduce::reduce_global_script_source(PROMISES_PRELUDE)
        .map_err(|errors| VmError::EvalError(errors.join("; ")))?;
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let factory = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    })?;
    let timers = crate::host::namespace_object_from_pairs(build());
    quench_runtime::vm::call_value(&factory, &Value::Undefined, &[timers])
}

/// Build the callback namespace with Node's promisify identity links.
pub fn build_with_promises() -> Result<Value, VmError> {
    let promises = build_promises()?;
    let mut bindings = build();
    for (name, value) in &mut bindings {
        let promise_name = match name.as_str() {
            "setTimeout" => Some("setTimeout"),
            "setImmediate" => Some("setImmediate"),
            _ => None,
        };
        if let Some(promise_name) = promise_name {
            let promise = quench_runtime::execute::get_property_result(&promises, promise_name)?;
            *value = quench_runtime::execute::set_property(
                value.clone(),
                crate::modules::util::PROMISIFY_CUSTOM_KEY,
                promise,
            );
        }
    }
    bindings.push(("promises".to_string(), promises));
    Ok(crate::host::namespace_object_from_pairs(bindings))
}
