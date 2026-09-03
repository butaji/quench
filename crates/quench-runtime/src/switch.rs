use crate::{
    facts::ProgramDb,
    ops::{Constant, Op},
    value::Value,
};
use oxc::ast::ast::SwitchStatement;
use std::cell::Cell;
use std::collections::HashMap;

type SwitchCases = Vec<(
    Option<crate::machine::FunctionCode>,
    crate::machine::FunctionCode,
)>;

/// The sole consumer of a statement's normal completion value while reducing
/// a nested statement. `UpdateEmpty` is the ES abrupt-completion transport;
/// it is not a general-purpose dead-result marker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatementResultUse {
    /// A parent expression or eval result may observe the produced value.
    Value,
    /// A loop, switch, or finally needs the last non-empty value to complete
    /// an otherwise empty `break`/`continue`.
    UpdateEmpty(u16),
    /// Ordinary function completion always returns `undefined`; normal
    /// statement values have no observer after this boundary.
    Unobservable,
}

thread_local! {
    static RESULT_USE: Cell<StatementResultUse> = const { Cell::new(StatementResultUse::Value) };
}

pub(crate) fn record_completion(ops: &mut Vec<Op>, src: u16) {
    if let StatementResultUse::UpdateEmpty(dst) = RESULT_USE.get() {
        ops.push(Op::Move { dst, src });
    }
}

pub(crate) fn suspend_completion<T>(reduce: impl FnOnce() -> T) -> T {
    with_result_use(StatementResultUse::Value, reduce)
}

pub(crate) fn with_completion<T>(dst: u16, reduce: impl FnOnce() -> T) -> T {
    with_result_use(StatementResultUse::UpdateEmpty(dst), reduce)
}

pub(crate) fn with_unobservable_completion<T>(reduce: impl FnOnce() -> T) -> T {
    with_result_use(StatementResultUse::Unobservable, reduce)
}

pub(crate) fn statement_result_use() -> StatementResultUse {
    RESULT_USE.get()
}

fn with_result_use<T>(use_: StatementResultUse, reduce: impl FnOnce() -> T) -> T {
    let previous = RESULT_USE.replace(use_);
    let result = reduce();
    RESULT_USE.set(previous);
    result
}

pub(crate) fn take_completion_register(ops: &mut Vec<Op>, next: &mut u16) -> u16 {
    let dst = *next;
    *next = next.saturating_add(1);
    ops.push(Op::Const {
        dst,
        value: Constant::Undefined,
    });
    dst
}

pub(crate) fn reduce(
    statement: &SwitchStatement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    let discriminant = crate::reduce::reduce_expression(
        &statement.discriminant,
        ops,
        facts,
        next_register,
        locals,
    )
    .ok_or_else(|| vec!["Unsupported switch discriminant".to_string()])?;
    let dst = take_switch_register(next_register);
    ops.push(Op::Const {
        dst,
        value: Constant::Undefined,
    });
    let mut next_slot = crate::reduce_support::register_base(locals);
    let mut block_locals = locals.clone();
    instantiate_case_block(statement, ops, &mut block_locals, &mut next_slot);
    let previous = RESULT_USE.replace(StatementResultUse::UpdateEmpty(dst));
    let cases = reduce_cases(
        statement,
        facts,
        next_register,
        &mut next_slot,
        &mut block_locals,
    );
    RESULT_USE.set(previous);
    let cases = cases?;
    ops.push(Op::Switch {
        discriminant,
        cases,
        dst,
    });
    Ok(Some(dst))
}

fn take_switch_register(next: &mut u16) -> u16 {
    let dst = *next;
    *next = next.saturating_add(1);
    dst
}

fn reduce_cases(
    statement: &SwitchStatement<'_>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    block_locals: &mut HashMap<String, u16>,
) -> Result<SwitchCases, Vec<String>> {
    let mut cases = Vec::new();
    for case in &statement.cases {
        let test = case
            .test
            .as_ref()
            .map(|test| reduce_case_test(test, facts, next_register, block_locals))
            .transpose()?;
        let mut body = Vec::new();
        for statement in &case.consequent {
            crate::reduce::reduce_statement(
                statement,
                &mut body,
                facts,
                next_register,
                next_slot,
                block_locals,
            )?;
        }
        cases.push((test, body));
    }
    let (tests, bodies): (Vec<_>, Vec<_>) = cases.into_iter().unzip();
    let tests = tests
        .into_iter()
        .map(|test| test.map(crate::machine::FunctionCode::from_ops))
        .collect::<Vec<_>>();
    let stores = crate::machine::FunctionCode::pending_many(bodies);
    Ok(tests.into_iter().zip(stores).collect())
}

fn reduce_case_test(
    test: &oxc::ast::ast::Expression<'_>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops = Vec::new();
    let src = crate::reduce::reduce_expression(test, &mut ops, facts, next_register, locals)
        .ok_or_else(|| vec!["Unsupported switch case".to_string()])?;
    ops.push(Op::Return { src });
    Ok(ops)
}

fn instantiate_case_block(
    statement: &SwitchStatement<'_>,
    ops: &mut Vec<Op>,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
) {
    for name in crate::reduce_support::annex_b_switch_function_names(statement) {
        crate::control_flow::preserve_annex_b_outer(locals, &name);
    }
    for case in &statement.cases {
        crate::reduce_support::predeclare_lexicals(&case.consequent, locals, next_slot);
        crate::using_scope::emit_tdz(&case.consequent, ops, locals);
        prepare_case_functions(&case.consequent, locals, next_slot, ops);
    }
}

fn prepare_case_functions(
    statements: &[oxc::ast::ast::Statement<'_>],
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
    ops: &mut Vec<Op>,
) {
    for statement in statements {
        let oxc::ast::ast::Statement::FunctionDeclaration(function) = statement else {
            continue;
        };
        let Some(identifier) = function.id.as_ref() else {
            continue;
        };
        let name = identifier.name.as_str();
        if locals.contains_key(&format!("\0annex-b-lexical:{name}")) {
            continue;
        }
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(name.to_string(), slot);
        locals.insert(format!("\0annex-b-lexical:{name}"), slot);
        ops.push(Op::MarkUninitialized { slot, shared: true });
    }
}

pub(crate) fn execute(
    registers: &mut crate::register_file::RegisterFile,
    op: &Op,
) -> Result<crate::completion::Completion, crate::execute::VmError> {
    let Op::Switch {
        discriminant,
        cases,
        dst,
    } = op
    else {
        return Err(crate::execute::VmError::MissingReturn);
    };
    let value = crate::execute::read_register(registers, *discriminant)?;
    let start = match_case(cases, &value, registers)?;
    let Some(start) = start else {
        return Ok(crate::completion::Completion::Normal);
    };
    for (_, body) in &cases[start..] {
        match crate::vm::execute_function_code_completion_in_current_frame(body, registers)? {
            crate::completion::Completion::Normal => continue,
            crate::completion::Completion::Break { label: None, .. } => {
                return Ok(crate::completion::Completion::Normal);
            }
            crate::completion::Completion::Continue { label, value: None } => {
                let value = crate::execute::read_register(registers, *dst).ok();
                return Ok(crate::completion::Completion::Continue { label, value });
            }
            completion => return Ok(completion),
        }
    }
    Ok(crate::completion::Completion::Normal)
}

fn match_case(
    cases: &[(
        Option<crate::machine::FunctionCode>,
        crate::machine::FunctionCode,
    )],
    value: &Value,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Option<usize>, crate::execute::VmError> {
    let mut default = None;
    for (index, (test, _)) in cases.iter().enumerate() {
        let Some(test) = test else {
            default = Some(index);
            continue;
        };
        if crate::equality::strict_equal(&evaluate_case_test(test, registers)?, value) {
            return Ok(Some(index));
        }
    }
    Ok(default)
}

fn evaluate_case_test(
    test: &crate::machine::FunctionCode,
    registers: &mut crate::register_file::RegisterFile,
) -> Result<Value, crate::execute::VmError> {
    match crate::vm::execute_function_code_completion_in_current_frame(test, registers)? {
        crate::completion::Completion::Return(value) => Ok(value),
        crate::completion::Completion::Normal => Ok(Value::Undefined),
        crate::completion::Completion::Throw(value) => Err(crate::execute::VmError::Thrown(value)),
        _ => Err(crate::execute::VmError::MissingReturn),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        statement_result_use, with_completion, with_unobservable_completion, StatementResultUse,
    };

    #[test]
    fn statement_result_use_restores_nested_consumers() {
        assert_eq!(statement_result_use(), StatementResultUse::Value);
        with_unobservable_completion(|| {
            assert_eq!(statement_result_use(), StatementResultUse::Unobservable);
            with_completion(7, || {
                assert_eq!(statement_result_use(), StatementResultUse::UpdateEmpty(7));
            });
            assert_eq!(statement_result_use(), StatementResultUse::Unobservable);
        });
        assert_eq!(statement_result_use(), StatementResultUse::Value);
    }
}
