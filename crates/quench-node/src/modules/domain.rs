//! Rust-owned legacy `domain` API. State is explicit; methods are host facts.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::registry::{
    SPEC_DOMAIN_ADD, SPEC_DOMAIN_ADD_EMITTER, SPEC_DOMAIN_CONSTRUCTOR, SPEC_DOMAIN_CREATE,
    SPEC_DOMAIN_DISPOSE, SPEC_DOMAIN_ENTER, SPEC_DOMAIN_EXIT, SPEC_DOMAIN_ON, SPEC_DOMAIN_ONCE,
    SPEC_DOMAIN_BIND, SPEC_DOMAIN_BIND_CALL, SPEC_DOMAIN_INTERCEPT, SPEC_DOMAIN_INTERCEPT_CALL,
    SPEC_DOMAIN_REMOVE, SPEC_DOMAIN_RUN,
};

const ID: &str = "\0quench:domain:id";
pub(crate) const PROMISE_DOMAIN: &str = "\0quench:domain:promise";
pub(crate) const HANDLER_DOMAIN: &str = "\0quench:domain:handler";

struct DomainData {
    object: Value,
    members: Vec<Value>,
    handler: Option<Value>,
    extra_handlers: Vec<(Value, bool)>,
    disposed: bool,
}

pub struct DomainState {
    next_id: u64,
    stack: Vec<u64>,
    domains: HashMap<u64, DomainData>,
    module: Option<Value>,
    process: Option<Value>,
}

impl DomainState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            stack: Vec::new(),
            domains: HashMap::new(),
            module: None,
            process: None,
        }
    }
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    install_promise_bridge();
    let module = crate::host::namespace_object(vec![
        ("active", Value::Null),
        ("_stack", host_api::array(Vec::new())),
        ("Domain", crate::host::capability(SPEC_DOMAIN_CONSTRUCTOR)),
        ("create", crate::host::capability(SPEC_DOMAIN_CREATE)),
        ("createDomain", crate::host::capability(SPEC_DOMAIN_CREATE)),
    ])
    .unwrap_or(Value::Undefined);
    let process = execute::get_property(&quench_runtime::vm::current_global_object(), "process");
    let mut host = state.borrow_mut();
    host.domain.module = Some(module.clone());
    if matches!(process, Value::Object(_) | Value::ObjectAlias(_)) {
        host.domain.process = Some(process);
    }
    module
}

pub(crate) const PROMISE_BRIDGE_SOURCE: &str = r#"(() => {
      const promiseDomain = "\0quench:domain:promise";
      const tag = (promise) => {
        const domain = globalThis.process?.domain;
        if (domain) Object.defineProperty(promise, promiseDomain, {
          configurable: true,
          value: domain,
        });
        return promise;
      };
      const originalResolve = Promise.resolve;
      const originalReject = Promise.reject;
      const originalThen = Promise.prototype.then;
      const wrap = (callback) => {
        const domain = globalThis.process?.domain;
        if (typeof callback !== "function" || !domain) return callback;
        return (...args) => domain.run(() => callback(...args));
      };
      Object.defineProperty(Promise.prototype, "then", {
        configurable: true,
        writable: true,
        value: function(onFulfilled, onRejected) {
          return tag(originalThen.call(this, wrap(onFulfilled), wrap(onRejected)));
        },
      });
      Object.defineProperty(Promise, "resolve", {
        configurable: true,
        writable: true,
        value: function(value) { return tag(originalResolve.call(this, value)); },
      });
      Object.defineProperty(Promise, "reject", {
        configurable: true,
        writable: true,
        value: function(value) { return tag(originalReject.call(this, value)); },
      });
      Object.defineProperty(globalThis, "__quench_domain_promises_patched", {
        configurable: true,
        value: true,
      });
    })()"#;

fn install_promise_bridge() {
    let global = quench_runtime::vm::current_global_object();
    if matches!(
        execute::get_property(&global, "__quench_domain_promises_patched"),
        Value::Boolean(true)
    ) {
        return;
    }
    let source = PROMISE_BRIDGE_SOURCE;
    let Ok(program) = quench_runtime::reduce::reduce_global_script_source(source) else {
        return;
    };
    let context = quench_runtime::vm::current_context();
    let mut registers = quench_runtime::register_file::RegisterFile::new();
    let _ = quench_runtime::vm::with_current_context(&context, || {
        quench_runtime::vm::execute_code_in_place_context(program.code(), &mut registers, &context)
    });
}

pub fn create(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    new_domain(state, &[])
}
pub fn new_domain(state: &Rc<RefCell<HostState>>, _: &[Value]) -> Result<Value, VmError> {
    let mut host = state.borrow_mut();
    let id = host.domain.next_id;
    host.domain.next_id += 1;
    let object = domain_object(id);
    host.domain.domains.insert(
        id,
        DomainData {
            object: object.clone(),
            members: Vec::new(),
            handler: None,
            extra_handlers: Vec::new(),
            disposed: false,
        },
    );
    Ok(object)
}

fn domain_object(id: u64) -> Value {
    host_api::object(vec![
        (ID.into(), Value::Number(id as f64)),
        ("members".into(), host_api::array(Vec::new())),
        ("active".into(), Value::Boolean(false)),
        ("disposed".into(), Value::Boolean(false)),
        ("enter".into(), crate::host::capability(SPEC_DOMAIN_ENTER)),
        ("exit".into(), crate::host::capability(SPEC_DOMAIN_EXIT)),
        ("add".into(), crate::host::capability(SPEC_DOMAIN_ADD)),
        ("remove".into(), crate::host::capability(SPEC_DOMAIN_REMOVE)),
        ("run".into(), crate::host::capability(SPEC_DOMAIN_RUN)),
        (
            "dispose".into(),
            crate::host::capability(SPEC_DOMAIN_DISPOSE),
        ),
        ("on".into(), crate::host::capability(SPEC_DOMAIN_ON)),
        ("once".into(), crate::host::capability(SPEC_DOMAIN_ONCE)),
        ("bind".into(), crate::host::capability(SPEC_DOMAIN_BIND)),
        (
            "intercept".into(),
            crate::host::capability(SPEC_DOMAIN_INTERCEPT),
        ),
        (
            "addEmitter".into(),
            crate::host::capability(SPEC_DOMAIN_ADD_EMITTER),
        ),
    ])
}

fn id(value: &Value) -> Result<u64, VmError> {
    execute::get_property_result(value, ID)
        .ok()
        .and_then(|v| match v {
            Value::Number(n) => Some(n as u64),
            _ => None,
        })
        .ok_or_else(|| type_error("domain"))
}
fn with_domain<'a>(
    state: &'a Rc<RefCell<HostState>>,
    value: &Value,
) -> Result<std::cell::RefMut<'a, DomainData>, VmError> {
    let id = id(value)?;
    std::cell::RefMut::filter_map(state.borrow_mut(), |host| host.domain.domains.get_mut(&id))
        .map_err(|_| type_error("domain"))
}
fn refresh(state: &Rc<RefCell<HostState>>) {
    let (module, process, stack, active) = {
        let host = state.borrow();
        (
            host.domain.module.clone(),
            host.domain.process.clone(),
            host.domain.stack.clone(),
            host.domain.stack.last().copied(),
        )
    };
    if let Some(module) = module {
        let values: Vec<Value> = {
            let host = state.borrow();
            stack
                .iter()
                .filter_map(|id| {
                    host.domain
                        .domains
                        .get(id)
                        .map(|domain| domain.object.clone())
                })
                .collect()
        };
        let _ = execute::set_property_in_place(&module, "_stack", host_api::array(values.clone()));
        let active = active
            .and_then(|id| {
                let host = state.borrow();
                host.domain
                    .domains
                    .get(&id)
                    .map(|domain| domain.object.clone())
            })
            .unwrap_or(Value::Null);
        let _ = execute::set_property_in_place(&module, "active", active.clone());
        let process = process.unwrap_or_else(|| {
            execute::get_property(&quench_runtime::vm::current_global_object(), "process")
        });
        let global = quench_runtime::vm::current_global_object();
        let process_domain = if matches!(active, Value::Null) {
            Value::Undefined
        } else {
            active.clone()
        };
        let _ = execute::set_property_in_place(&process, "domain", process_domain);
        let hidden = |value| {
            host_api::object(vec![
                ("configurable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(false)),
                ("writable".into(), Value::Boolean(true)),
                ("value".into(), value),
            ])
        };
        let _ = execute::define_property(
            global.clone(),
            "__quench_active_domain",
            hidden(active.clone()),
        );
        let _ = execute::set_property_in_place(&global, "__quench_active_domain", active);
        let stack = host_api::array(values);
        let _ = execute::define_property(
            global.clone(),
            "__quench_domain_stack",
            hidden(stack.clone()),
        );
        let _ = execute::set_property_in_place(&global, "__quench_domain_stack", stack);
    }
}

pub fn enter(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let id = id(receiver)?;
    let disposed = { with_domain(state, receiver)?.disposed };
    if !disposed {
        let mut host = state.borrow_mut();
        host.domain.stack.push(id);
        drop(host);
        refresh(state);
    }
    Ok(receiver.clone())
}
pub fn exit(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let id = id(receiver)?;
    let mut host = state.borrow_mut();
    if let Some(pos) = host.domain.stack.iter().rposition(|entry| *entry == id) {
        // Exiting a domain unwinds it and every nested domain entered after
        // it; the stack is a dynamic context, not a set of independent IDs.
        host.domain.stack.truncate(pos);
    }
    drop(host);
    refresh(state);
    Ok(receiver.clone())
}
pub fn add(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let member = args.first().cloned().unwrap_or(Value::Undefined);
    let _ = attach_member(state, receiver, member)?;
    Ok(receiver.clone())
}

pub(crate) fn attach_member(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    member: Value,
) -> Result<Value, VmError> {
    let mut domain = with_domain(state, receiver)?;
    if !domain.disposed && !domain.members.iter().any(|v| *v == member) {
        let member = execute::define_property(
            member.clone(),
            "domain",
            host_api::object(vec![
                ("configurable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(false)),
                ("writable".into(), Value::Boolean(true)),
                ("value".into(), receiver.clone()),
            ]),
        )?;
        domain.members.push(member);
        let members = host_api::array(domain.members.clone());
        execute::set_property_in_place(receiver, "members", members);
        return Ok(domain.members.last().cloned().unwrap_or(Value::Undefined));
    }
    Ok(member)
}
pub fn remove(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let member = args.first().unwrap_or(&Value::Undefined);
    let mut domain = with_domain(state, receiver)?;
    if let Some(i) = domain.members.iter().position(|v| v == member) {
        let member = domain.members.remove(i);
        let members = host_api::array(domain.members.clone());
        execute::set_property_in_place(receiver, "members", members);
        let (updated, _) = execute::delete_property(member.clone(), "domain");
        execute::replace_value(&member, &updated);
    }
    Ok(receiver.clone())
}
pub fn run(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let callback = args.first().ok_or_else(|| type_error("function"))?;
    run_callback(
        state,
        receiver,
        callback,
        &Value::Undefined,
        args.get(1..).unwrap_or(&[]),
    )
}

fn run_callback(
    state: &Rc<RefCell<HostState>>,
    receiver: &Value,
    callback: &Value,
    this: &Value,
    args: &[Value],
) -> Result<Value, VmError> {
    enter(state, Some(receiver), &[])?;
    let result = execute::call(callback, this, args);
    let handlers = {
        let domain = with_domain(state, receiver)?;
        let mut handlers = Vec::with_capacity(domain.extra_handlers.len() + 1);
        if let Some(handler) = &domain.handler {
            handlers.push((handler.clone(), false));
        }
        handlers.extend(domain.extra_handlers.iter().cloned());
        handlers
    };
    let _ = exit(state, Some(receiver), &[])?;
    match result {
        Ok(value) => Ok(value),
        Err(VmError::Thrown(value)) => {
            if matches!(
                execute::get_property_result(&value, HANDLER_DOMAIN),
                Ok(origin) if execute::same_value(&origin, receiver)
            ) {
                return Err(VmError::Thrown(value));
            }
            let value = mark_error(&value, receiver, &Value::Undefined, true)?;
            if !handlers.is_empty() {
                let mut result = Ok(Value::Undefined);
                for (handler, once) in &handlers {
                    result =
                        execute::call(handler, &Value::Undefined, std::slice::from_ref(&value));
                    if result.is_err() {
                        break;
                    }
                    if *once {
                        let mut domain = with_domain(state, receiver)?;
                        domain
                            .extra_handlers
                            .retain(|(candidate, _)| !execute::same_value(candidate, handler));
                    }
                }
                result
            } else {
                Err(VmError::Thrown(value))
            }
        }
        Err(error) => Err(error),
    }
}

pub fn bind(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let domain = receiver.ok_or_else(|| type_error("domain"))?;
    let callback = args.first().ok_or_else(|| type_error("function"))?;
    if !quench_runtime::is_callable(callback) {
        return Err(type_error("function"));
    }
    Ok(host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_DOMAIN_BIND_CALL),
        vec![domain.clone(), callback.clone()],
    ))
}

pub fn bind_call(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let domain = args.first().ok_or_else(|| type_error("domain"))?;
    let callback = args.get(1).ok_or_else(|| type_error("function"))?;
    run_callback(
        state,
        domain,
        callback,
        receiver.unwrap_or(&Value::Undefined),
        args.get(2..).unwrap_or(&[]),
    )
}

pub fn intercept(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let domain = receiver.ok_or_else(|| type_error("domain"))?;
    let callback = args.first().ok_or_else(|| type_error("function"))?;
    if !quench_runtime::is_callable(callback) {
        return Err(type_error("function"));
    }
    Ok(host_api::bound_capability_with_arguments(
        crate::host::capability_ref(SPEC_DOMAIN_INTERCEPT_CALL),
        vec![domain.clone(), callback.clone()],
    ))
}

pub fn intercept_call(
    state: &Rc<RefCell<HostState>>,
    _: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let domain = args.first().ok_or_else(|| type_error("domain"))?;
    let callback = args.get(1).ok_or_else(|| type_error("function"))?;
    let error = args.get(2).cloned().unwrap_or(Value::Undefined);
    if !matches!(error, Value::Undefined | Value::Null) {
        let error = mark_error(&error, domain, callback, false)?;
        return match error_handler(state, domain) {
            Some(handler) => execute::call(&handler, &Value::Undefined, &[error]),
            None => Err(VmError::Thrown(error)),
        };
    }
    run_callback(
        state,
        domain,
        callback,
        &Value::Undefined,
        args.get(3..).unwrap_or(&[]),
    )
}

fn mark_error(
    error: &Value,
    domain: &Value,
    callback: &Value,
    thrown: bool,
) -> Result<Value, VmError> {
    let value = if matches!(error, Value::Object(_) | Value::ObjectAlias(_)) {
        execute::define_property(
            error.clone(),
            "domain",
            host_api::object(vec![
                ("configurable".into(), Value::Boolean(true)),
                ("enumerable".into(), Value::Boolean(false)),
                ("writable".into(), Value::Boolean(true)),
                ("value".into(), domain.clone()),
            ]),
        )?
    } else {
        error.clone()
    };
    let value = execute::set_property(value, "domainThrown", Value::Boolean(thrown));
    if thrown {
        Ok(value)
    } else {
        Ok(execute::set_property(value, "domainBound", callback.clone()))
    }
}
pub fn dispose(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    _: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let mut domain = with_domain(state, receiver)?;
    domain.disposed = true;
    domain.members.clear();
    drop(domain);
    let _ = exit(state, Some(receiver), &[])?;
    Ok(receiver.clone())
}
pub fn on(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    if matches!(args.first(), Some(Value::String(name)) if name == "error") {
        let mut domain = with_domain(state, receiver)?;
        if let Some(listener) = args.get(1).cloned() {
            domain.handler = Some(listener);
        }
    }
    Ok(receiver.clone())
}

pub fn once(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let receiver = receiver.ok_or_else(|| type_error("domain"))?;
    let listener = args.get(1).ok_or_else(|| type_error("function"))?;
    if !quench_runtime::is_callable(listener) {
        return Err(type_error("function"));
    }
    let mut domain = with_domain(state, receiver)?;
    domain.extra_handlers.push((listener.clone(), true));
    Ok(receiver.clone())
}

pub fn current(state: &Rc<RefCell<HostState>>) -> Option<Value> {
    let domain = {
        let host = state.borrow();
        host.domain.stack.last().and_then(|id| {
            host.domain
                .domains
                .get(id)
                .map(|domain| domain.object.clone())
        })
    };
    if let Some(domain) = domain {
        return Some(domain);
    }
    let process = execute::get_property(&quench_runtime::vm::current_global_object(), "process");
    match execute::get_property(&process, "domain") {
        Value::Object(_) => Some(execute::get_property(&process, "domain")),
        _ => None,
    }
}

pub fn error_handler(state: &Rc<RefCell<HostState>>, domain: &Value) -> Option<Value> {
    let id = id(domain).ok()?;
    state
        .borrow()
        .domain
        .domains
        .get(&id)
        .and_then(|entry| entry.handler.clone())
}

pub(crate) fn promise_domain(
    state: &Rc<RefCell<HostState>>,
    promise: &Value,
) -> Option<(Value, Value)> {
    let domain = execute::get_property(promise, PROMISE_DOMAIN);
    let handler = error_handler(state, &domain)?;
    Some((domain, handler))
}

pub(crate) fn stack_values(state: &Rc<RefCell<HostState>>) -> Vec<Value> {
    let host = state.borrow();
    host.domain
        .stack
        .iter()
        .filter_map(|id| host.domain.domains.get(id).map(|domain| domain.object.clone()))
        .collect()
}

pub(crate) fn replace_stack(state: &Rc<RefCell<HostState>>, values: &[Value]) {
    let stack = values.iter().filter_map(|value| id(value).ok()).collect();
    state.borrow_mut().domain.stack = stack;
    refresh(state);
}

pub(crate) fn call_error_handler(
    state: &Rc<RefCell<HostState>>,
    domain: &Value,
    handler: &Value,
    error: &Value,
) -> Result<Value, VmError> {
    let id = id(domain)?;
    let previous = {
        let mut host = state.borrow_mut();
        let previous = host.domain.stack.clone();
        while host.domain.stack.last() == Some(&id) {
            host.domain.stack.pop();
        }
        previous
    };
    refresh(state);
    let result = execute::call(handler, domain, std::slice::from_ref(error));
    state.borrow_mut().domain.stack = previous;
    refresh(state);
    result
}
pub fn add_emitter(
    state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    add(state, receiver, args)
}
fn type_error(name: &str) -> VmError {
    VmError::Thrown(host_api::object(vec![
        ("name".into(), Value::String("TypeError".into())),
        (
            "message".into(),
            Value::String(format!("The {name} argument must be valid")),
        ),
    ]))
}
