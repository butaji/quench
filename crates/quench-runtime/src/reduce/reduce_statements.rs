use crate::{
    blocks::reduce as reduce_block, control_flow, facts::ProgramDb, functions, ops::Op,
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
pub fn reduce_eval_source(
    source: &str,
    inherited_strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
) -> Result<ResidualProgram, Vec<String>> {
    let strict_source = inherited_strict.then(|| format!("\"use strict\";\n{source}"));
    let source = strict_source.as_deref().unwrap_or(source);
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    validate_parse(&parsed)?;
    let (scope_count, symbol_count) = crate::semantic::analyze(&parsed.program)?;
    let strict = inherited_strict || has_strict_directive(&parsed.program);
    crate::reduce_support::validate_eval_var_names(&parsed.program, strict, forbidden_var_names)?;
    let directive_completion =
        super::reduce_eval::directive_completion(&parsed.program, inherited_strict);
    let mut facts = ProgramDb {
        strict,
        scope_count,
        symbol_count,
        ..ProgramDb::default()
    };
    let (locals, next_slot, mut prefix, behavior) =
        crate::reduce_support::eval_bindings(&parsed.program, bindings, strict, global);
    let mut ops = reduce_statements_opt(
        &parsed.program.body,
        &mut facts,
        locals,
        next_slot,
        true,
        behavior,
        directive_completion,
    )?;
    prefix.append(&mut ops);
    Ok(ResidualProgram { facts, ops: prefix })
}
fn has_strict_directive(program: &oxc::ast::ast::Program<'_>) -> bool {
    program
        .directives
        .iter()
        .any(|directive| directive.directive.as_str() == "use strict")
}
fn validate_parse(parsed: &oxc::parser::ParserReturn<'_>) -> Result<(), Vec<String>> {
    if parsed.panicked {
        return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
    }
    if parsed.errors.is_empty() {
        return Ok(());
    }
    Err(parsed
        .errors
        .iter()
        .map(|error| format!("SyntaxError: {error}"))
        .collect())
}
pub fn reduce_source_with_type(
    source: &str,
    source_type: SourceType,
) -> Result<ResidualProgram, Vec<String>> {
    let (scope_count, symbol_count, ops) = {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, source_type).parse();
        validate_parse(&parsed)?;
        let (scope_count, symbol_count) = crate::semantic::analyze(&parsed.program)?;
        let strict = source_type.is_module()
            || parsed
                .program
                .directives
                .iter()
                .any(|directive| directive.directive.as_str() == "use strict");
        let mut facts = ProgramDb {
            strict,
            ..ProgramDb::default()
        };
        let ops = reduce_statements(&parsed.program.body, source_type, &mut facts)?;
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
    let mut state = StatementReducer::new(source_type);
    let last = state.append(statements, facts, true)?;
    crate::reduce_support::finish_program(state.ops, last)
}
pub(super) struct StatementReducer {
    locals: HashMap<String, u16>,
    pub(super) ops: Vec<Op>,
    next_slot: u16,
    next_register: u16,
}
impl StatementReducer {
    pub(super) fn new(source_type: SourceType) -> Self {
        let mut locals = HashMap::from([(GLOBAL_THIS.to_string(), 0)]);
        if !source_type.is_module() {
            locals.insert(SCRIPT_THIS_SLOT.to_string(), 0);
        }
        let mut ops = Vec::new();
        let mut next_register = 0;
        let properties = crate::globals::script_properties(&mut ops, &mut next_register);
        ops.push(Op::MakeObject {
            dst: next_register,
            properties,
        });
        ops.push(Op::StoreLocal {
            slot: 0,
            src: next_register,
        });
        next_register = next_register.saturating_add(1);
        Self {
            locals,
            ops,
            next_slot: 1,
            next_register,
        }
    }
    pub(super) fn append(
        &mut self,
        statements: &[Statement<'_>],
        facts: &mut ProgramDb,
        program_scope: bool,
    ) -> Result<Option<u16>, Vec<String>> {
        crate::reduce_support::predeclare_functions(
            statements,
            &mut self.locals,
            &mut self.next_slot,
        );
        self.next_register = self
            .next_register
            .max(crate::reduce_support::register_base(&self.locals));
        let mut last = None;
        for statement in statements {
            last = reduce_state_statement(self, statement, facts, program_scope)?.or(last);
        }
        Ok(last)
    }
}
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
    if program_scope {
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
        false,
        crate::reduce_support::EvalBehavior::Normal,
        None,
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
        true,
        crate::reduce_support::EvalBehavior::Normal,
        None,
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
        false,
        crate::reduce_support::EvalBehavior::Normal,
        None,
    )
}
fn reduce_statements_opt(
    statements: &[Statement<'_>],
    facts: &mut ProgramDb,
    mut locals: HashMap<String, u16>,
    mut next_slot: u16,
    tail: bool,
    eval_behavior: crate::reduce_support::EvalBehavior,
    directive_completion: Option<String>,
) -> Result<Vec<Op>, Vec<String>> {
    let mut ops = Vec::new();
    let mut next_register = 0;
    crate::reduce_support::predeclare_functions(statements, &mut locals, &mut next_slot);
    next_register = next_register.max(crate::reduce_support::register_base(&locals));
    let eval = eval_behavior != crate::reduce_support::EvalBehavior::Normal;
    let initial_value =
        super::reduce_eval::emit_directive(&mut ops, &mut next_register, directive_completion);
    if eval {
        super::reduce_eval::instantiate_functions(
            statements,
            facts,
            (&mut ops, &mut next_register, &mut next_slot, &mut locals),
            eval_behavior,
        )?;
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
        if eval_behavior == crate::reduce_support::EvalBehavior::Global {
            crate::reduce_support::mirror_script_bindings(statement, locals, ops, next_register);
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
    let strictness = crate::reduce_support::function_strictness(body, facts.strict);
    let body_ops = functions::reduce_body(
        body,
        &function.params,
        facts,
        parameters,
        parameter_count,
        captures,
        strictness,
    )?;
    let register = take_register(next_register);
    ops.push(function_declaration_op(
        register,
        body_ops,
        parameter_count,
        captures,
        function_metadata(function, strictness),
    ));
    store_function(ops, slot, register);
    Ok(())
}
fn function_metadata(
    function: &oxc::ast::ast::Function<'_>,
    strictness: crate::ops::FunctionStrictness,
) -> functions::FunctionMetadata {
    functions::FunctionMetadata {
        kind: functions::function_kind(function),
        strictness,
        is_async: function.r#async,
        mapped_arguments: crate::function_parameters::is_simple(&function.params),
    }
}
fn store_function(ops: &mut Vec<Op>, slot: u16, src: u16) {
    ops.push(Op::StoreLocal { slot, src });
}
fn function_declaration_op(
    dst: u16,
    body: Vec<Op>,
    params: u16,
    captures: u16,
    metadata: functions::FunctionMetadata,
) -> Op {
    Op::MakeFunctionWithKind {
        dst,
        body,
        params,
        captures,
        kind: metadata.kind,
        strictness: metadata.strictness,
        is_async: metadata.is_async,
        mapped_arguments: metadata.mapped_arguments,
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
    bindings.extend(parameters);
    Ok((bindings, parameter_count, captures))
}
