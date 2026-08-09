use std::collections::HashMap;

use oxc::ast::ast::{ArrayExpression, ArrayExpressionElement};

use crate::{facts::ProgramDb, ops::Op, value::Value};

pub(crate) fn reduce(
    array: &ArrayExpression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let mut elements = Vec::new();
    for element in &array.elements {
        let register = match element {
            ArrayExpressionElement::Elision(_) => crate::reduce::emit_undefined(ops, next_register),
            ArrayExpressionElement::SpreadElement(_) => return None,
            _ => crate::reduce::reduce_expression(
                element.as_expression()?,
                ops,
                facts,
                next_register,
                locals,
            )?,
        };
        elements.push(register);
    }
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeArray {
        dst: register,
        elements,
    });
    Some(register)
}

pub(crate) fn property(values: &[Value], key: &str) -> Value {
    if key == "length" {
        return Value::Number(values.len() as f64);
    }
    let method = match key {
        "forEach" => crate::ops::Builtin::ArrayForEach,
        "map" => crate::ops::Builtin::ArrayMap,
        "filter" => crate::ops::Builtin::ArrayFilter,
        "some" => crate::ops::Builtin::ArraySome,
        _ => return index(values, key),
    };
    Value::Builtin(method)
}

fn index(values: &[Value], key: &str) -> Value {
    key.parse::<usize>()
        .ok()
        .and_then(|index| values.get(index).cloned())
        .unwrap_or(Value::Undefined)
}

pub(crate) fn some(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let Some(Value::Array(values)) = receiver else {
        return Ok(Value::Boolean(false));
    };
    let Some(callback) = arguments.first() else {
        return Ok(Value::Boolean(false));
    };
    for (index, value) in values.iter().enumerate() {
        let args = [
            value.clone(),
            Value::Number(index as f64),
            receiver.cloned().unwrap_or(Value::Undefined),
        ];
        let result = crate::functions::execute_target(callback, &Value::Undefined, &args)?;
        if crate::execute::is_truthy(&result) {
            return Ok(Value::Boolean(true));
        }
    }
    Ok(Value::Boolean(false))
}
