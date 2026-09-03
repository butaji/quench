pub(crate) struct GeneratorStep {
    pub(crate) completion: crate::completion::Completion,
    pub(crate) pc: usize,
    pub(crate) suspension: Option<crate::continuation::SuspensionPoint>,
}

pub(crate) fn execute_generator_code_step(
    code: crate::machine::CodeView<'_>,
    registers: &mut crate::register_file::RegisterFile,
    environment: Rc<crate::environment::Environment>,
    pc: usize,
    resume: crate::completion::Completion,
) -> Result<GeneratorStep, VmError> {
    let context = crate::vm::current_context_or_default();
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
    let mut next = pc;
    while next < code.len() {
        let instruction = code.instruction(next).ok_or(VmError::MissingReturn)?;
        match instruction.opcode {
            crate::ir::Opcode::Jump => {
                next = usize::from(instruction.a);
                continue;
            }
            crate::ir::Opcode::JumpIfFalse => {
                let truthy = registers
                    .word_truthiness(usize::from(instruction.a))
                    .map_or_else(
                        || run_read_truthiness(registers, instruction.a),
                        Ok,
                    )?;
                next = if truthy { next.saturating_add(1) } else { usize::from(instruction.b) };
                continue;
            }
            _ => {}
        }
        if let Some(op @ Op::YieldStar { .. }) = code.cold(instruction) {
            if let Some(step) = run_yield_star_step(registers, op, &resume, next)? {
                return Ok(step);
            }
            next += 1;
            continue;
        }
        let result = match run_instruction(code, next, instruction, registers, context) {
            Ok(result) => result,
            Err(error) => match crate::completion::Completion::from_vm_error(error) {
                Ok(completion) => Some(completion),
                Err(error) => return Err(error),
            },
        };
        if let Some(completion) = result.filter(|value| !matches!(value, crate::completion::Completion::Normal)) {
            if let crate::completion::Completion::Call(continuation) = completion {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
                next = next.saturating_add(1);
                continue;
            }
            crate::vm::flush_global_declaration_batch(registers);
            let suspension = code.cold(instruction).and_then(|op| {
                completion.is_suspension()
                    .then(|| direct_suspension(op, next))
                    .flatten()
            });
            return Ok(GeneratorStep { completion, pc: next + 1, suspension });
        }
        next += 1;
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok(GeneratorStep { completion: crate::completion::Completion::Normal, pc: code.len(), suspension: None })
}

fn run_read_truthiness(
    registers: &crate::register_file::RegisterFile,
    register: u16,
) -> Result<bool, VmError> {
    crate::execute::read_register(registers, register).map(|value| crate::execute::is_truthy(&value))
}

fn run_yield_star_step(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
    resume: &crate::completion::Completion,
    next: usize,
) -> Result<Option<GeneratorStep>, VmError> {
    let completion = match crate::generator::execute_yield_star(registers, op, resume.clone()) {
        Ok(Some(completion)) => completion,
        Ok(None) => return Ok(None),
        Err(error) => match crate::completion::Completion::from_vm_error(error) {
            Ok(completion) => completion,
            Err(error) => {
                crate::vm::flush_global_declaration_batch(registers);
                return Err(error);
            }
        },
    };
    crate::vm::flush_global_declaration_batch(registers);
    let offset = usize::from(!completion.is_suspension());
    let suspension =
        matches!(completion, crate::completion::Completion::Yield(_)).then(|| match op {
            Op::YieldStar { dst, iterator, .. } => crate::continuation::SuspensionPoint::YieldStar {
                pc: next,
                dst: *dst,
                iterator: *iterator,
            },
            _ => crate::continuation::SuspensionPoint::Yield { pc: next, src: 0 },
        });
    Ok(Some(GeneratorStep {
        completion,
        pc: next + offset,
        suspension,
    }))
}

fn direct_suspension(op: &Op, pc: usize) -> Option<crate::continuation::SuspensionPoint> {
    match op {
        Op::Yield { src } => Some(crate::continuation::SuspensionPoint::Yield { pc, src: *src }),
        Op::Loop {
            label,
            body,
            test,
            update,
            post_test,
            dst,
            ..
        } => {
            let body_code = body.code()?;
            let (index, candidate) = body_code.find_cold(|candidate| {
                matches!(candidate, Op::Yield { .. } | Op::Await { .. })
            })?;
            let src = match candidate {
                Op::Yield { src } | Op::Await { dst: src, .. } => *src,
                _ => return None,
            };
            Some(crate::continuation::SuspensionPoint::Loop {
                pc,
                label: label.clone(),
                body: body.range,
                test: test.range,
                update: update.range,
                body_resume: crate::machine::CodeRange {
                    code: body.range.code,
                    start: body.range.start.saturating_add(index as u32 + 1),
                    end: body.range.end,
                },
                dst: *dst,
                yield_dst: src,
                post_test: *post_test,
            })
        }
        _ => None,
    }
}
