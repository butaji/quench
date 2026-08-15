use crate::{
    blocks::reduce as reduce_block, control_flow, facts::ProgramDb, functions, ops::Op,
    ops::PropertyDefinitionKind, statements::reduce_declaration as reduce_declaration_statement,
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

    pub fn ops(&self) -> &[Op] {
        self.code.ops()
    }
}
pub fn reduce_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::cjs())
}
pub(crate) fn reduce_global_script_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type_and_global(source, SourceType::cjs(), true)
}
pub fn reduce_module_source(source: &str) -> Result<ResidualProgram, Vec<String>> {
    reduce_source_with_type(source, SourceType::mjs())
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
    let parsed = Parser::new(&allocator, source, source_type).parse();
    crate::reduce_support::validate_parse(&parsed)?;
    let (mut facts, ops, scope_count, symbol_count, module_metadata, local_slots) =
        reduce_program(&parsed.program, source, source_type, global)?;
    facts.scope_count = scope_count;
    facts.symbol_count = symbol_count;
    Ok(ResidualProgram::new(
        facts,
        ops,
        module_metadata,
        local_slots,
    ))
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
    program_scope: bool,
) -> Result<Option<u16>, Vec<String>> {
    let value = reduce_statement(
        statement,
        &mut state.ops,
        facts,
        &mut state.next_register,
        &mut state.next_slot,
        &mut state.locals,
    )?;
    if program_scope && !state.script {
        crate::reduce_support::mirror_script_bindings(
            statement,
            &state.locals,
            &mut state.ops,
            &mut state.next_register,
        );
    }
    Ok(value)
}
pub fn reduce_statements_with_locals(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops = reduce_statements_opt(
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
}
/// Reduce statements without appending a terminal `Return`. Used for nested
/// blocks whose control flow continues after the block (if/try/switch bodies).
pub fn reduce_statements_no_tail(
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
) -> Result<Vec<Op>, Vec<String>> {
    let StatementsOptions {
        tail,
        eval_behavior,
        directive_completion,
    } = options;
    let (mut ops, mut next_register, initial_value) = initialize_statement_reduction(
        statements,
        facts,
        &mut locals,
        &mut next_slot,
        eval_behavior,
        directive_completion,
    )?;
    let last_value = reduce_statement_list(
        statements,
        facts,
        &mut ops,
        &mut next_register,
        &mut next_slot,
        &mut locals,
        (eval_behavior, initial_value),
    )?;
    finish_statements_opt(ops, last_value, tail)
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
    crate::reduce_support::predeclare_functions(statements, locals, next_slot);
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
    if statement.is_module_declaration() {
        super::reduce_module::reduce_module_declaration(
            statement,
            ops,
            facts,
            next_register,
            next_slot,
            locals,
        )
    } else {
        reduce_plain_statement(statement, ops, facts, next_register, next_slot, locals)
    }
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
        | Statement::ClassDeclaration(_) => {
            reduce_declaration_statement(statement, ops, facts, next_register, next_slot, locals)
        }
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
    let slot = declaration_slot(identifier.name.as_str(), next_slot, locals);
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
    Ok(())
}

pub fn reduce_default_function_declaration(
    function: &oxc::ast::ast::Function<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let Some(body) = function.body.as_ref() else {
        return Err(vec!["Function without body".to_string()]);
    };
    let slot = declaration_slot("default", next_slot, locals);
    let (_, parameter_count) = crate::function_parameters::bindings(&function.params)?;
    let (body_ops, captures) = functions::reduce_named_declaration(
        body,
        &function.params,
        facts,
        locals,
        "default",
        functions::function_kind(function),
        function.r#async,
    )?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(function_declaration_op(
        register,
        body_ops,
        parameter_count,
        captures,
        function_metadata(
            function,
            crate::reduce_support::function_strictness(body, facts.strict),
        ),
    ));
    ops.push(Op::SetFunctionName {
        function: register,
        name: "default".to_string(),
    });
    ops.push(Op::StoreLocal {
        slot,
        src: register,
    });
    Ok(())
}
fn reduce_function_body(
    function: &oxc::ast::ast::Function<'_>,
    body: &oxc::ast::ast::FunctionBody<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<(Vec<Op>, u16, u16, functions::FunctionMetadata), Vec<String>> {
    let (_, parameter_count) = crate::function_parameters::bindings(&function.params)?;
    let strictness = crate::reduce_support::function_strictness(body, facts.strict);
    let metadata = function_metadata(function, strictness);
    let (body_ops, captures) = functions::reduce_named_declaration(
        body,
        &function.params,
        facts,
        locals,
        function.id.as_ref().map_or("", |id| id.name.as_str()),
        functions::function_kind(function),
        function.r#async,
    )?;
    Ok((body_ops, parameter_count, captures, metadata))
}
fn function_metadata(
    function: &oxc::ast::ast::Function<'_>,
    strictness: crate::ops::FunctionStrictness,
) -> functions::FunctionMetadata {
    functions::FunctionMetadata {
        kind: functions::function_kind(function),
        length: crate::function_parameters::expected_argument_count(&function.params),
        strictness,
        is_async: function.r#async,
        mapped_arguments: crate::function_parameters::is_simple(&function.params),
    }
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
