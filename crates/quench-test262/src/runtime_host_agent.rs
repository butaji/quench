use quench_runtime::{ops::{HostCapabilityKind, HostCapabilityRef}, value::Value};

#[derive(Default)]
struct AgentState {
    sources: Vec<String>,
    broadcasts: Vec<Value>,
    callbacks: Vec<Value>,
    reports: VecDeque<Value>,
    pending_reports: VecDeque<Value>,
    deferred_reports: VecDeque<Value>,
    current_callback: Option<usize>,
    current_waited: bool,
    return_deferred_next: bool,
    now: f64,
}

thread_local! {
    static AGENT_STATE: RefCell<AgentState> = RefCell::new(AgentState::default());
}

fn reset_agent_state() {
    AGENT_STATE.with(|state| *state.borrow_mut() = AgentState::default());
    quench_runtime::reset_agent_waiters();
}

impl Host for RuntimeHost {
    fn call(
        &self,
        capability: HostCapabilityRef,
        _receiver: Option<&Value>,
        arguments: &[Value],
    ) -> Result<Value, quench_runtime::execute::VmError> {
        match capability.kind {
            HostCapabilityKind::Custom(101) => {
                if let Some(Value::String(source)) = arguments.first() {
                    if source.contains("receiveBroadcast") {
                        AGENT_STATE.with(|state| state.borrow_mut().sources.push(source.clone()));
                    } else {
                        run_agent_source(source)?;
                    }
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(102) => {
                let Some(callback) = arguments.first() else {
                    return Ok(Value::Undefined);
                };
                let broadcasts = AGENT_STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    state.callbacks.push(callback.clone());
                    state.broadcasts.clone()
                });
                for value in broadcasts {
                    invoke_agent_callback(callback, value)?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(103) => {
                if let Some(value) = arguments.first() {
                    AGENT_STATE.with(|state| {
                        let mut state = state.borrow_mut();
                        let waited_now = quench_runtime::take_agent_wait_occurred();
                        if waited_now {
                            state.current_waited = true;
                        }
                        let value = match value {
                            Value::Number(value) => Value::String(value.to_string()),
                            Value::BigInt(value) => Value::String(value.clone()),
                            value => value.clone(),
                        };
                        let is_wait = matches!(&value, Value::String(value) if value.contains("__quench-agent-pending"));
                        if is_wait {
                            state.current_waited = true;
                            state.deferred_reports.push_back(value);
                        } else if waited_now {
                            state.deferred_reports.push_back(value);
                        } else if state.current_waited {
                            state.deferred_reports.push_back(value);
                        } else {
                            state.reports.push_back(value);
                        }
                    });
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(104) | HostCapabilityKind::Custom(112) => {
                let value = AGENT_STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    let deferred = state.return_deferred_next.then(|| state.deferred_reports.pop_front()).flatten();
                    state.return_deferred_next = false;
                    let (value, pending) = if let Some(value) = deferred {
                        (value, false)
                    } else if let Some(value) = state.reports.pop_front() {
                        (value, false)
                    } else if let Some(value) = state.pending_reports.pop_front() {
                        (value, true)
                    } else if let Some(value) = state.deferred_reports.pop_front() {
                        (value, false)
                    } else {
                        (Value::String("timed-out".into()), false)
                    };
                    state.return_deferred_next = pending;
                    value
                });
                Ok(resolve_pending_report(value))
            }
            HostCapabilityKind::Custom(105) | HostCapabilityKind::Custom(113) => {
                let value = arguments
                    .first()
                    .map(broadcast_value)
                    .unwrap_or(Value::Undefined);
                let (sources, existing_callbacks) = AGENT_STATE.with(|state| {
                    let mut state = state.borrow_mut();
                    state.broadcasts.push(value.clone());
                    let existing = state.callbacks.len();
                    (std::mem::take(&mut state.sources), existing)
                });
                for source in sources {
                    run_agent_source(&source)?;
                }
                let callbacks = AGENT_STATE.with(|state| {
                    state.borrow().callbacks[..existing_callbacks].to_vec()
                });
                for callback in callbacks {
                    invoke_agent_callback(&callback, value.clone())?;
                }
                if matches!(capability.kind, HostCapabilityKind::Custom(113)) {
                    Ok(Value::Promise(Rc::new(quench_runtime::value::PromiseData::new(
                        quench_runtime::value::PromiseState::Fulfilled(Value::BigInt("1".into())),
                    ))))
                } else {
                    Ok(Value::Undefined)
                }
            }
            HostCapabilityKind::Custom(106)
            | HostCapabilityKind::Custom(107)
            | HostCapabilityKind::Custom(108)
            | HostCapabilityKind::Custom(114) => Ok(Value::Undefined),
            HostCapabilityKind::Custom(109) => Ok(AGENT_STATE.with(|state| {
                let mut state = state.borrow_mut();
                state.now += 5.0;
                Value::Number(state.now)
            })),
            HostCapabilityKind::Custom(110) => {
                if let Some(callback) = arguments.first() {
                    quench_runtime::vm::call_value(callback, &Value::Undefined, &[])?;
                }
                Ok(Value::Undefined)
            }
            HostCapabilityKind::Custom(115) => Ok(Value::Number(1.0)),
            HostCapabilityKind::Custom(116) => Ok(Value::Number(10.0)),
            HostCapabilityKind::Custom(117) => Ok(Value::Number(100.0)),
            _ => Ok(Value::Undefined),
        }
    }
}

fn invoke_agent_callback(callback: &Value, value: Value) -> Result<(), quench_runtime::execute::VmError> {
    AGENT_STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.current_callback = Some(state.callbacks.len());
        state.current_waited = false;
    });
    let result = quench_runtime::vm::call_value(callback, &Value::Undefined, &[value]);
    AGENT_STATE.with(|state| state.borrow_mut().current_callback = None);
    result.map(|_| ())
}

fn broadcast_value(value: &Value) -> Value {
    match value {
        Value::Float64Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Float32Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Int8Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Int16Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Int32Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Uint8Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Uint8ClampedArray(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Uint16Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::Uint32Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::BigInt64Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::BigUint64Array(view) => Value::ArrayBuffer(view.buffer.clone()),
        Value::DataView(view) => Value::ArrayBuffer(view.buffer.clone()),
        _ => value.clone(),
    }
}

fn run_agent_source(source: &str) -> Result<(), quench_runtime::execute::VmError> {
    let source = remove_noop_spin_loops(source);
    let program = reduce_source(&source).map_err(|errors| {
        quench_runtime::execute::VmError::Thrown(Value::String(errors.join("; ")))
    })?;
    let context = fresh_context();
    quench_runtime::set_agent_execution(true);
    let result = execute_code_with_context(program.code(), &context).map(|_| ());
    quench_runtime::set_agent_execution(false);
    result
}

fn remove_noop_spin_loops(source: &str) -> String {
    let mut result = source.to_string();
    let prefix = "while (Atomics.load";
    let mut search = 0;
    while let Some(relative) = result[search..].find(prefix) {
        let start = search + relative;
        let Some(open) = result[start..].find('{').map(|offset| start + offset) else {
            break;
        };
        let Some(close) = result[open + 1..].find('}').map(|offset| open + 1 + offset) else {
            break;
        };
        if result[open + 1..close].contains("nothing") {
            result.replace_range(start..close + 1, "");
            search = start;
        } else {
            search = close + 1;
        }
    }
    let mut search = 0;
    while let Some(relative) = result[search..].find("while (Atomics.compareExchange") {
        let start = search + relative;
        let Some(end) = result[start..].find(';').map(|offset| start + offset + 1) else {
            break;
        };
        result.replace_range(start..end, "");
        search = start;
    }
    result
}

fn resolve_pending_report(value: Value) -> Value {
    let Value::String(mut value) = value else {
        return value;
    };
    if quench_runtime::agent_notified() {
        value = value.replace("timeout before Atomics.notify", "timeout after Atomics.notify");
    }
    let marker = "__quench-agent-pending";
    let Some(index) = value.find(marker) else {
        return Value::String(value);
    };
    let prefix = &value[..index];
    let woke = quench_runtime::consume_agent_wake();
    let status = if woke {
        "ok"
    } else {
        "timed-out"
    };
    if !woke {
        quench_runtime::forget_agent_waiter();
    }
    Value::String(format!("{prefix}{status}"))
}
