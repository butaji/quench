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
