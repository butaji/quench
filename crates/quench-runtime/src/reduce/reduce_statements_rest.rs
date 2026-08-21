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
