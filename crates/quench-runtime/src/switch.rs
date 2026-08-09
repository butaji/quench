use crate::{
    facts::ProgramDb,
    literal::reduce_literal,
    ops::{Constant, Op},
    value::Value,
};
use oxc::ast::ast::SwitchStatement;
use std::collections::HashMap;

type SwitchCases = Vec<(Option<Constant>, Vec<Op>)>;

pub(crate) fn reduce(
    statement: &SwitchStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let discriminant = crate::reduce::reduce_expression(
        &statement.discriminant,
        ops,
        facts,
        next_register,
        locals,
    )
    .ok_or_else(|| vec!["Unsupported switch discriminant".to_string()])?;
    let cases = reduce_cases(statement, facts, next_register, locals)?;
    ops.push(Op::Switch {
        discriminant,
        cases,
    });
    Ok(())
}

fn reduce_cases(
    statement: &SwitchStatement<'_>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<SwitchCases, Vec<String>> {
    let mut cases = Vec::new();
    for case in &statement.cases {
        let test = case
            .test
            .as_ref()
            .map(|test| {
                reduce_literal(test)
                    .map(|literal| literal.op)
                    .ok_or_else(|| vec!["Dynamic switch case is unsupported".to_string()])
            })
            .transpose()?;
        let mut body = Vec::new();
        let mut next_slot = crate::reduce_support::register_base(locals);
        let mut body_locals = locals.clone();
        for statement in &case.consequent {
            crate::reduce::reduce_statement(
                statement,
                &mut body,
                facts,
                next_register,
                &mut next_slot,
                &mut body_locals,
            )?;
        }
        cases.push((test, body));
    }
    Ok(cases)
}

pub(crate) fn execute(registers: &mut Vec<Value>, op: &Op) -> Result<(), crate::execute::VmError> {
    let Op::Switch {
        discriminant,
        cases,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *discriminant)?;
    if let Some((_, body)) = cases.iter().find(|(test, _)| {
        test.as_ref()
            .map_or(true, |test| same_constant(&value, test))
    }) {
        crate::execute::execute_in_place(body, registers)?;
    }
    Ok(())
}

fn same_constant(value: &Value, constant: &Constant) -> bool {
    match (value, constant) {
        (Value::Number(left), Constant::Number(right)) => left == right,
        (Value::Boolean(left), Constant::Boolean(right)) => left == right,
        (Value::String(left), Constant::String(right)) => left == right,
        (Value::Null, Constant::Null) | (Value::Undefined, Constant::Undefined) => true,
        _ => false,
    }
}
