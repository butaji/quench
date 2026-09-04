use crate::{
    blocks::reduce as reduce_block, control_flow, facts::ProgramDb, functions, ops::Op,
    ops::PropertyDefinitionKind, statements::reduce_declaration as reduce_declaration_statement,
};
use oxc::{
    allocator::Allocator,
    ast::ast::{Expression, Statement},
    parser::Parser,
    span::{GetSpan, SourceType},
};
use std::collections::HashMap;
const GLOBAL_THIS: &str = "globalThis";
const SCRIPT_THIS_SLOT: &str = "\0script_this";
pub(super) const MODULE_THIS_SLOT: &str = "\0module_this";
pub(super) const IMPORT_META_SLOT: &str = "\0import_meta";
type ReducedStatements = (Vec<Op>, HashMap<String, u16>);
type ReducedProgram = (
    ProgramDb,
    Vec<Op>,
    usize,
    usize,
    Option<crate::reduce::ModuleMetadata>,
    HashMap<String, u16>,
);
include!("function_declaration_ops.rs");
include!("reduce_eval_entry.rs");
#[derive(Debug, PartialEq)]
pub struct ResidualProgram {
    pub facts: ProgramDb,
    code: crate::machine::ExecutableCode,
    pub module_metadata: Option<crate::reduce::ModuleMetadata>,
    pub local_slots: HashMap<String, u16>,
}

impl ResidualProgram {
    pub(crate) fn new(
        facts: ProgramDb,
        ops: Vec<Op>,
        module_metadata: Option<crate::reduce::ModuleMetadata>,
        local_slots: HashMap<String, u16>,
    ) -> Self {
        Self {
            facts,
            code: crate::machine::ExecutableCode::from_ops(ops),
            module_metadata,
            local_slots,
        }
    }

    pub fn code(&self) -> crate::machine::CodeView<'_> {
        self.code.code()
    }
}
pub fn reduce_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::cjs())
}
/// Reduce a script that shares the current global object instead of
/// minting a fresh one. Hosts use this for re-entrant loads (e.g. CJS
/// `require`) so `globalThis` inside the loaded code is the running
/// context's global and sees host-installed values.
pub fn reduce_global_script_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type_and_global(source, SourceType::cjs(), true)
}
pub fn reduce_module_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type_and_global(source, SourceType::mjs(), true)
}

pub fn inspect_module_source(source: &str) -> Result<crate::reduce::ModuleMetadata, Vec<String>> {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::mjs()).parse();
    crate::reduce_support::validate_parse(&parsed)?;
    Ok(crate::reduce::ModuleMetadata::from_statements(
        &parsed.program.body,
    ))
}
pub fn reduce_source_with_type(
    source: &str,
    source_type: SourceType,
) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type_and_global(source, source_type, false)
}
fn reduce_source_with_type_and_global(
    source: &str,
    source_type: SourceType,
    global: bool,
) -> Result<ResidualProgram, Vec<String>> {
    let allocator = Allocator::default();
    if !source_type.is_module() && !global {
        let wrapped = wrap_cjs_source(source);
        let wrapped = crate::reduce_support::prepare_source(&wrapped);
        let parsed = Parser::new(&allocator, wrapped.as_ref(), source_type).parse();
        crate::reduce_support::validate_parse(&parsed)?;
        let (mut facts, ops, scope_count, symbol_count, module_metadata, local_slots) =
            reduce_program(&parsed.program, &wrapped, source_type, global)?;
        facts.scope_count = scope_count;
        facts.symbol_count = symbol_count;
        return Ok(ResidualProgram::new(
            facts,
            ops,
            module_metadata,
            local_slots,
        ));
    }
    let source = crate::reduce_support::prepare_source(source);
    let parsed = Parser::new(&allocator, source.as_ref(), source_type).parse();
    crate::reduce_support::validate_parse(&parsed)?;
    let (mut facts, ops, scope_count, symbol_count, module_metadata, local_slots) =
        reduce_program(&parsed.program, source.as_ref(), source_type, global)?;
    facts.scope_count = scope_count;
    facts.symbol_count = symbol_count;
    Ok(ResidualProgram::new(
        facts,
        ops,
        module_metadata,
        local_slots,
    ))
}

fn wrap_cjs_source(source: &str) -> String {
    let mut output = String::with_capacity(source.len() + 64);
    // Invoke the module wrapper so the CommonJS body actually executes. A
    // bare function expression (previously) left the body inert, so every
    // CJS script swallowed its assertions and reported a vacuous pass. The
    // no-param IIFE lets the body resolve require/module/exports/__filename/
    // __dirname as free identifiers through globalThis, whose values the host
    // prelude installs, avoiding a `.call(args...)` path that would drop the
    // parameter bindings.
    output.push_str("(function(){");
    output.push('\n');
    output.push_str(source);
    output.push_str("\n})()");
    output
}

fn reduce_program(
    program: &oxc::ast::ast::Program<'_>,
    source: &str,
    source_type: SourceType,
    global: bool,
) -> Result<ReducedProgram, Vec<String>> {
    let analysis = crate::semantic::analyze(program)?;
    let strict = source_type.is_module() || crate::reduce_support::has_strict_directive(program);
    let mut facts = ProgramDb {
        strict,
        private_names: analysis.private_names.into_iter().collect(),
        ..ProgramDb::default()
    };
    facts.install_reduction_source(source);
    facts.install_fact_sites(analysis.fact_sites);
    let (ops, local_slots) = reduce_statements(&program.body, source_type, global, &mut facts)?;
    facts.finish_reduction();
    Ok((
        facts,
        ops,
        analysis.scope_count,
        analysis.symbol_count,
        module_metadata(source_type, &program.body),
        local_slots,
    ))
}

fn module_metadata(
    source_type: SourceType,
    statements: &[Statement<'_>],
) -> Option<crate::reduce::ModuleMetadata> {
    source_type
        .is_module()
        .then(|| crate::reduce::ModuleMetadata::from_statements(statements))
}

fn reduce_statements(
    statements: &[Statement<'_>],
    source_type: SourceType,
    global: bool,
    facts: &mut ProgramDb,
) -> Result<ReducedStatements, Vec<String>> {
    let mut state = StatementReducer::new_with_global(source_type, global);
    let last = state.append(statements, facts, true)?;
    Ok((
        crate::reduce_support::finish_program(state.ops, last)?,
        state.locals,
    ))
}
pub(super) struct StatementReducer {
    locals: HashMap<String, u16>,
    pub(super) ops: Vec<Op>,
    next_slot: u16,
    next_register: u16,
    script: bool,
}
include!("statement_reducer.rs");
fn reduce_state_statement(
    state: &mut StatementReducer,
    statement: &Statement<'_>,
    facts: &mut ProgramDb,
    _program_scope: bool,
) -> Result<Option<u16>, Vec<String>> {
    let value = reduce_statement(
        statement,
        &mut state.ops,
        facts,
        &mut state.next_register,
        &mut state.next_slot,
        &mut state.locals,
    )?;
    Ok(value)
}
pub fn reduce_statements_with_locals(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    let (mut ops, _) = reduce_statements_opt(
        statements,
        facts,
        locals,
        next_slot,
        StatementsOptions {
            tail: false,
            eval_behavior: crate::reduce_support::EvalBehavior::Normal,
            directive_completion: None,
        },
    )?;
    ops.push(Op::Const {
        dst: 0,
        value: crate::ops::Constant::Undefined,
    });
    ops.push(Op::Return { src: 0 });
    Ok(ops)
}
pub fn reduce_expression_statements_with_locals(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    reduce_statements_opt(
        statements,
        facts,
        locals,
        next_slot,
        StatementsOptions {
            tail: true,
            eval_behavior: crate::reduce_support::EvalBehavior::Normal,
            directive_completion: None,
        },
    )
    .map(|(ops, _)| ops)
}
pub fn reduce_statements_no_tail(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    reduce_statements_no_tail_value(statements, facts, locals, next_slot).map(|(ops, _)| ops)
}

pub fn reduce_statements_no_tail_value(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<(Vec<Op>, Option<u16>), Vec<String>> {
    reduce_statements_opt(
        statements,
        facts,
        locals,
        next_slot,
        StatementsOptions {
            tail: false,
            eval_behavior: crate::reduce_support::EvalBehavior::Normal,
            directive_completion: None,
        },
    )
}

pub(crate) struct StatementsOptions {
    tail: bool,
    eval_behavior: crate::reduce_support::EvalBehavior,
    directive_completion: Option<String>,
}

fn reduce_statements_opt(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    mut locals: HashMap<String, u16>,
    mut next_slot: u16,
    options: StatementsOptions,
) -> Result<(Vec<Op>, Option<u16>), Vec<String>> {
    let StatementsOptions {
        tail,
        eval_behavior,
        directive_completion,
    } = options;
    let stack = crate::using_scope::reserve(statements, &mut locals, &mut next_slot);
    let await_using = crate::using_scope::has_await_using(statements);
    let (mut ops, mut next_register, initial_value) = initialize_statement_reduction(
        statements,
        facts,
        &mut locals,
        &mut next_slot,
        eval_behavior,
        directive_completion,
    )?;
    crate::using_scope::emit_tdz(statements, &mut ops, &locals);
    if let Some(stack) = stack {
        crate::using_scope::emit_create(&mut ops, stack, await_using, &mut next_register);
    }
    let last_value = reduce_statement_list(
        statements,
        facts,
        &mut ops,
        &mut next_register,
        &mut next_slot,
        &mut locals,
        (eval_behavior, initial_value),
    )?;
    let ops = match stack {
        Some(stack) => crate::using_scope::wrap(ops, stack, await_using, &mut next_register)?,
        None => ops,
    };
    let ops = finish_statements_opt(ops, last_value, tail)?;
    Ok((ops, last_value))
}

fn initialize_statement_reduction(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: &mut HashMap<String, u16>,
    next_slot: &mut u16,
    eval_behavior: crate::reduce_support::EvalBehavior,
    directive_completion: Option<String>,
) -> Result<(Vec<Op>, u16, Option<u16>), Vec<String>> {
    let mut ops = Vec::new();
    let mut next_register = 0;
    if eval_behavior == crate::reduce_support::EvalBehavior::Normal {
        crate::reduce_support::predeclare_functions(statements, locals, next_slot, facts.strict);
    } else {
        for statement in statements {
            let excluded = crate::semantic_early::annex_b_lexical_collisions_in(
                std::slice::from_ref(statement),
            )
            .into_iter()
            .collect::<Vec<_>>();
            crate::reduce_support::predeclare_functions_excluding(
                std::slice::from_ref(statement),
                locals,
                next_slot,
                &excluded,
                false,
            );
        }
    }
    next_register = next_register.max(crate::reduce_support::register_base(locals));
    let eval = eval_behavior != crate::reduce_support::EvalBehavior::Normal;
    let initial_value =
        super::reduce_eval::emit_directive(&mut ops, &mut next_register, directive_completion);
    if eval {
        super::reduce_eval::instantiate_functions(
            statements,
            facts,
            (&mut ops, &mut next_register, next_slot, locals),
            eval_behavior,
        )?;
    }
    Ok((ops, next_register, initial_value))
}

fn finish_statements_opt(
    ops: Vec<Op>,
    last_value: Option<u16>,
    tail: bool,
) -> Result<Vec<Op>, Vec<String>> {
    if tail {
        crate::reduce_support::finish_program(ops, last_value)
    } else {
        Ok(ops)
    }
}
fn reduce_statement_list(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    ops: &mut Vec<Op>,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    completion: (crate::reduce_support::EvalBehavior, Option<u16>),
) -> Result<Option<u16>, Vec<String>> {
    let (eval_behavior, mut last_value) = completion;
    let eval = eval_behavior != crate::reduce_support::EvalBehavior::Normal;
    for statement in statements {
        if eval && matches!(statement, Statement::FunctionDeclaration(_)) {
            continue;
        }
        if let Some(value) =
            reduce_statement(statement, ops, facts, next_register, next_slot, locals)?
        {
            last_value = Some(value);
        }
    }
    Ok(last_value)
}
pub fn reduce_statement(
    statement: &Statement<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<Option<u16>, Vec<String>> {
    trace_statement_site(ops, statement.span().start);
    let last = if statement.is_module_declaration() {
        super::reduce_module::reduce_module_declaration(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )?
    } else {
        reduce_plain_statement(statement, ops, facts, next_register, next_slot, locals)?
    };
    if let Some(value) = last {
        crate::switch::record_completion(ops, value);
    }
    Ok(last)
}

fn trace_statement_site(ops: &mut Vec<Op>, source: u32) {
    ops.push(Op::TraceSite { source });
}

fn reduce_plain_statement(
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
        | Statement::ClassDeclaration(_) => crate::switch::suspend_completion(|| {
            reduce_declaration_statement(statement, ops, facts, next_register, next_slot, locals)
        }),
        Statement::ReturnStatement(rs) => {
            control_flow::reduce_return(rs, ops, facts, next_register, locals)
        }
        Statement::ThrowStatement(ts) => {
            control_flow::reduce_throw(ts, ops, facts, next_register, locals)
        }
        Statement::ExpressionStatement(expression) => {
            reduce_expression_stmt(&expression.expression, ops, facts, next_register, locals)
                .map(Some)
        }
        statement => crate::statement_control::reduce(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        ),
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
    let slot = declaration_slot(identifier.name.as_str(), next_slot, locals, facts);
    let (body_ops, parameter_count, captures, metadata) =
        reduce_function_body(function, body, facts, locals)?;
    let reserve = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(Op::Const {
        dst: reserve,
        value: crate::ops::Constant::Undefined,
    });
    ops.push(Op::StoreLocal { slot, src: reserve });
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(function_declaration_op(
        register,
        body_ops,
        parameter_count,
        captures,
        metadata,
    ));
    name_function_declaration(ops, register, next_register, identifier.name.as_str());
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    store_annex_b_var(ops, identifier.name.as_str(), register, locals, facts);
    Ok(())
}

include!("reduce_function_helpers.rs");
