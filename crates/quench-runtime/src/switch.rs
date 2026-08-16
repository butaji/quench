use crate::{
    facts::ProgramDb,
    literal::reduce_literal,
    ops::{Constant, Op},
    value::Value,
};
use oxc::ast::ast::SwitchStatement;
use std::collections::HashMap;

type SwitchCases = Vec<(Option<Constant>, crate::machine::FunctionCode)>;

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
        prepare_case_locals(&case.consequent, &mut body_locals, &mut next_slot);
        for statement in &case.consequent {
            if skip_annex_b_function(statement, facts) {
                continue;
            }
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
    let (tests, bodies): (Vec<_>, Vec<_>) = cases.into_iter().unzip();
    let stores = crate::machine::FunctionCode::from_ops_many(bodies);
    Ok(tests.into_iter().zip(stores).collect())
}

fn skip_annex_b_function(statement: &oxc::ast::ast::Statement<'_>, facts: &ProgramDb) -> bool {
    let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
        return false;
    };
    function.id.as_ref().is_some_and(|identifier| {
        facts
            .eval_var_barrier
            .contains(&identifier.name.to_string())
    })
}

fn prepare_case_locals(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for statement in statements {
        let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        let Some(identifier) = function.id.as_ref() else {
            continue;
        };
        let name = identifier.name.as_str();
        if let Some(slot) = locals.get(name).copied() {
            locals
                .entry(format!("\0annex-b-outer:{name}"))
                .or_insert(slot);
        }
        locals.insert(name.to_string(), *next_slot);
        *next_slot = next_slot.saturating_add(1);
    }
}

pub(crate) fn execute(
    registers: &mut Vec<Value>,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Op::Switch {
        discriminant,
        cases,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *discriminant)?;
    let exact = cases.iter().position(|(test, _)| {
        test.as_ref()
            .is_some_and(|test| same_constant(&value, test))
    });
    let default = cases.iter().position(|(test, _)| test.is_none());
    let Some(start) = exact.or(default) else {
        return Ok(crate::completion::Completion::Normal);
    };
    for (_, body) in &cases[start..] {
        let Some(body) = body.ops() else {
            return Err(crate::execute::VmError::MissingReturn);
        };
        match crate::execute::execute_completion_in_place(body, registers)? {
            crate::completion::Completion::Normal => continue,
            crate::completion::Completion::Break(None) => {
                return Ok(crate::completion::Completion::Normal);
            }
            completion => return Ok(completion),
        }
    }
    Ok(crate::completion::Completion::Normal)
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
