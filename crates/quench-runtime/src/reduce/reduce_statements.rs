//! OXC-to-residual reduction entry point.
use crate::{
    blocks::reduce as reduce_block,
    control_flow,
    facts::ProgramDb,
    functions,
    ops::{Builtin, Op},
    statements::reduce_declaration as reduce_declaration_statement,
};
use oxc::{
    allocator::Allocator,
    ast::ast::{Expression, Statement},
    parser::Parser,
    span::SourceType,
};
use std::collections::HashMap;

const GLOBAL_THIS: &str = "globalThis";
const SCRIPT_THIS_SLOT: &str = "\0script_this";
#[derive(Debug, PartialEq)]
pub struct ResidualProgram {
    pub facts: ProgramDb,
    pub ops: Vec<Op>,
}
pub fn reduce_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::cjs())
}
pub fn reduce_module_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::mjs())
}
pub fn reduce_source_with_type(
    source: &str,
    source_type: SourceType,
) -> Result<ResidualProgram, Vec<String>> {
    let (scope_count, symbol_count, ops) = {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, source_type).parse();
        if parsed.panicked {
            return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
        }
        if !parsed.errors.is_empty() {
            return Err(parsed
                .errors
                .iter()
                .map(|error| format!("SyntaxError: {error}"))
                .collect());
        }
        let (scope_count, symbol_count) = crate::semantic::analyze(&parsed.program)?;
        let ops = reduce_statements(&parsed.program.body, source_type, &mut ProgramDb::default())?;
        (scope_count, symbol_count, ops)
    };
    let facts = ProgramDb {
        scope_count,
        symbol_count,
        ..ProgramDb::default()
    };
    Ok(ResidualProgram { facts, ops })
}
fn reduce_statements(
    statements: &[Statement<'_>],
    source_type: SourceType,
    facts: &mut ProgramDb,
) -> Result<Vec<Op>, Vec<String>> {
    let mut locals = HashMap::new();
    let mut ops = Vec::new();
    let mut next_slot: u16 = 0;
    let mut next_register: u16 = 0;
    let global_slot = next_slot;
    next_slot = next_slot.saturating_add(1);
    locals.insert(GLOBAL_THIS.to_string(), global_slot);
    if !source_type.is_module() {
        locals.insert(SCRIPT_THIS_SLOT.to_string(), global_slot);
    }
    let global_properties = global_properties(&mut ops, &mut next_register);
    let global_register = next_register;
    ops.push(Op::MakeObject {
        dst: global_register,
        properties: global_properties,
    });
    ops.push(Op::StoreLocal {
        slot: global_slot,
        src: global_register,
    });
    next_register = global_register.saturating_add(1);
    reduce_statements_opt(
        statements,
        facts,
        locals,
        next_slot,
        true,
        ops,
        next_register,
    )
}

fn global_properties(ops: &mut Vec<Op>, next_register: &mut u16) -> Vec<(String, u16)> {
    let globals = [
        ("Object", Builtin::Object),
        ("Function", Builtin::Function),
        ("Array", Builtin::Array),
        ("Promise", Builtin::Promise),
        ("RegExp", Builtin::RegExp),
        ("Date", Builtin::Date),
        ("Error", Builtin::Error),
        ("TypeError", Builtin::TypeError),
        ("RangeError", Builtin::RangeError),
        ("ReferenceError", Builtin::ReferenceError),
        ("SyntaxError", Builtin::SyntaxError),
        ("EvalError", Builtin::EvalError),
        ("URIError", Builtin::URIError),
    ];
    globals
        .into_iter()
        .map(|(name, builtin)| {
            let register = *next_register;
            *next_register = next_register.saturating_add(1);
            ops.push(Op::MakeBuiltin {
                dst: register,
                builtin,
            });
            (name.to_string(), register)
        })
        .collect()
}
pub fn reduce_statements_with_locals(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops =
        reduce_statements_opt(statements, facts, locals, next_slot, false, Vec::new(), 0)?;
    ops.push(Op::Const {
        dst: 0,
        value: crate::ops::Constant::Undefined,
    });
    ops.push(Op::Return { src: 0 });
    Ok(ops)
}
/// Reduce statements without appending a terminal `Return`. Used for nested
/// blocks whose control flow continues after the block (if/try/switch bodies).
pub fn reduce_statements_no_tail(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    reduce_statements_opt(statements, facts, locals, next_slot, false, Vec::new(), 0)
}
fn reduce_statements_opt(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    mut locals: HashMap<String, u16>,
    mut next_slot: u16,
    tail: bool,
    mut ops: Vec<Op>,
    mut next_register: u16,
) -> Result<Vec<Op>, Vec<String>> {
    let program_scope = !ops.is_empty();
    crate::reduce_support::predeclare_functions(statements, &mut locals, &mut next_slot);
    next_register = next_register.max(crate::reduce_support::register_base(&locals));
    let mut last_value = None;
    for statement in statements {
        if let Some(value) = reduce_statement(
            statement,
            &mut ops,
            facts,
            &mut next_register,
            &mut next_slot,
            &mut locals,
        )? {
            last_value = Some(value);
        }
        if program_scope {
            crate::reduce_support::mirror_script_function(
                statement,
                &locals,
                &mut ops,
                &mut next_register,
            );
        }
    }
    if tail {
        crate::reduce_support::finish_program(ops, last_value)
    } else {
        Ok(ops)
    }
}
pub fn reduce_statement(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    match statement {
        Statement::EmptyStatement(_) => Ok(None),
        Statement::BlockStatement(block) => {
            reduce_block(block, ops, facts, next_register, next_slot, locals)
        }
        Statement::VariableDeclaration(_)
        | Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_) => {
            reduce_declaration_statement(statement, ops, facts, next_register, next_slot, locals)
        }
        Statement::ReturnStatement(return_statement) => {
            control_flow::reduce_return(return_statement, ops, facts, next_register, locals)
        }
        Statement::ThrowStatement(statement) => {
            control_flow::reduce_throw(statement, ops, facts, next_register, locals)
        }
        Statement::ExpressionStatement(expression) => {
            reduce_expression_stmt(&expression.expression, ops, facts, next_register, locals)
                .map(Some)
        }
        statement => {
            reduce_other_statement(statement, ops, facts, next_register, next_slot, locals)
        }
    }
}

fn reduce_expression_stmt(
    expression: &Expression<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<u16, Vec<String>> {
    super::reduce_expressions::reduce_expression_statement(
        expression,
        ops,
        facts,
        next_register,
        locals,
    )
}

fn reduce_other_statement(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    crate::statement_control::reduce(statement, ops, facts, next_register, next_slot, locals)
}
pub fn reduce_function_declaration(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let Some(identifier) = function.id.as_ref() else {
        return Err(vec!["Anonymous function declaration".to_string()]);
    };
    let Some(body) = function.body.as_ref() else {
        return Err(vec!["Function without body".to_string()]);
    };
    let slot = declaration_slot(identifier.name.as_str(), next_slot, locals);
    let (parameters, parameter_count, captures) = function_locals(function, locals)?;
    let body_ops = functions::reduce_body(body, facts, parameters, parameter_count, captures)?;
    let register = take_register(next_register);
    ops.push(Op::MakeFunctionWithKind {
        dst: register,
        body: body_ops,
        params: parameter_count,
        captures,
        kind: crate::ops::FunctionKind::Ordinary,
        strictness: crate::reduce_support::function_strictness(body),
        is_async: function.r#async,
    });
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(())
}

fn declaration_slot(name: &str, next_slot: &mut u16, locals: &mut HashMap<String, u16>) -> u16 {
    if let Some(slot) = locals.get(name) {
        return *slot;
    }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(name.to_string(), slot);
    slot
}

fn take_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}

fn function_locals(
    function: &oxc::ast::ast::Function<'_>,
    locals: &HashMap<String, u16>,
) -> Result<(HashMap<String, u16>, u16, u16), Vec<String>> {
    let (mut parameters, parameter_count) = functions::function_parameters(function)?;
    let captures = locals
        .values()
        .copied()
        .max()
        .map_or(0, |slot| slot.saturating_add(1));
    for slot in parameters.values_mut() {
        *slot = slot.saturating_add(captures);
    }
    let mut bindings = locals.clone();
    bindings.insert(
        "arguments".to_string(),
        captures.saturating_add(parameter_count),
    );
    bindings.insert(
        "this".to_string(),
        captures.saturating_add(parameter_count).saturating_add(1),
    );
    let formal_parameters = parameters.clone();
    bindings.extend(parameters);
    if let Some(body) = &function.body {
        crate::reduce_support::shadow_function_bindings(
            &body.statements,
            &mut bindings,
            &formal_parameters,
        );
    }
    Ok((bindings, parameter_count, captures))
}
