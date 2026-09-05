pub(crate) struct CompletionStep {
    pub(crate) completion: crate::completion::Completion,
    pub(crate) next: usize,
    /// Exact residual operation that produced a suspension.  This is carried
    /// from the driver, rather than reconstructed by scanning a cold tree.
    pub(crate) suspended_pc: Option<usize>,
}

pub(crate) fn execute_completion_step_in_place(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
) -> Result<CompletionStep, VmError> {
    let context = current_context_or_default();
    execute_completion_step_context(ops, registers, &context)
}

pub(crate) fn execute_code_completion_step_in_place(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<CompletionStep, VmError> {
    let context = current_context_or_default();
    if crate::locals::is_installed() {
        return run_code_completion_step_from(code, 0, registers, &context);
    }
    let environment = crate::environment::Environment::child(
        &crate::environment::Environment::new(),
        registers.to_values(),
    );
    let _context_guard = ContextGuard::install(&context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_code_completion_step_from(code, 0, registers, &context)?;
    let completion = preserve_frame_completion(step.completion)?;
    Ok(CompletionStep { completion, next: step.next, suspended_pc: step.suspended_pc })
}

fn execute_completion_step_context(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    if crate::locals::is_installed() {
        return run_ops_completion_step(ops, registers, context);
    }
    let environment = crate::environment::Environment::child(
        &crate::environment::Environment::new(),
        registers.to_values(),
    );
    let _context_guard = ContextGuard::install(context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    let step = run_ops_completion_step(ops, registers, context)?;
    let completion = preserve_frame_completion(step.completion)?;
    Ok(CompletionStep { completion, next: step.next, suspended_pc: step.suspended_pc })
}
