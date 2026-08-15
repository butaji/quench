use crate::intl::tolocale::value::{is_finite, to_string};
use crate::ops::{
    Builtin, FunctionKind, FunctionStrictness, HostCapabilityKind, HostCapabilityRef, Op, RealmId,
};
use crate::value::Value;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
pub(crate) mod realm;
mod scope;
mod vm_arithmetic;
pub(crate) mod vm_ops;
mod vm_typed_bigint;

pub fn reset_host_agent_state() {
    reset_agent_state();
}
pub use crate::intl::tolocale::value::is_truthy;
pub use scope::ExecutionScope;
pub type OutputSink = Arc<dyn Fn(&str) + Send + Sync>;

thread_local! {
    static ERROR_REALM: Cell<Option<RealmId>> = const { Cell::new(None) };
}

pub(crate) fn with_error_realm<T>(realm: RealmId, callback: impl FnOnce() -> T) -> T {
    let previous = ERROR_REALM.with(|slot| slot.replace(Some(realm)));
    let result = callback();
    ERROR_REALM.with(|slot| slot.set(previous));
    result
}

pub(crate) fn current_error_realm() -> Option<RealmId> {
    ERROR_REALM.with(Cell::get)
}

pub(crate) fn with_realm<T>(realm: RealmId, callback: impl FnOnce() -> T) -> Option<T> {
    realm::with_realm(realm, callback)
}

pub(crate) fn global_builtin_exists(key: &str) -> bool {
    realm::global_builtin_exists(key)
}

pub(crate) fn global_builtin_value(key: &str) -> Option<Value> {
    crate::globals::builtin(key).map(realm_intrinsic)
}

pub(crate) fn intrinsic_for_realm(realm: RealmId, builtin: Builtin) -> Value {
    realm::intrinsic(realm, builtin).unwrap_or(Value::Builtin(builtin))
}
type ObjectProperties = Rc<crate::value::ObjectData>;
type DeferredCallback = Rc<dyn Fn(u32) -> Result<Value, VmError>>;
#[derive(Clone)]
pub struct VmContext {
    output_sink: Option<OutputSink>,
    realm: RealmId,
    can_block: bool,
    capabilities: Vec<HostCapabilityRef>,
    host_bindings: Vec<(String, HostCapabilityRef)>,
}
impl Default for VmContext {
    fn default() -> Self {
        Self {
            output_sink: None,
            realm: RealmId::ROOT,
            can_block: true,
            capabilities: Vec::new(),
            host_bindings: Vec::new(),
        }
    }
}
thread_local! {
    static CURRENT_CONTEXT: RefCell<Option<VmContext>> = const { RefCell::new(None) };
    static GLOBAL_OBJECT: RefCell<Option<ObjectProperties>> = const { RefCell::new(None) };
    static DEFERRED_MODULE_CALLBACK: RefCell<Option<DeferredCallback>> = const { RefCell::new(None) };
    static DYNAMIC_IMPORT_CALLBACK: RefCell<Option<DynamicImportCallback>> = const { RefCell::new(None) };
    static DEFERRED_MODULE_DEPTH: Cell<u32> = const { Cell::new(0) };
    static ASYNC_MODULE_STEP: Cell<bool> = const { Cell::new(false) };
}

pub struct AsyncModuleStepGuard {
    previous: bool,
}

pub fn install_async_module_step() -> AsyncModuleStepGuard {
    let previous = ASYNC_MODULE_STEP.with(|slot| slot.replace(true));
    AsyncModuleStepGuard { previous }
}

pub(crate) fn async_module_step() -> bool {
    ASYNC_MODULE_STEP.with(Cell::get)
}

pub struct DeferredModuleCallbackGuard {
    previous: Option<DeferredCallback>,
}

pub type DynamicImportCallback = Rc<dyn Fn(String, bool, Value) -> Result<Value, VmError>>;

pub struct DynamicImportCallbackGuard {
    previous: Option<DynamicImportCallback>,
}

pub fn install_deferred_module_callback(callback: DeferredCallback) -> DeferredModuleCallbackGuard {
    let previous = DEFERRED_MODULE_CALLBACK.with(|slot| slot.replace(Some(callback)));
    DeferredModuleCallbackGuard { previous }
}

impl Drop for DeferredModuleCallbackGuard {
    fn drop(&mut self) {
        DEFERRED_MODULE_CALLBACK.with(|slot| slot.replace(self.previous.take()));
    }
}

pub fn install_dynamic_import_callback(
    callback: DynamicImportCallback,
) -> DynamicImportCallbackGuard {
    let previous = DYNAMIC_IMPORT_CALLBACK.with(|slot| slot.replace(Some(callback)));
    DynamicImportCallbackGuard { previous }
}

impl Drop for DynamicImportCallbackGuard {
    fn drop(&mut self) {
        DYNAMIC_IMPORT_CALLBACK.with(|slot| slot.replace(self.previous.take()));
    }
}

impl Drop for AsyncModuleStepGuard {
    fn drop(&mut self) {
        ASYNC_MODULE_STEP.with(|slot| slot.set(self.previous));
    }
}

pub(crate) fn execute_dynamic_import(
    specifier: String,
    deferred: bool,
    options: Value,
) -> Result<Value, VmError> {
    let callback = DYNAMIC_IMPORT_CALLBACK.with(|slot| slot.borrow().clone());
    callback.map_or(Err(VmError::NotCallable), |callback| {
        callback(specifier, deferred, options)
    })
}

pub(crate) fn execute_deferred_module(id: u32) -> Result<Value, VmError> {
    let nested = DEFERRED_MODULE_DEPTH.with(|depth| {
        let nested = depth.get() > 0;
        if !nested {
            depth.set(1);
        }
        nested
    });
    if nested {
        return Err(crate::value::error::throw_type_error(
            "Cannot evaluate a deferred module while it is evaluating",
        ));
    }
    let callback = DEFERRED_MODULE_CALLBACK.with(|slot| slot.borrow().clone());
    let result = callback.map_or(Err(VmError::NotCallable), |callback| callback(id));
    DEFERRED_MODULE_DEPTH.with(|depth| depth.set(0));
    result
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

    pub fn with_host_capability(
        mut self,
        name: impl Into<String>,
        value: HostCapabilityRef,
    ) -> Self {
        self.host_bindings.push((name.into(), value));
        self
    }

    pub(crate) fn host_binding(&self, name: &str) -> Option<HostCapabilityRef> {
        self.host_bindings
            .iter()
            .rev()
            .find_map(|(key, value)| (key == name).then_some(*value))
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

    pub fn with_can_block(mut self, can_block: bool) -> Self {
        self.can_block = can_block;
        self
    }

    pub(crate) fn can_block(&self) -> bool {
        self.can_block
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

pub(crate) fn intrinsic_for_realm(realm: RealmId, builtin: Builtin) -> Value {
    realm::intrinsic(realm, builtin).unwrap_or(Value::Builtin(builtin))
}

pub(crate) fn execute_completion_in_place(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<crate::completion::Completion, VmError> {
    let context = current_context_or_default();
    execute_completion_in_place_context(ops, registers, &context)
}

pub fn execute_with_context(ops: &[Op], context: &VmContext) -> Result<Value, VmError> {
    crate::locals::reset_replacements();
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
include!("vm_completion_step.rs");

fn run_ops(ops: &[Op], registers: &mut Vec<Value>, context: &VmContext) -> Result<Value, VmError> {
    completion_result(run_ops_completion(ops, registers, context)?)
}

fn run_ops_completion(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<crate::completion::Completion, VmError> {
    Ok(run_ops_completion_step(ops, registers, context)?.completion)
}

fn run_ops_completion_step(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    for (index, op) in ops.iter().enumerate() {
        let result = match run_op(registers, op, context) {
            Ok(result) => result,
            Err(error) => {
                crate::vm::flush_global_declaration_batch(registers);
                return error_completion(error).map(|completion| CompletionStep {
                    completion,
                    next: index + 1,
                });
            }
        };
        match result {
            None | Some(crate::completion::Completion::Normal) => {}
            Some(completion) => {
                crate::vm::flush_global_declaration_batch(registers);
                return Ok(CompletionStep {
                    completion,
                    next: index + 1,
                });
            }
        }
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok(CompletionStep {
        completion: crate::completion::Completion::Normal,
        next: ops.len(),
    })
}

pub(crate) fn execute_completion_step_in_environment(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
    environment: Rc<crate::environment::Environment>,
    drain_microtasks: bool,
) -> Result<CompletionStep, VmError> {
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    if drain_microtasks {
        crate::promise::drain_microtasks();
    }
    let step = run_ops_completion_step(ops, registers, context)?;
    let completion = preserve_frame_completion(step.completion)?;
    Ok(CompletionStep {
        completion,
        next: step.next,
    })
}

fn error_completion(error: VmError) -> Result<crate::completion::Completion, VmError> {
    crate::completion::Completion::from_vm_error(error)
}

pub(crate) fn completion_result(
    completion: crate::completion::Completion,
) -> Result<Value, VmError> {
    completion.into_vm_error()
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
    if matches!(
        function.kind,
        FunctionKind::Ordinary | FunctionKind::Generator
    ) && matches!(function.strictness, FunctionStrictness::Sloppy)
    {
        if matches!(this_value, Value::Undefined | Value::Null) {
            let global = function.captures.get(0);
            return if matches!(global, Value::Object(_)) {
                global
            } else {
                current_global_object()
            };
        }
        let global = function.captures.get(0);
        return to_object_value_in_realm(this_value, &global);
    }
    this_value.clone()
}

pub(crate) fn to_object_value(this_value: &Value) -> Value {
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
        Value::StringUnits(_) => boxed_primitive(this_value, crate::ops::Builtin::String),
        Value::BigInt(_) => boxed_primitive(this_value, crate::ops::Builtin::BigInt),
        Value::Null | Value::Undefined | Value::BindingCell(_) => this_value.clone(),
    }
}

fn to_object_value_in_realm(this_value: &Value, global: &Value) -> Value {
    let constructor = match this_value {
        Value::Boolean(_) => "Boolean",
        Value::Number(_) => "Number",
        Value::String(_) | Value::StringUnits(_) => "String",
        Value::BigInt(_) => "BigInt",
        _ => return to_object_value(this_value),
    };
    let constructor = crate::execute::get_property(global, constructor);
    let prototype = crate::execute::get_property(&constructor, "prototype");
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("_value".to_string(), this_value.clone()),
        ("constructor".to_string(), constructor),
        ("\0prototype".to_string(), prototype),
    ])))
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
    if builtin == Builtin::ArrayBufferByteLengthGetter {
        return execute_array_buffer_byte_length(receiver);
    }
    if builtin == Builtin::ArrayBufferDetachedGetter {
        return execute_array_buffer_detached(receiver);
    }
    if builtin == Builtin::ArrayBufferMaxByteLengthGetter {
        return execute_array_buffer_max_byte_length(receiver);
    }
    if builtin == Builtin::ArrayBufferResizableGetter {
        return execute_array_buffer_resizable(receiver);
    }
    if builtin == Builtin::ArrayBufferImmutableGetter {
        return execute_array_buffer_immutable(receiver);
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
        Builtin::AsyncGeneratorNext => Some(crate::generator::async_next(receiver, arguments)),
        Builtin::GeneratorReturn => Some(crate::generator::return_(receiver, arguments)),
        Builtin::AsyncGeneratorReturn => Some(crate::generator::async_return(receiver, arguments)),
        Builtin::GeneratorThrow => Some(crate::generator::throw(receiver, arguments)),
        Builtin::AsyncGeneratorThrow => Some(crate::generator::async_throw(receiver, arguments)),
        Builtin::AsyncIteratorDispose => Some(async_iterator_dispose(receiver)),
        Builtin::AsyncIteratorMethod => Some(Ok(receiver.cloned().unwrap_or(Value::Undefined))),
        Builtin::ProxyRevoke => Some(crate::proxy::revoke(receiver)),
        Builtin::Math => Some(Err(not_callable())),
        _ => None,
    }
}

fn async_iterator_dispose(receiver: Option<&Value>) -> Result<Value, VmError> {
    let target = receiver.unwrap_or(&Value::Undefined);
    let method = crate::execute::get_property_result(target, "return")?;
    if matches!(method, Value::Undefined | Value::Null) {
        return Ok(crate::promise::promise_resolve(&[Value::Undefined]));
    }
    let result = crate::functions::execute_target(&method, target, &[])?;
    Ok(crate::promise::promise_resolve(&[result]))
}

fn is_object_special(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::ObjectHasOwnProperty
            | Builtin::ObjectHasOwn
            | Builtin::ObjectGetOwnPropertyDescriptor
            | Builtin::ObjectGetOwnPropertyDescriptors
            | Builtin::ObjectGetOwnPropertyNames
            | Builtin::ObjectGetOwnPropertySymbols
            | Builtin::ObjectKeys
            | Builtin::ObjectValues
            | Builtin::ObjectEntries
            | Builtin::ObjectAssign
            | Builtin::ObjectFromEntries
            | Builtin::ObjectGroupBy
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
