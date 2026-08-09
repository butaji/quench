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
    let target = crate::execute::read_register(registers, *callee)?;
    let value = construct_value(&target, &arguments)?;
    crate::execute::write_value(registers, *dst, value);
    Ok(())
}

pub(crate) fn construct_value(
    target: &Value,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    match target {
        Value::Builtin(crate::ops::Builtin::Array) => Ok(crate::builtins::array(arguments)),
        Value::Builtin(crate::ops::Builtin::Object) => Ok(crate::builtins::object(arguments)),
        Value::Builtin(crate::ops::Builtin::TypeError) => Ok(crate::builtins::object(arguments)),
        Value::Builtin(crate::ops::Builtin::Date) => Ok(crate::builtins::object(arguments)),
        Value::Builtin(crate::ops::Builtin::RegExp) => Ok(crate::builtins::object(arguments)),
        Value::Builtin(
            crate::ops::Builtin::IntlNumberFormat
            | crate::ops::Builtin::IntlDateTimeFormat
            | crate::ops::Builtin::IntlCollator
            | crate::ops::Builtin::IntlPluralRules
            | crate::ops::Builtin::IntlListFormat
            | crate::ops::Builtin::IntlRelativeTimeFormat
            | crate::ops::Builtin::IntlSegmenter
            | crate::ops::Builtin::IntlDisplayNames
            | crate::ops::Builtin::IntlLocale,
        ) => crate::intl::execute(*target_builtin(target), arguments, None)
            .unwrap_or_else(|| Ok(crate::builtins::object(arguments))),
        Value::Function(_) => Ok(Value::Object(std::rc::Rc::new(Vec::new()))),
        _ => Err(crate::execute::VmError::NotCallable),
    }
}

fn target_builtin(target: &Value) -> &crate::ops::Builtin {
    match target {
        Value::Builtin(builtin) => builtin,
        _ => &crate::ops::Builtin::Array,
    }
}
