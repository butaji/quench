pub(crate) fn execute_generator_step(
    ops: &[Op],
    registers: &mut Vec<Value>,
    environment: Rc<crate::environment::Environment>,
    pc: usize,
    resume: crate::completion::Completion,
) -> Result<(crate::completion::Completion, usize), VmError> {
    let context = VmContext::default();
    let _context_guard = ContextGuard::install(&context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    run_generator_steps(ops, registers, pc, resume, &context)
}

fn run_generator_steps(
    ops: &[Op],
    registers: &mut Vec<Value>,
    pc: usize,
    resume: crate::completion::Completion,
    context: &VmContext,
) -> Result<(crate::completion::Completion, usize), VmError> {
    for (offset, op) in ops[pc..].iter().enumerate() {
        if matches!(op, Op::YieldStar { .. }) {
            let maybe = run_yield_star_step(registers, op, &resume, pc + offset)?;
            if let Some(result) = maybe {
                return Ok(result);
            }
            continue;
        }
        let result = run_generator_op(registers, op, context, pc + offset)?;
        if let Some(completion) = result {
            return Ok(completion);
        }
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok((crate::completion::Completion::Normal, ops.len()))
}

fn run_yield_star_step(
    registers: &mut Vec<Value>,
    op: &Op,
    resume: &crate::completion::Completion,
    next: usize,
) -> Result<Option<(crate::completion::Completion, usize)>, VmError> {
    let completion = crate::generator::execute_yield_star(registers, op, resume.clone())?;
    if let Some(completion) = completion {
        crate::vm::flush_global_declaration_batch(registers);
        let offset = usize::from(!completion.is_suspension());
        return Ok(Some((completion, next + offset)));
    }
    Ok(None)
}

fn run_generator_op(
    registers: &mut Vec<Value>,
    op: &Op,
    context: &VmContext,
    next: usize,
) -> Result<Option<(crate::completion::Completion, usize)>, VmError> {
    let result = match run_op(registers, op, context) {
        Err(VmError::Yield(value)) => {
            crate::vm::flush_global_declaration_batch(registers);
            return Ok(Some((crate::completion::Completion::Yield(value), next + 1)));
        }
        Err(error) => {
            crate::vm::flush_global_declaration_batch(registers);
            return Err(error);
        }
        Ok(result) => result,
    };
    if matches!(
        result,
        Some(crate::completion::Completion::Normal) | None
    ) {
        return Ok(None);
    }
    if let Some(completion) = result {
        crate::vm::flush_global_declaration_batch(registers);
        return Ok(Some((completion, next + 1)));
    }
    Ok(None)
}
