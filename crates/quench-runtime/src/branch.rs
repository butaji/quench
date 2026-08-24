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
    let Some(selected) = selected.code() else {
        return missing_return();
    };
    crate::vm::execute_code_completion_in_current_frame(selected, registers)
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
}

pub(crate) fn reduce(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<(Vec<Op>, Option<u16>), Vec<String>> {
    let mut next_register = crate::reduce_support::register_base(locals);
    let mut next_slot = crate::reduce_support::register_base(locals);
    reduce_with_registers(statement, facts, &mut next_register, &mut next_slot, locals)
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
