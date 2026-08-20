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
    // The replacement log is the heap's forwarding table for copy-on-write
    // object snapshots: it must outlive any single top-level entry so that
    // references stashed in heap slots (event-loop callbacks, timers) still
    // resolve to the latest snapshot. Hosts running independent programs on
    // one thread call `execute::reset_replacements` between programs.
    let result = execute_with_registers_context(ops, Vec::new(), context);
    // Drive the promise microtask queue so `.then`/`.catch` reactions and
    // synchronously-settling promise chains run to completion. Reinstall
    // the realm: reactions may call host capabilities (console.log) and
    // read globals, both of which require an active execution context.
    let realm = context.realm();
    if realm::with_realm(realm, crate::promise::drain_microtasks_all).is_none() {
        crate::vm::with_current_context(context, crate::promise::drain_microtasks_all);
    }
    result
}

/// The context currently active on this thread, or a default one.
/// Hosts use this to re-enter the VM from inside a capability call.
pub fn current_context() -> VmContext {
    current_context_or_default()
}

/// Call a function value from host code through the current context.
pub fn call_value(
    target: &Value,
    receiver: &Value,
    arguments: &[Value],
) -> Result<Value, VmError> {
    crate::functions::execute_target(target, receiver, arguments)
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

/// Execute a fragment inside the currently installed lexical environment.
/// Loop tests and updates must mutate the surrounding loop bindings; creating
/// a child environment would discard those writes when the fragment returns.
pub fn execute_in_current_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<Value, VmError> {
    let context = current_context_or_default();
    if crate::locals::is_installed() {
        return completion_result(run_ops_completion(ops, registers, &context)?);
    }
    execute_in_place_context(ops, registers, &context)
}

pub fn execute_in_place_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<Value, VmError> {
    let parent = crate::locals::current();
    let environment =
        crate::environment::Environment::in_place_child(&parent, registers.clone());
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

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::value::{ObjectData, Value};

    /// Bug reproducer: the replacement log is the forwarding table that
    /// keeps heap-resident object references (event-loop callbacks, timers)
    /// pointing at the latest copy-on-write snapshot. Re-entering the VM
    /// for another top-level execution of the same program must not clear
    /// it — otherwise those references silently revert to stale snapshots.
    #[test]
    fn execute_with_context_preserves_replacements() {
        let old = Value::Object(Rc::new(ObjectData::new(vec![])));
        let new = Value::Object(Rc::new(ObjectData::new(vec![])));
        crate::locals::replace_value(&old, &new);
        let program = crate::reduce::reduce_source("0;").expect("source reduces");
        crate::vm::execute_with_context(program.ops(), &crate::vm::VmContext::default())
            .expect("program runs");
        let resolved = crate::locals::resolved_replacement(old);
        let (Value::Object(resolved), Value::Object(expected)) = (resolved, new) else {
            panic!("replacement must resolve to the new snapshot");
        };
        assert!(Rc::ptr_eq(&resolved, &expected));
    }
}
