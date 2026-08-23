pub(crate) struct CompletionStep {
    pub(crate) completion: crate::completion::Completion,
    pub(crate) next: usize,
}

pub(crate) fn execute_completion_step_in_place(
    ops: &[Op],
    registers: &mut Vec<Value>,
) -> Result<CompletionStep, VmError> {
    let context = current_context_or_default();
    execute_completion_step_context(ops, registers, &context)
}

fn execute_completion_step_context(
    ops: &[Op],
    registers: &mut Vec<Value>,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    if crate::locals::is_installed() {
        return run_ops_completion_step(ops, registers, context);
    }
    let environment = crate::environment::Environment::child(
        &crate::environment::Environment::new(),
        registers.clone(),
    );
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_ops_completion_step(ops, registers, context)?;
    let completion = preserve_frame_completion(step.completion)?;
    Ok(CompletionStep { completion, next: step.next })
}
