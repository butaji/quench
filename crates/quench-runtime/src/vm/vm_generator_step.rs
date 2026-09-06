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
    let context = crate::vm::current_context_or_default();
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
        return Ok(GeneratorStep {
            completion: resume,
            pc,
            suspension: None,
        });
    }
    let mut next = pc;
    while next < code.len() {
        let instruction = code.instruction(next).ok_or(VmError::MissingReturn)?;
        if let Some(op @ Op::YieldStar { .. }) = code.cold(instruction) {
            let point = code_resume(code.range(), next);
            if let Some(step) = run_yield_star_step(registers, op, &resume, next, Some(point))? {
                return Ok(step);
            }
            next += 1;
            continue;
        }
        let transition = match run_instruction(code, next, instruction, registers, context) {
            Ok(transition) => transition,
            Err(error) => match crate::completion::Completion::from_vm_error(error) {
                Ok(completion) => {
                    let target = if matches!(&completion, crate::completion::Completion::Normal) {
                        crate::vm::DispatchTarget::Callee(next + 1)
                    } else {
                        crate::vm::DispatchTarget::Exit
                    };
                    crate::vm::DispatchTransition {
                        next_pc: next + 1,
                        completion: Some(completion),
                        target,
                    }
                }
                Err(error) => return Err(error),
            },
        };
        // The handler supplies the continuation target.  `next_pc` remains
        // metadata for exit reporting; normal generator dispatch follows the
        // callee-directed target rather than recomputing a successor.
        let next_pc = match transition.target {
            crate::vm::DispatchTarget::Callee(target) => target,
            crate::vm::DispatchTarget::Exit => transition.next_pc,
        };
        if let Some(completion) = transition
            .completion
            .filter(|value| !matches!(value, crate::completion::Completion::Normal))
        {
            if let crate::completion::Completion::Call(continuation) = completion {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
                next = next_pc;
                continue;
            }
            crate::vm::flush_global_declaration_batch(registers);
            let suspension = completion.suspension_point().cloned().or_else(|| {
                code.cold(instruction).and_then(|op| {
                    matches!(
                        completion,
                        crate::completion::Completion::Yield(_)
                            | crate::completion::Completion::Suspend(_)
                    )
                    .then(|| direct_suspension(op, Some(code_resume(code.range(), next_pc))))
                    .flatten()
                })
            });
            return Ok(GeneratorStep {
                completion,
                pc: next_pc,
                suspension,
            });
        }
        next = next_pc;
    }
    crate::vm::flush_global_declaration_batch(registers);
    Ok(GeneratorStep {
        completion: crate::completion::Completion::Normal,
        pc: code.len(),
        suspension: None,
    })
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
            let maybe = run_yield_star_step(registers, op, &resume, pc + offset, None)?;
            if let Some(result) = maybe {
                return Ok(result);
            }
            continue;
        }
        let result = run_generator_op(registers, op, context, pc + offset)?;
        if let Some(step) = result {
            // Ordinary calls are nested VM transitions, not suspensions of
            // the generator itself. Consume their continuation here so the
            // body continues until an explicit yield/await boundary.
            if let crate::completion::Completion::Call(continuation) = step.completion {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
                continue;
            }
            return Ok(step);
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
    point_resume: Option<crate::machine::CodeRange>,
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
            Op::YieldStar { dst, iterator, .. } => {
                crate::continuation::SuspensionPoint::YieldStar {
                    resume: point_resume,
                    dst: *dst,
                    iterator: *iterator,
                }
            }
            _ => crate::continuation::SuspensionPoint::Yield {
                resume: point_resume,
                src: 0,
            },
        });
    Ok(Some(GeneratorStep {
        completion,
        pc: next + offset,
        suspension,
    }))
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
        let suspension = completion.suspension_point().cloned().or_else(|| {
            matches!(
                completion,
                crate::completion::Completion::Yield(_) | crate::completion::Completion::Suspend(_)
            )
            .then(|| direct_suspension(op, None))
            .flatten()
        });
        return Ok(Some(GeneratorStep {
            completion,
            pc: next + 1,
            suspension,
        }));
    }
    Ok(None)
}

fn direct_suspension(
    op: &Op,
    resume: Option<crate::machine::CodeRange>,
) -> Option<crate::continuation::SuspensionPoint> {
    match op {
        Op::Yield { src } => {
            Some(crate::continuation::SuspensionPoint::Yield { resume, src: *src })
        }
        // Await uses the same internal resume-slot shape as a yield.  It is
        // never exposed as a generator yield; the marker lets a structured
        // loop retain its body suffix until the promise resumes.
        Op::Await { dst, .. } => {
            Some(crate::continuation::SuspensionPoint::Yield { resume, src: *dst })
        }
        Op::Loop { .. } => None,
        _ => None,
    }
}

fn code_resume(range: crate::machine::CodeRange, relative_pc: usize) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(relative_pc as u32),
        end: range.end,
    }
}
