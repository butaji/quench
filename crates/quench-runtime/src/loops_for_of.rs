enum ForOfPattern<'a> {
    Binding(&'a oxc::ast::ast::BindingPattern<'a>),
    Assignment(&'a oxc::ast::ast::AssignmentTarget<'a>),
}

type ForOfSlot<'a> = (u16, bool, Option<ForOfPattern<'a>>);

fn prepend_for_of_binding(
    pattern: ForOfPattern<'_>,
    slot: u16,
    body: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Result<(), Vec<String>> {
    let source = *next_register;
    *next_register = next_register.saturating_add(1);
    let mut binding = vec![Op::LoadLocal { dst: source, slot }];
    let bound = match pattern {
        ForOfPattern::Binding(pattern) => crate::binding_patterns::bind(
            pattern,
            source,
            &mut binding,
            facts,
            next_register,
            locals,
        ),
        ForOfPattern::Assignment(target) => crate::binding_patterns::assign_target(
            target,
            source,
            &mut binding,
            facts,
            next_register,
            locals,
        ),
    };
    bound.ok_or_else(|| vec!["Unsupported for-of binding pattern".to_string()])?;
    binding.append(body);
    *body = binding;
    Ok(())
}

fn for_of_slot<'a>(
    left: &'a oxc::ast::ast::ForStatementLeft<'a>,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> Result<ForOfSlot<'a>, Vec<String>> {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        if let Some(result) = assignment_slot(left, next_slot)? {
            return Ok(result);
        }
        let (slot, per_iteration) = for_in_slot(left, next_slot, locals)?;
        return Ok((slot, per_iteration, None));
    };
    let Some(declarator) = declaration.declarations.first() else {
        return Err(vec!["Missing for-of binding".to_string()]);
    };
    if let oxc::ast::ast::BindingPatternKind::BindingIdentifier(identifier) = &declarator.id.kind {
        let (slot, per_iteration) = named_slot(
            identifier.name.as_str(),
            declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var,
            next_slot,
            locals,
        );
        return Ok((slot, per_iteration, None));
    }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    for name in crate::binding_patterns::names(&declarator.id) {
        named_slot(&name, true, next_slot, locals);
    }
    Ok((
        slot,
        declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var,
        Some(ForOfPattern::Binding(&declarator.id)),
    ))
}

fn assignment_slot<'a>(
    left: &'a oxc::ast::ast::ForStatementLeft<'a>,
    next_slot: &mut u16,
) -> Result<Option<ForOfSlot<'a>>, Vec<String>> {
    let Some(target) = left.as_assignment_target() else {
        return Ok(None);
    };
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    Ok(Some((slot, false, Some(ForOfPattern::Assignment(target)))))
}

fn named_slot(
    name: &str,
    per_iteration: bool,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> (u16, bool) {
    if !per_iteration {
        if let Some(slot) = locals.get(name) {
            return (*slot, per_iteration);
        }
    }
    let slot = *next_slot;
    *next_slot = next_slot.saturating_add(1);
    locals.insert(name.to_string(), slot);
    (slot, per_iteration)
}

fn emit_for_head_tdz(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    ops: &mut Vec<Op>,
    locals: &HashMap<String, u16>,
) {
    if !for_left_lexical(left) {
        return;
    }
    for name in for_declaration_names(left) {
        let Some(&slot) = locals.get(&name) else {
            continue;
        };
        ops.push(Op::MarkUninitialized { slot, shared: true });
    }
}

fn refresh_iteration_locals(
    left: &oxc::ast::ast::ForStatementLeft<'_>,
    next_slot: &mut u16,
    locals: &mut HashMap<String, u16>,
) -> u16 {
    let pattern_binding = matches!(
        left,
        oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration)
            if declaration.declarations.first().is_some_and(|declarator| {
                !matches!(
                    declarator.id.kind,
                    oxc::ast::ast::BindingPatternKind::BindingIdentifier(_)
                )
            })
    );
    let temporary = pattern_binding.then(|| {
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        slot
    });
    let mut first = None;
    for name in for_declaration_names(left) {
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        locals.insert(name.clone(), slot);
        locals.insert(format!("\0lexical-predeclared:{name}"), slot);
        first.get_or_insert(slot);
    }
    temporary.unwrap_or_else(|| first.unwrap_or_else(|| {
        let slot = *next_slot;
        *next_slot = next_slot.saturating_add(1);
        slot
    }))
}

fn for_left_lexical(left: &oxc::ast::ast::ForStatementLeft<'_>) -> bool {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return false;
    };
    declaration.kind != oxc::ast::ast::VariableDeclarationKind::Var
}

fn for_declaration_names(left: &oxc::ast::ast::ForStatementLeft<'_>) -> Vec<String> {
    let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) = left else {
        return Vec::new();
    };
    declaration
        .declarations
        .iter()
        .flat_map(|declarator| crate::binding_patterns::names(&declarator.id))
        .collect()
}
