use crate::intl::tolocale::value::{is_finite, to_number, to_string};
use crate::ops::{
    Builtin, FunctionKind, FunctionStrictness, HostCapabilityKind, HostCapabilityRef, Op, RealmId,
};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
mod realm;
mod vm_arithmetic;
mod vm_ops;
mod vm_typed_bigint;
pub use crate::intl::tolocale::value::is_truthy;
pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;
type ObjectProperties = Rc<crate::value::ObjectData>;
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
    Yield(Value),
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
            VmError::NotCallable => "TypeError: value is not callable".to_string(),
            other => format!("{other:?}"),
        }
    }
}

pub fn execute(ops: &[Op]) -> Result<Value, VmError> {
    execute_with_context(ops, &VmContext::default())
}

pub fn execute_with_registers(ops: &[Op], registers: Vec<Value>) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_with_registers_context(ops, registers, &context)
}

pub fn execute_in_place(ops: &[Op], registers: &mut Vec<Value>) -> Result<Value, VmError> {
    let context = current_context_or_default();
    execute_in_place_context(ops, registers, &context)
}

pub(crate) fn current_context_or_default() -> VmContext {
    CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_default()
}

pub(crate) fn execute_completion_in_place(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    execute_completion_in_place_context(ops, registers, &context)
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
    preserve_frame_completion(run_ops_completion(ops, registers, context)?)
}

fn preserve_frame_completion(
    completion: crate::completion::Completion,
) -> Result<crate::completion::Completion, VmError> {
    use crate::completion::Completion;
    Ok(match completion {
        Completion::TailCall(request) => tail_call_completion(request),
        completion => completion,
    })
}

fn tail_call_completion(
    request: crate::completion::TailCallRequest,
) -> crate::completion::Completion {
    crate::completion::Completion::TailCall(request)
}

pub(crate) fn execute_in_environment(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<Value, VmError> {
    completion_result(execute_frame_completion(
        ops,
        registers,
        context,
        environment,
    )?)
}

pub(crate) fn execute_frame_completion(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
) -> Result<crate::completion::Completion, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    run_ops_completion(ops, registers, context)
}
pub(crate) fn execute_indirect_eval(ops: &[Op]) -> Result<Value, VmError> {
    let context = CURRENT_CONTEXT
        .with(|current| current.borrow().clone())
        .unwrap_or_default();
    if realm::context(context.realm()).is_some() {
        return execute_indirect_eval_in_realm(context.realm(), ops);
    }
    let global = current_global_object();
    let caller = crate::locals::current();
    let environment = crate::environment::Environment::new();
    environment.set(0, global.clone());
    let mut registers = Vec::new();
    let _with_scope = crate::with_scope::FunctionGuard::isolate();
    let result = execute_in_environment(ops, &mut registers, &context, environment);
    caller.replace_value(&global, &current_global_object());
    result
}
pub(crate) fn execute_indirect_eval_in_realm(
    realm_id: RealmId,
    ops: &[Op],
) -> Result<Value, VmError> {
    realm::execute(realm_id, ops)
}

include!("vm_generator_step.rs");

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
            Err(error) => {
                crate::vm::flush_global_declaration_batch(registers);
                return error_completion(error);
            }
        };
        match result {
            None | Some(crate::completion::Completion::Normal) => {}
            Some(completion) => {
                crate::vm::flush_global_declaration_batch(registers);
                return Ok(completion);
            }
        }
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok(crate::completion::Completion::Normal)
}

fn error_completion(error: VmError) -> Result<crate::completion::Completion, VmError> {
    use crate::completion::Completion;
    match error {
        VmError::Thrown(value) => Ok(Completion::Throw(value)),
        VmError::Break(label) => Ok(Completion::Break(label)),
        VmError::Continue(label) => Ok(Completion::Continue(label)),
        VmError::Suspended(promise) => Ok(Completion::Suspend(promise)),
        VmError::Yield(value) => Ok(Completion::Yield(value)),
        VmError::NotCallable => {
            if let VmError::Thrown(value) = not_callable() {
                Ok(Completion::Throw(value))
            } else {
                Ok(Completion::Throw(crate::builtins::error(
                    Builtin::TypeError,
                    &[Value::String("value is not callable".to_string())],
                )))
            }
        }
        error => Err(error),
    }
}

pub(crate) fn completion_result(
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    use crate::completion::Completion;
    match completion {
        Completion::Normal => Err(VmError::MissingReturn),
        Completion::Return(value) => Ok(value),
        Completion::TailCall(_) => Err(VmError::EvalError(
            "Unconsumed tail-call completion".to_string(),
        )),
        Completion::Throw(value) => Err(VmError::Thrown(value)),
        Completion::Break(label) => Err(VmError::Break(label)),
        Completion::Continue(label) => Err(VmError::Continue(label)),
        Completion::Suspend(promise) => Err(VmError::Suspended(promise)),
        Completion::Yield(value) => Err(VmError::Yield(value)),
    }
}

struct GlobalObjectGuard {
    previous: Option<ObjectProperties>,
    restore: bool,
    realm: Option<RealmId>,
}
include!("vm_global.rs");

pub(crate) fn bare_call_receiver(
    function: &crate::value::FunctionValue,
    this_value: &Value,
) -> Value {
    if matches!(function.kind, FunctionKind::Ordinary)
        && matches!(function.strictness, FunctionStrictness::Sloppy)
    {
        if matches!(this_value, Value::Undefined | Value::Null) {
            return current_global_object();
        }
        return to_object_value(this_value);
    }
    this_value.clone()
}

fn to_object_value(this_value: &Value) -> Value {
    match this_value {
        Value::Object(_)
        | Value::Array(_)
        | Value::Function(_)
        | Value::BoundFunction(_)
        | Value::Builtin(_)
        | Value::ObjectAlias(_)
        | Value::Proxy(_)
        | Value::Promise(_)
        | Value::Map(_)
        | Value::Set(_)
        | Value::ArrayBuffer(_)
        | Value::DataView(_)
        | Value::Float32Array(_)
        | Value::Float64Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::Uint8Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_)
        | Value::Uint32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Iterator(_)
        | Value::Generator(_)
        | Value::HostCapability(_) => this_value.clone(),
        Value::Number(_) => boxed_primitive(this_value, crate::ops::Builtin::Number),
        Value::Boolean(_) => boxed_primitive(this_value, crate::ops::Builtin::Boolean),
        Value::String(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::BigInt(_) => boxed_primitive(this_value, crate::ops::Builtin::BigInt),
        Value::Null | Value::Undefined | Value::BindingCell(_) => this_value.clone(),
    }
}

fn boxed_primitive(value: &Value, constructor: crate::ops::Builtin) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("_value".to_string(), value.clone()),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ])))
}

pub fn execute_builtin_with_receiver(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = stateful_builtin(builtin, receiver, arguments) {
        return result;
    }
    if builtin == Builtin::Print {
        return execute_print(arguments);
    }
    if is_object_special(builtin) {
        return crate::builtins::object::execute_special(builtin, receiver, arguments);
    }
    if let Some(result) = define_builtin(builtin, arguments) {
        return result;
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

fn define_builtin(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::ObjectDefineProperty => Some(crate::builtins::define_property(arguments)),
        Builtin::ObjectDefineProperties => Some(crate::builtins::define_properties(arguments)),
        _ => None,
    }
}

fn stateful_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::GeneratorNext => Some(crate::generator::next(receiver, arguments)),
        Builtin::GeneratorReturn => Some(crate::generator::return_(receiver, arguments)),
        Builtin::GeneratorThrow => Some(crate::generator::throw(receiver, arguments)),
        Builtin::ProxyRevoke => Some(crate::proxy::revoke(receiver)),
        Builtin::Math => Some(Err(not_callable())),
        _ => None,
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
            | Builtin::ObjectAssign
            | Builtin::ObjectCreate
            | Builtin::ObjectSetPrototypeOf
            | Builtin::ObjectPropertyIsEnumerable
            | Builtin::ObjectPrototypeIsPrototypeOf
            | Builtin::ObjectPrototypeDefineGetter
            | Builtin::ObjectPrototypeDefineSetter
            | Builtin::ObjectPrototypeLookupGetter
            | Builtin::ObjectPrototypeLookupSetter
    )
}

include!("vm_host.rs");
include!("vm_boolean_value.rs");
include!("vm_builtins.rs");
include!("vm_properties.rs");
include!("vm_dispatch.rs");
