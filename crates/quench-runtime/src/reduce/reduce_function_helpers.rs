fn reduce_function_body(
    function: &oxc::ast::ast::Function<'_>,
    body: &oxc::ast::ast::FunctionBody<'_>,
    facts: &mut ProgramDb,
    locals: &HashMap<String, u16>,
) -> Result<(Vec<Op>, u16, u16, functions::FunctionMetadata), Vec<String>> {
    let (_, parameter_count) = crate::function_parameters::bindings(&function.params)?;
    let strictness = crate::reduce_support::function_strictness(body, facts.strict);
    let metadata = function_metadata(function, strictness, locals);
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
    locals: &HashMap<String, u16>,
) -> functions::FunctionMetadata {
    functions::FunctionMetadata {
        kind: functions::function_kind(function),
        length: crate::function_parameters::expected_argument_count(&function.params),
        strictness, is_async: function.r#async,
        mapped_arguments: crate::function_parameters::is_simple(&function.params),
        direct_constructor: crate::functions::direct_constructor_fact(function, locals),
        composed_constructor: crate::functions::composed_constructor_fact(function, locals),
    }
}

fn declaration_slot(
    name: &str,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
    _facts: &crate::facts::ProgramDb,
) -> u16 {
    // Function declarations are instantiated before the body is reduced.  A
    // hoisted binding may therefore already have a slot below `next_slot`;
    // advance the allocator past it so a later declaration/local cannot reuse
    // the same slot and replace the function value.
    let existing = locals
        .get(&format!("\0annex-b-lexical:{name}"))
        .or_else(|| locals.get(&format!("\0annex-b-outer:{name}")))
        .or_else(|| locals.get(name))
        .copied();
    if let Some(slot) = existing {
        *next_slot = (*next_slot).max(slot.saturating_add(1));
        return slot;
    }
    reserve_blocked_function(name, next_slot, locals)
}

fn store_annex_b_var(
    ops: &mut Vec<Op>,
    name: &str,
    src: u16,
    locals: &HashMap<String, u16>,
    facts: &crate::facts::ProgramDb,
) {
    if is_eval_barrier(facts, name) {
        return;
    }
    let Some(&outer) = locals.get(&format!("\0annex-b-outer:{name}")) else {
        return;
    };
    ops.push(Op::StoreLocal { slot: outer, src });
}

fn is_eval_barrier(facts: &crate::facts::ProgramDb, name: &str) -> bool {
    facts.eval_var_barrier.iter().any(|bound| bound == name)
        || facts.eval_formals.iter().any(|bound| bound == name)
}

fn reserve_blocked_function(
    name: &str,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> u16 {
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
    let slot = declaration_slot("default", next_slot, locals, facts);
    let (_, parameter_count) = crate::function_parameters::bindings(&function.params)?;
    let (body_ops, captures) = functions::reduce_named_declaration(
        body, &function.params, facts, locals, "default",
        functions::function_kind(function), function.r#async,
    )?;
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    ops.push(function_declaration_op(
        register, body_ops, parameter_count, captures,
        function_metadata(
            function,
            crate::reduce_support::function_strictness(body, facts.strict),
            locals,
        ),
    ));
    ops.push(Op::SetFunctionName { function: register, name: "default".to_string() });
    ops.push(Op::StoreLocal { slot, src: register });
    Ok(())
}
