use std::{collections::HashMap, convert::TryFrom};

use oxc::ast::ast::BindingPatternKind;

use crate::{facts::ProgramDb, ops::Op};

pub(crate) fn function_parameters(
    function: &oxc::ast::ast::Function<'_>,
) -> Result<(HashMap<String, u16>, u16), Vec<String>> {
    let mut parameters = HashMap::new();
    for (slot, parameter) in function.params.items.iter().enumerate() {
        let BindingPatternKind::BindingIdentifier(identifier) = &parameter.pattern.kind else {
            return Err(vec!["Unsupported function parameter pattern".to_string()]);
        };
        let slot =
            u16::try_from(slot).map_err(|_| vec!["Too many function parameters".to_string()])?;
        parameters.insert(identifier.name.to_string(), slot);
    }
    let count = u16::try_from(function.params.items.len())
        .map_err(|_| vec!["Too many function parameters".to_string()])?;
    Ok((parameters, count))
}

pub(crate) fn reduce_expression(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
) -> Option<u16> {
    let body = function.body.as_ref()?;
    let (parameters, parameter_count) = function_parameters(function).ok()?;
    let body_ops = crate::reduce::reduce_statements_with_locals(
        &body.statements,
        facts,
        parameters,
        parameter_count,
    )
    .ok()?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::MakeFunction {
        dst: register,
        body: body_ops,
        params: parameter_count,
    });
    Some(register)
}

pub(crate) fn make(body: &[Op], params: u16) -> crate::value::Value {
    crate::value::Value::Function(std::rc::Rc::new(crate::value::FunctionValue {
        body: body.to_vec(),
        params,
        properties: std::rc::Rc::new(Vec::new()),
    }))
}
