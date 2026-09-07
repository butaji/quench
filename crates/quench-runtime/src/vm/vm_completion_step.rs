pub(crate) struct CompletionStep {
    pub(crate) completion: crate::completion::Completion,
    pub(crate) next: usize,
}

pub(crate) fn execute_code_completion_step_in_place(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<CompletionStep, VmError> {
    execute_code_completion_step_from_in_place(code, 0, registers)
}

pub(crate) fn execute_code_completion_step_from_in_place(
    code: crate::machine::CodeView<'_>,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<CompletionStep, VmError> {
    let context = current_context_or_default();
    if crate::locals::is_installed() {
        return execute_completion_steps(code, start, registers, &context);
    }
    let environment = crate::environment::Environment::child_registers(
        &crate::environment::Environment::new(),
        registers.clone(),
    );
    let _context_guard = ContextGuard::install(&context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    execute_completion_steps(code, start, registers, &context)
}

fn execute_completion_steps(
    code: crate::machine::CodeView<'_>,
    start: usize,
    registers: &mut crate::register_file::RegisterFile,
    context: &VmContext,
) -> Result<CompletionStep, VmError> {
    let mut cursor = start;
    loop {
        let step = run_code_completion_step_from(code, cursor, registers, context)?;
        match step.completion {
            crate::completion::Completion::Call(continuation) => {
                // A resumed branch/try/iterator suffix may end its step on a
                // synchronous call. Drain that call before handing the
                // completion back to the frame owner, matching the ordinary
                // completion driver and avoiding a false MissingReturn.
                match crate::vm::vm_ops::execute_call_continuation(registers, continuation) {
                    Ok(()) => cursor = step.next,
                    Err(VmError::Thrown(value)) => {
                        return Ok(CompletionStep {
                            completion: crate::completion::Completion::Throw(value),
                            next: step.next,
                        });
                    }
                    Err(error) => return Err(error),
                }
            }
            completion => {
                return Ok(CompletionStep {
                    completion: preserve_frame_completion(completion)?,
                    next: step.next,
                });
            }
        }
    }
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
    Ok(CompletionStep {
        completion,
        next: step.next,
    })
}
