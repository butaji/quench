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
        body, &function.params, facts, locals,
        function.id.as_ref().map_or("", |id| id.name.as_str()),
        functions::function_kind(function), function.r#async,
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
        strictness, is_async: function.r#async,
        mapped_arguments: crate::function_parameters::is_simple(&function.params),
    }
}

fn declaration_slot(name: &str, next_slot: &mut u16, locals: &mut HashMap<String, u16>) -> u16 {
    if let Some(slot) = locals.get(&format!("\0annex-b-outer:{name}")) { return *slot; }
    if let Some(slot) = locals.get(name) { return *slot; }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(name.to_string(), slot);
    slot
}
pub fn reduce_default_function_declaration(
    function: &oxc::ast::ast::Function<'_>, ops: &mut Vec<Op>, facts: &mut ProgramDb,
    next_register: &mut u16, next_slot: &mut u16, locals: &mut HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let Some(body) = function.body.as_ref() else {
        return Err(vec!["Function without body".to_string()]);
    };
    let slot = declaration_slot("default", next_slot, locals);
    let (_, parameter_count) = crate::function_parameters::bindings(&function.params)?;
    let (body_ops, captures) = functions::reduce_named_declaration(
        body, &function.params, facts, locals, "default",
        functions::function_kind(function), function.r#async,
    )?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(function_declaration_op(
        register, body_ops, parameter_count, captures,
        function_metadata(function, crate::reduce_support::function_strictness(body, facts.strict)),
    ));
    ops.push(Op::SetFunctionName { function: register, name: "default".to_string() });
    ops.push(Op::StoreLocal { slot, src: register });
    Ok(())
}
