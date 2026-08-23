use crate::{completion::Completion, execute::VmError, ops::Op, value::Value};
use std::collections::HashMap;

#[cold]
#[inline(never)]
fn missing_return() -> Result<Completion, VmError> {
    Err(VmError::MissingReturn)
}

#[inline]
pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<Completion, VmError> {
    let Op::Branch {
        condition,
        then_ops,
        else_ops,
    } = op
    else {
        return missing_return();
    };
    let value = crate::execute::read_register(registers, *condition)?;
    // Truthy conditions are the common path for conditionals. Keep that arm
    // as the fall-through arm; malformed branch bodies stay out of line.
    let selected = if crate::execute::is_truthy(&value) {
        then_ops
    } else {
        else_ops
    };
    let Some(selected) = selected.ops() else {
        return missing_return();
    };
    crate::execute::execute_completion_in_place(selected, registers)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let mut registers = vec![
            condition,
            Value::String("then".into()),
            Value::String("else".into()),
        ];
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
        let mut registers = vec![Value::Boolean(true)];
        let op = Op::Return { src: 0 };

        assert_eq!(execute(&mut registers, &op), Err(VmError::MissingReturn));
    }
}

pub(crate) fn reduce(
    statement: &oxc::ast::ast::Statement<'_>,
    facts: &mut crate::facts::ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<(Vec<Op>, Option<u16>), Vec<String>> {
    match statement {
        oxc::ast::ast::Statement::BlockStatement(block) => {
            let mut next_slot = crate::reduce_support::register_base(locals);
            let mut block_locals = locals.clone();
            crate::reduce_support::predeclare_lexicals(
                &block.body,
                &mut block_locals,
                &mut next_slot,
            );
            let (ops, last) = crate::reduce::reduce_statements_no_tail_value(
                &block.body,
                facts,
                block_locals,
                next_slot,
            )?;
            Ok((ops, last))
        }
        statement => {
            let mut ops = Vec::new();
            let mut next_register = crate::reduce_support::register_base(locals);
            let mut next_slot = crate::reduce_support::register_base(locals);
            let mut locals = locals.clone();
            let last = crate::reduce::reduce_statement(
                statement,
                &mut ops,
                facts,
                &mut next_register,
                &mut next_slot,
                &mut locals,
            )?;
            Ok((ops, last))
        }
    }
}
