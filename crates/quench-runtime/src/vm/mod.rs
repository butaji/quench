use crate::intl::tolocale::value::{is_finite, to_number, to_string};
use crate::ops::{
    Builtin, FunctionKind, FunctionStrictness, HostCapabilityKind, HostCapabilityRef, Op, RealmId,
};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

mod vm_arithmetic;
mod vm_ops;

pub use crate::intl::tolocale::value::is_truthy;

pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;
type ObjectProperties = Rc<Vec<(String, Value)>>;
static NEXT_REALM: AtomicU64 = AtomicU64::new(1);

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

pub(crate) fn not_callable() -> VmError {
    VmError::Thrown(crate::builtins::error(
        Builtin::TypeError,
        &[Value::String("value is not callable".to_string())],
    ))
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

pub(crate) fn execute_completion_in_place(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<crate::completion::Completion, VmError> {
    execute_completion_in_place_context(ops, registers, &VmContext::default())
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    execute_with_registers_context(ops, Vec::new(), context)
}

pub fn execute_with_registers_context(
    ops: &[Op],
    mut registers: Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    let environment = crate::environment::Environment::child(
        &crate::environment::Environment::new(),
        registers.clone(),
    );
    execute_in_environment(ops, &mut registers, context, environment)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    if crate::locals::is_installed() {
        return run_ops(ops, registers, context);
    }
    let environment = crate::environment::Environment::child(
        &crate::environment::Environment::new(),
        registers.clone(),
    );
    execute_in_environment(ops, registers, context, environment)
}

fn execute_completion_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    if crate::locals::is_installed() {
        return run_ops_completion(ops, registers, context);
    }
    let environment = crate::environment::Environment::child(
        &crate::environment::Environment::new(),
        registers.clone(),
    );
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    run_ops_completion(ops, registers, context)
}

pub(crate) fn execute_in_environment(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    run_ops(ops, registers, context)
}

fn run_ops(ops: &[Op], registers: &mut Vec<Value>, context: &VmContext) -> Result<Value, VmError> {
    completion_result(run_ops_completion(ops, registers, context)?)
}

fn run_ops_completion(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    for op in ops {
        let result = match run_op(registers, op, context) {
            Ok(result) => result,
            Err(error) => return error_completion(error),
        };
        match result {
            None | Some(crate::completion::Completion::Normal) => {}
            Some(completion) => return Ok(completion),
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn error_completion(error: VmError) -> Result<crate::completion::Completion, VmError> {
    use crate::completion::Completion;
    match error {
        VmError::Thrown(value) => Ok(Completion::Throw(value)),
        VmError::Break(label) => Ok(Completion::Break(label)),
        VmError::Continue(label) => Ok(Completion::Continue(label)),
        VmError::Suspended(promise) => Ok(Completion::Suspend(promise)),
        error => Err(error),
    }
}

fn completion_result(completion: crate::completion::Completion) -> Result<Value, VmError> {
    use crate::completion::Completion;
    match completion {
        Completion::Normal => Err(VmError::MissingReturn),
        Completion::Return(value) => Ok(value),
        Completion::Throw(value) => Err(VmError::Thrown(value)),
        Completion::Break(label) => Err(VmError::Break(label)),
        Completion::Continue(label) => Err(VmError::Continue(label)),
        Completion::Suspend(promise) => Err(VmError::Suspended(promise)),
    }
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

pub(crate) fn is_global_object(value: &Value) -> bool {
    let Value::Object(object) = value else {
        return false;
    };
    GLOBAL_OBJECT.with(|global| {
        global
            .borrow()
            .as_ref()
            .is_some_and(|global| Rc::ptr_eq(global, object))
    })
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
    if builtin == Builtin::GeneratorNext {
        return crate::generator::next(receiver);
    }
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if is_object_special(builtin) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if builtin == Builtin::ObjectDefineProperty {
        return crate::builtins::define_property(arguments);
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

fn is_object_special(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertySymbols
            | Builtin::ObjectKeys
            | Builtin::ObjectPropertyIsEnumerable
    )
}

pub(crate) fn execute_host_capability(
    kind: HostCapabilityKind,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
    let Some(Value::HostCapability(capability)) = receiver else {
        return Err(VmError::NotCallable);
    };
    let descriptor = HostCapabilityRef {
        realm: capability.realm(),
        kind,
    };
    let permitted = CURRENT_CONTEXT.with(|context| {
        context
            .borrow()
            .as_ref()
            .is_some_and(|context| context.permits(descriptor))
    });
    if !permitted {
        return Err(VmError::NotCallable);
    }
    match kind {
        HostCapabilityKind::GetGlobal if arguments.is_empty() => current_global_value(),
        HostCapabilityKind::GetGlobal => Err(type_error("getGlobal expects no arguments")),
        HostCapabilityKind::CreateRealm if arguments.is_empty() => Ok(create_realm_value()),
        HostCapabilityKind::CreateRealm => Err(type_error("createRealm expects no arguments")),
        HostCapabilityKind::DetachArrayBuffer => vm_ops::detach_array_buffer(arguments),
        _ => Err(VmError::EvalError(
            "Host capability is unavailable".to_string(),
        )),
    }
}

fn create_realm_value() -> Value {
    let realm = RealmId::new(NEXT_REALM.fetch_add(1, Ordering::Relaxed));
    let capability = Value::HostCapability(Rc::new(crate::value::HostCapabilityValue::new(
        HostCapabilityRef {
            realm,
            kind: HostCapabilityKind::GetGlobal,
        },
    )));
    let constructor = Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        target: Value::Builtin(Builtin::TypeError),
        receiver: capability,
        arguments: Vec::new(),
    }));
    let global = Value::Object(Rc::new(vec![("TypeError".to_string(), constructor)]));
    Value::Object(Rc::new(vec![("global".to_string(), global)]))
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
