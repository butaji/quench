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
    SPEC_DOMAIN_DISPOSE, SPEC_DOMAIN_ENTER, SPEC_DOMAIN_EXIT, SPEC_DOMAIN_ON, SPEC_DOMAIN_REMOVE,
    SPEC_DOMAIN_RUN,
};

const ID: &str = "\0quench:domain:id";

struct DomainData {
    object: Value,
    members: Vec<Value>,
    handler: Option<Value>,
    disposed: bool,
}

pub struct DomainState {
    next_id: u64,
    stack: Vec<u64>,
    domains: HashMap<u64, DomainData>,
    module: Option<Value>,
}

impl DomainState {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            stack: Vec::new(),
            domains: HashMap::new(),
            module: None,
        }
    }
}

pub fn build(state: &Rc<RefCell<HostState>>) -> Value {
    let module = crate::host::namespace_object(vec![
        ("active", Value::Null),
        ("_stack", host_api::array(Vec::new())),
        ("Domain", crate::host::capability(SPEC_DOMAIN_CONSTRUCTOR)),
        ("create", crate::host::capability(SPEC_DOMAIN_CREATE)),
        ("createDomain", crate::host::capability(SPEC_DOMAIN_CREATE)),
    ])
    .unwrap_or(Value::Undefined);
    state.borrow_mut().domain.module = Some(module.clone());
    module
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
    let (module, stack, active) = {
        let host = state.borrow();
        (
            host.domain.module.clone(),
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
        let global = quench_runtime::vm::current_global_object();
        let process = execute::get_property(&global, "process");
        let _ = execute::set_property_in_place(&process, "domain", active.clone());
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
        host.domain.stack.retain(|entry| *entry != id);
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
    if let Some(pos) = host.domain.stack.iter().position(|entry| *entry == id) {
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
    let mut domain = with_domain(state, receiver)?;
    if !domain.disposed && !domain.members.iter().any(|v| *v == member) {
        domain.members.push(member.clone());
        let members = host_api::array(domain.members.clone());
        execute::set_property_in_place(receiver, "members", members);
        let _ = execute::set_property_in_place(&member, "domain", receiver.clone());
    }
    Ok(receiver.clone())
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
        let _ = execute::set_property_in_place(&member, "domain", Value::Undefined);
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
    enter(state, Some(receiver), &[])?;
    let result = execute::call(callback, &Value::Undefined, &[]);
    let handler = { with_domain(state, receiver)?.handler.clone() };
    let _ = exit(state, Some(receiver), &[])?;
    match result {
        Ok(value) => Ok(value),
        Err(VmError::Thrown(value)) => {
            let value = if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
                execute::define_property(
                    value,
                    "domain",
                    host_api::object(vec![
                        ("configurable".into(), Value::Boolean(true)),
                        ("enumerable".into(), Value::Boolean(false)),
                        ("writable".into(), Value::Boolean(true)),
                        ("value".into(), receiver.clone()),
                    ]),
                )?
            } else {
                value
            };
            let value = execute::set_property(value, "domainThrown", Value::Boolean(true));
            if let Some(handler) = handler {
                execute::call(&handler, &Value::Undefined, &[value])
            } else {
                Err(VmError::Thrown(value))
            }
        }
        Err(error) => Err(error),
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
        with_domain(state, receiver)?.handler = args.get(1).cloned();
    }
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
