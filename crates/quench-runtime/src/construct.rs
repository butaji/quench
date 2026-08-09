use std::collections::HashMap;

use crate::{facts::ProgramDb, ops::Op, value::Value};

pub(crate) fn reduce(
    expression: &oxc::ast::ast::NewExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let callee =
        crate::reduce::reduce_expression(&expression.callee, ops, facts, next_register, locals)?;
    let args = expression
        .arguments
        .iter()
        .map(|argument| {
            crate::reduce::reduce_expression(
                argument.as_expression()?,
                ops,
                facts,
                next_register,
                locals,
            )
        })
        .collect::<Option<Vec<_>>>()?;
    let dst = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Construct { dst, callee, args });
    Some(dst)
}

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), crate::execute::VmError> {
    let Op::Construct { dst, callee, args } = op else {
        return Err(crate::execute::VmError::NotCallable);
    };
    let arguments = args
        .iter()
        .map(|index| crate::execute::read_register(registers, *index))
        .collect::<Result<Vec<_>, _>>()?;
    let value = match crate::execute::read_register(registers, *callee)? {
        Value::Builtin(crate::ops::Builtin::Array) => crate::builtins::array(&arguments),
        Value::Builtin(crate::ops::Builtin::Object) => crate::builtins::object(&arguments),
        Value::Builtin(crate::ops::Builtin::TypeError) => crate::builtins::object(&arguments),
        Value::Builtin(crate::ops::Builtin::Date) => crate::builtins::object(&arguments),
        Value::Function(_) => Value::Object(std::rc::Rc::new(Vec::new())),
        _ => return Err(crate::execute::VmError::NotCallable),
    };
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}
