pub(crate) struct GeneratorStep {
    pub(crate) completion: crate::completion::Completion,
    pub(crate) pc: usize,
    pub(crate) suspension: Option<crate::continuation::SuspensionPoint>,
}

pub(crate) fn execute_generator_step(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    environment: Rc<crate::environment::Environment>,
    pc: usize,
    resume: crate::completion::Completion,
) -> Result<GeneratorStep, VmError> {
    let context = VmContext::default();
    let _context_guard = ContextGuard::install(&context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    run_generator_steps(ops, registers, pc, resume, &context)
}

pub(crate) fn execute_generator_code_step(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    environment: Rc<crate::environment::Environment>,
    pc: usize,
    resume: crate::completion::Completion,
) -> Result<GeneratorStep, VmError> {
    let context = VmContext::default();
    let _context_guard = ContextGuard::install(&context);
    let _global_guard = GlobalObjectGuard::install();
    let _environment_guard = crate::locals::EnvironmentGuard::install(environment);
    run_generator_code_steps(code, registers, pc, resume, &context)
}

fn run_generator_code_steps(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    pc: usize,
    resume: crate::completion::Completion,
    context: &VmContext,
) -> Result<GeneratorStep, VmError> {
    if !matches!(resume, crate::completion::Completion::Normal)
        && !matches!(code.cold_at(pc), Some(Op::YieldStar { .. }))
    {
        return Ok(GeneratorStep { completion: resume, pc, suspension: None });
    }
    for next in pc..code.len() {
        let instruction = code.instruction(next).ok_or(VmError::MissingReturn)?;
        if let Some(op @ Op::YieldStar { .. }) = code.cold(instruction) {
            if let Some(step) = run_yield_star_step(registers, op, &resume, next)? {
                return Ok(step);
            }
            continue;
        }
        let result = run_instruction(code, instruction, registers, context)?;
        if let Some(completion) = result.filter(|value| !matches!(value, crate::completion::Completion::Normal)) {
            crate::vm::flush_global_declaration_batch(registers);
            let suspension = code.cold(instruction).and_then(|op| {
                matches!(completion, crate::completion::Completion::Yield(_))
                    .then(|| direct_suspension(op, next))
                    .flatten()
            });
            return Ok(GeneratorStep { completion, pc: next + 1, suspension });
        }
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok(GeneratorStep { completion: crate::completion::Completion::Normal, pc: code.len(), suspension: None })
}

fn run_generator_steps(
    ops: &[Op],
    registers: &mut crate::register_file::RegisterFile,
    pc: usize,
    resume: crate::completion::Completion,
    context: &VmContext,
) -> Result<GeneratorStep, VmError> {
    if !matches!(resume, crate::completion::Completion::Normal)
        && !matches!(ops.get(pc), Some(Op::YieldStar { .. }))
    {
        crate::vm::flush_global_declaration_batch(registers);
        return Ok(GeneratorStep {
            completion: resume,
            pc,
            suspension: None,
        });
    }
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
    Ok(GeneratorStep {
        completion: crate::completion::Completion::Normal,
        pc: ops.len(),
        suspension: None,
    })
}

fn run_yield_star_step(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
    resume: &crate::completion::Completion,
    next: usize,
) -> Result<Option<GeneratorStep>, VmError> {
    let completion = crate::generator::execute_yield_star(registers, op, resume.clone())?;
    if let Some(completion) = completion {
        crate::vm::flush_global_declaration_batch(registers);
        let offset = usize::from(!completion.is_suspension());
        let suspension =
            matches!(completion, crate::completion::Completion::Yield(_)).then(|| match op {
                Op::YieldStar { dst, iterator, .. } => {
                    crate::continuation::SuspensionPoint::YieldStar {
                        pc: next,
                        dst: *dst,
                        iterator: *iterator,
                    }
                }
                _ => crate::continuation::SuspensionPoint::Yield { pc: next, src: 0 },
            });
        return Ok(Some(GeneratorStep {
            completion,
            pc: next + offset,
            suspension,
        }));
    }
    Ok(None)
}

fn run_generator_op(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
    context: &VmContext,
    next: usize,
) -> Result<Option<GeneratorStep>, VmError> {
    let result = match run_op(registers, op, context) {
        Err(error) => match crate::completion::Completion::from_vm_error(error) {
            Ok(completion) => Some(completion),
            Err(error) => {
                crate::vm::flush_global_declaration_batch(registers);
                return Err(error);
            }
        },
        Ok(result) => result,
    };
    if matches!(result, Some(crate::completion::Completion::Normal) | None) {
        return Ok(None);
    }
    if let Some(completion) = result {
        crate::vm::flush_global_declaration_batch(registers);
        let suspension = matches!(completion, crate::completion::Completion::Yield(_))
            .then(|| direct_suspension(op, next))
            .flatten();
        return Ok(Some(GeneratorStep {
            completion,
            pc: next + 1,
            suspension,
        }));
    }
    Ok(None)
}

fn direct_suspension(op: &Op, pc: usize) -> Option<crate::continuation::SuspensionPoint> {
    match op {
        Op::Yield { src } => Some(crate::continuation::SuspensionPoint::Yield { pc, src: *src }),
        _ => None,
    }
}
