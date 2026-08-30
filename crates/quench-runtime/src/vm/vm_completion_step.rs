pub(crate) struct CompletionStep {
    pub(crate) completion: crate::completion::Completion,
    pub(crate) next: usize,
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
    Ok(CompletionStep { completion, next: step.next })
}
