fn suspended_try_op(op: &crate::ops::Op, state: &crate::value::GeneratorState) -> bool {
    match op {
        crate::ops::Op::Yield { .. } => true,
        crate::ops::Op::YieldStar { iterator, .. } => crate::execute::read_register(&state.registers, *iterator)
            .is_ok_and(|value| !matches!(value, Value::Undefined)),
        _ => false,
    }
}

fn resume_suspended_try_op(
    registers: &mut Vec<Value>,
    yield_op: &crate::ops::Op,
    suffix: &[crate::ops::Op],
    resume: crate::completion::Completion,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    if matches!(yield_op, crate::ops::Op::Yield { .. }) {
        return match resume {
            crate::completion::Completion::Normal => {
                crate::execute::execute_completion_in_place(suffix, registers)
            }
            completion => Ok(completion),
        };
    }
    match crate::generator::execute_yield_star(registers, yield_op, resume) {
        Ok(Some(crate::completion::Completion::Yield(value))) => {
            Ok(crate::completion::Completion::Yield(value))
        }
        Ok(Some(completion)) => Ok(completion),
        Ok(None) => crate::execute::execute_completion_in_place(suffix, registers),
        Err(crate::execute::VmError::Thrown(value)) => {
            Ok(crate::completion::Completion::Throw(value))
        }
        Err(error) => Err(error),
    }
}
