use crate::{completion::Completion, execute::VmError, ops::Op};
use std::collections::HashMap;

#[cold]
#[inline(never)]
fn missing_return() -> Result<Completion, VmError> {
    Err(VmError::MissingReturn)
}

#[inline]
pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<Completion, VmError> {
    let context = crate::vm::current_context_or_default();
    execute_with_context(registers, op, &context)
}

#[inline]
pub(crate) fn execute_with_context(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
    context: &crate::vm::VmContext,
) -> Result<Completion, VmError> {
    let Op::Branch {
        condition,
        then_ops,
        else_ops,
    } = op
    else {
        return missing_return();
    };
    let truthy = match registers.word_truthiness(usize::from(*condition)) {
        Some(truthy) => truthy,
        None => crate::execute::is_truthy(&crate::execute::read_register(registers, *condition)?),
    };
    let selected = if truthy { then_ops } else { else_ops };
    let Some(code) = selected.code() else {
        return missing_return();
    };
    crate::vm::with_current_context(context, || execute_selected(registers, selected, code))
}

fn execute_selected(
    registers: &mut crate::register_file::RegisterFile,
    owner: &crate::machine::FunctionCode,
    code: crate::machine::CodeView<'_>,
) -> Result<Completion, VmError> {
    let _ = owner.enter_invocation();
    let mut pc = 0;
    loop {
        let step = crate::vm::execute_code_completion_step_with_owner(code, owner, pc, registers)?;
        let next = step.next;
        let suspended_pc = step.suspended_pc;
        match step.completion {
            Completion::Call(continuation) => {
                crate::vm::vm_ops::execute_call_continuation(registers, continuation)?;
                pc = next;
            }
            completion if completion.is_suspension() => {
                return branch_suspension(code, next, suspended_pc, completion);
            }
            completion => return Ok(completion),
        }
    }
}

fn branch_suspension(
    code: crate::machine::CodeView<'_>,
    next: usize,
    suspended_pc: Option<usize>,
    completion: Completion,
) -> Result<Completion, VmError> {
    let pc = suspended_pc.or_else(|| next.checked_sub(1));
    let yield_dst = pc
        .and_then(|pc| code.cold_at(pc))
        .and_then(suspension_destination)
        .or_else(|| completion.suspension_point().and_then(point_destination))
        .ok_or(VmError::MissingReturn)?;
    let point = crate::continuation::SuspensionPoint::Branch {
        body_resume: suffix(code.range(), next),
        yield_dst,
    };
    Ok(wrap_suspension(completion, point))
}

fn suspension_destination(op: &Op) -> Option<u16> {
    match op {
        Op::Await { dst, .. } | Op::Yield { src: dst } => Some(*dst),
        _ => None,
    }
}

fn point_destination(point: &crate::continuation::SuspensionPoint) -> Option<u16> {
    match point {
        crate::continuation::SuspensionPoint::Yield { src, .. }
        | crate::continuation::SuspensionPoint::Loop { yield_dst: src, .. }
        | crate::continuation::SuspensionPoint::Branch { yield_dst: src, .. } => Some(*src),
        crate::continuation::SuspensionPoint::YieldStar { dst, .. } => Some(*dst),
        crate::continuation::SuspensionPoint::Nested { inner, .. } => point_destination(inner),
    }
}

fn suffix(range: crate::machine::CodeRange, next: usize) -> crate::machine::CodeRange {
    crate::machine::CodeRange {
        code: range.code,
        start: range.start.saturating_add(next as u32),
        end: range.end,
    }
}

fn wrap_suspension(
    completion: Completion,
    outer: crate::continuation::SuspensionPoint,
) -> Completion {
    match completion {
        Completion::Suspend(value) => Completion::SuspendAt(value, outer),
        Completion::Yield(value) => Completion::YieldAt(value, outer),
        Completion::SuspendAt(value, inner) => Completion::SuspendAt(
            value,
            crate::continuation::SuspensionPoint::Nested {
                inner: Box::new(inner),
                outer: Box::new(outer),
            },
        ),
        Completion::YieldAt(value, inner) => Completion::YieldAt(
            value,
            crate::continuation::SuspensionPoint::Nested {
                inner: Box::new(inner),
                outer: Box::new(outer),
            },
        ),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    fn branch_value(condition: Value) -> Value {
        let then_ops = crate::machine::FunctionCode::from_ops(vec![Op::Return { src: 1 }]);
        let else_ops = crate::machine::FunctionCode::from_ops(vec![Op::Return { src: 2 }]);
        let op = Op::Branch {
            condition: 0,
            then_ops,
            else_ops,
        };
        // Keep the selected values in registers so both arms exercise the
        // same completion machinery and only truthiness chooses the path.
        let mut registers = crate::register_file::RegisterFile::from_values(vec![
            condition,
            Value::String("then".into()),
            Value::String("else".into()),
        ]);
        let completion = execute(&mut registers, &op).expect("branch completes");
        match completion {
            Completion::Return(value) => value,
            other => panic!("unexpected completion: {other:?}"),
        }
    }

    #[test]
    fn truthy_branch_is_the_fall_through_semantic_path() {
        assert_eq!(
            branch_value(Value::Boolean(true)),
            Value::String("then".into())
        );
    }

    #[test]
    fn falsy_branch_still_selects_the_alternate() {
        assert_eq!(
            branch_value(Value::Boolean(false)),
            Value::String("else".into())
        );
    }

    #[test]
    fn non_branch_opcode_uses_cold_error_path() {
        let mut registers =
            crate::register_file::RegisterFile::from_values(vec![Value::Boolean(true)]);
        let op = Op::Return { src: 0 };

        assert_eq!(execute(&mut registers, &op), Err(VmError::MissingReturn));
    }

    #[test]
    fn nested_branch_body_retains_its_tier_owner() {
        let then_ops = crate::machine::FunctionCode::from_ops(vec![Op::Return { src: 1 }]);
        then_ops.set_tier_threshold_for_test(2);
        let else_ops = crate::machine::FunctionCode::from_ops(vec![Op::Return { src: 2 }]);
        let op = Op::Branch {
            condition: 0,
            then_ops: then_ops.clone(),
            else_ops,
        };
        for _ in 0..3 {
            let mut registers = crate::register_file::RegisterFile::from_values(vec![
                Value::Boolean(true),
                Value::Number(1.0),
            ]);
            assert!(matches!(
                execute(&mut registers, &op),
                Ok(Completion::Return(_))
            ));
        }
        assert_eq!(then_ops.tier(), crate::machine::ExecutionTier::Baseline);
    }
}

pub(crate) fn reduce_with_registers(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut crate::facts::ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<(Vec<Op>, Option<u16>), Vec<String>> {
    match statement {
        oxc::ast::ast::Statement::BlockStatement(block) => {
            let mut block_locals = locals.clone();
            crate::reduce_support::predeclare_lexicals(&block.body, &mut block_locals, next_slot);
            let mut ops = Vec::new();
            let mut last = None;
            for child in &block.body {
                last = crate::reduce::reduce_statement(
                    child,
                    &mut ops,
                    facts,
                    next_register,
                    next_slot,
                    &mut block_locals,
                )?
                .or(last);
            }
            let (ops, last) = (ops, last);
            Ok((ops, last))
        }
        statement => {
            let mut ops = Vec::new();
            let mut locals = locals.clone();
            let last = crate::reduce::reduce_statement(
                statement,
                &mut ops,
                facts,
                next_register,
                next_slot,
                &mut locals,
            )?;
            Ok((ops, last))
        }
    }
}
