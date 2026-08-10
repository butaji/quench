use crate::intl::tolocale::value::{is_finite, to_number, to_string};
use crate::ops::{
    Builtin, FunctionKind, FunctionStrictness, HostCapabilityKind, HostCapabilityRef, Op, RealmId,
};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

mod vm_arithmetic;
mod vm_ops;

pub use crate::intl::tolocale::value::is_truthy;

pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;
type ObjectProperties = Rc<Vec<(String, Value)>>;

#[derive(Clone)]
pub struct VmContext {
    output_sink: Option<OutputSink>,
    realm: RealmId,
    capabilities: Vec<HostCapabilityRef>,
}

impl Default for VmContext {
    fn default() -> Self {
        Self {
            output_sink: None,
            realm: RealmId::ROOT,
            capabilities: Vec::new(),
        }
    }
}

thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<VmContext>> = const { RefCell::new(None) };
    static GLOBAL_OBJECT: RefCell<Option<ObjectProperties>> = const { RefCell::new(None) };
}

struct ContextGuard {
    previous: Option<VmContext>,
}

impl ContextGuard {
    fn install(context: &VmContext) -> Self {
        let previous = CURRENT_CONTEXT.with(|current| current.replace(Some(context.clone())));
        Self { previous }
    }
}

impl Drop for ContextGuard {
    fn drop(&mut self) {
        CURRENT_CONTEXT.with(|current| current.replace(self.previous.take()));
    }
}

impl VmContext {
    pub fn with_output_sink(output_sink: OutputSink) -> Self {
        Self {
            output_sink: Some(output_sink),
            ..Self::default()
        }
    }

    pub fn for_realm(realm: RealmId, capabilities: Vec<HostCapabilityKind>) -> Self {
        let capabilities = capabilities
            .into_iter()
            .map(|kind| HostCapabilityRef { realm, kind })
            .collect();
        Self {
            realm,
            capabilities,
            ..Self::default()
        }
    }

    pub fn realm(&self) -> RealmId {
        self.realm
    }

    pub fn has_capability(&self, kind: HostCapabilityKind) -> bool {
        self.capabilities
            .iter()
            .any(|capability| capability.kind == kind)
    }

    pub(crate) fn permits(&self, capability: HostCapabilityRef) -> bool {
        capability.realm == self.realm && self.has_capability(capability.kind)
    }

    pub fn emit_output(&self, text: &str) {
        if let Some(output_sink) = &self.output_sink {
            output_sink(text);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VmError {
    RegisterOutOfBounds(u16),
    MissingReturn,
    Break(Option<String>),
    Continue(Option<String>),
    NotCallable,
    EvalError(String),
    Thrown(Value),
    Suspended(Rc<crate::value::PromiseData>),
}

impl VmError {
    pub fn render(&self) -> String {
        match self {
            VmError::Thrown(value) => render_thrown(value),
            VmError::Suspended(_) => "Suspended".to_string(),
            other => format!("{other:?}"),
        }
    }
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_context(ops, &VmContext::default())
}

pub fn execute_with_registers(ops: &[Op], registers: Vec<Value>) -> Result<Value, VmError> {
    execute_with_registers_context(ops, registers, &VmContext::default())
}

pub fn execute_in_place(ops: &[Op], registers: &mut Vec<Value>) -> Result<Value, VmError> {
    execute_in_place_context(ops, registers, &VmContext::default())
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    execute_with_registers_context(ops, Vec::new(), context)
}

pub fn execute_with_registers_context(
    ops: &[Op],
    mut registers: Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    execute_in_place_context(ops, &mut registers, context)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    for op in ops {
        match run_op(registers, op, context)? {
            None => {}
            Some(value) => return Ok(value),
        }
    }
    Err(VmError::MissingReturn)
}

struct GlobalObjectGuard {
    previous: Option<ObjectProperties>,
}

pub(crate) fn current_global_object() -> Value {
    GLOBAL_OBJECT
        .with(|global| {
            global
                .borrow()
                .as_ref()
                .map(|object| Value::Object(object.clone()))
        })
        .unwrap_or(Value::Undefined)
}

pub(crate) fn synchronize_global_object(registers: &mut Vec<Value>, old: &Value, new: &Value) {
    let (Value::Object(old_object), Value::Object(new_object)) = (old, new) else {
        return;
    };
    let is_global = GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|object| Rc::ptr_eq(object, old_object))
    });
    if !is_global {
        return;
    }
    GLOBAL_OBJECT.with(|global| global.replace(Some(new_object.clone())));
    for register in registers {
        if let Value::Object(object) = register {
            if Rc::ptr_eq(object, old_object) {
                *object = new_object.clone();
            }
        }
    }
}

pub(crate) fn bare_call_receiver(
    function: &crate::value::FunctionValue,
    this_value: &Value,
) -> Value {
    if matches!(this_value, Value::Undefined)
        && matches!(function.kind, FunctionKind::Ordinary)
        && matches!(function.strictness, FunctionStrictness::Sloppy)
    {
        current_global_object()
    } else {
        this_value.clone()
    }
}

impl GlobalObjectGuard {
    fn install() -> Self {
        let previous = GLOBAL_OBJECT.with(|global| {
            let previous = global.borrow().clone();
            if previous.is_none() {
                global.replace(None);
            }
            previous
        });
        Self { previous }
    }
}

impl Drop for GlobalObjectGuard {
    fn drop(&mut self) {
        GLOBAL_OBJECT.with(|global| global.replace(self.previous.take()));
    }
}

pub fn execute_builtin_with_receiver(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectPropertyIsEnumerable
    ) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = early_dispatch(builtin, receiver, arguments) {
        return result;
    }
    if is_data_view_builtin(builtin) {
        return execute_data_view_builtin(builtin, receiver, arguments);
    }
    if let Builtin::HostCapability(kind) = builtin {
        return vm_ops::execute_host_capability(kind, receiver, arguments);
    }
    match builtin {
        _ if is_function_builtin(builtin) => {
            crate::functions::function_builtin(builtin, receiver, arguments)
        }
        _ if is_simple_builtin(builtin) => execute_simple_builtin(builtin, arguments, receiver),
        _ => vm_ops::execute_builtin_tail(builtin, arguments, receiver),
    }
}

pub(crate) fn execute_host_capability(
    kind: HostCapabilityKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::HostCapability(capability)) = receiver else {
        return Err(VmError::NotCallable);
    };
    let permitted = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.permits(capability.descriptor))
    });
    if !permitted || capability.descriptor.kind != kind {
        return Err(VmError::NotCallable);
    }
    match kind {
        HostCapabilityKind::GetGlobal if arguments.is_empty() => current_global_value(),
        HostCapabilityKind::GetGlobal => Err(type_error("getGlobal expects no arguments")),
        HostCapabilityKind::DetachArrayBuffer => vm_ops::detach_array_buffer(arguments),
        _ => Err(VmError::EvalError(
            "Host capability is unavailable".to_string(),
        )),
    }
}

fn current_global_value() -> Result<Value, VmError> {
    GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .clone()
            .map(Value::Object)
            .ok_or_else(|| VmError::EvalError("Global object is unavailable".to_string()))
    })
}

fn execute_print(arguments: &[Value]) -> Result<Value, VmError> {
    let text = arguments
        .iter()
        .map(|value| to_string(Some(value)))
        .collect::<Vec<_>>()
        .join(" ");
    let context = CURRENT_CONTEXT.with(|current| current.borrow().clone());
    if let Some(context) = context {
        context.emit_output(&text);
    }
    Ok(Value::Undefined)
}

include!("vm_builtins.rs");
include!("vm_properties.rs");
include!("vm_dispatch.rs");
