//! Checks a compilation's parsed program against the shape it expects (`ParsedProgramShape`), and
//! the private names an eval inherits or aliases.

use super::*;

/// Whether `program` has the shape `expected` asks of it: a dynamic function's one function
/// expression, or, for a wrapped eval, its method body at the expected span.
fn has_expected_shape(program: &mut Program<'_>, expected: ParsedProgramShape<'_>, wrapped_eval: bool, ordinary_private_context: bool) -> bool {
    match expected {
        ParsedProgramShape::Any | ParsedProgramShape::KeptRecords | ParsedProgramShape::FirstCallChunk(_) => true,
        ParsedProgramShape::Eval(_, expected_span) => {
            !wrapped_eval || eval_method_body(program, ordinary_private_context).is_some_and(|body| Some(body.span) == expected_span)
        }
        ParsedProgramShape::DynamicFunction { parameter_list_end } => is_dynamic_function_expression(program, parameter_list_end),
    }
}

/// Whether `program` is one parenthesized function expression named by the generated public
/// name, which ends at `parameter_list_end`; the name is then no self binding of the function.
fn is_dynamic_function_expression(program: &mut Program<'_>, parameter_list_end: usize) -> bool {
    let [Statement::ExpressionStatement(statement)] = program.body.as_mut_slice() else {
        return false;
    };
    let Expression::ParenthesizedExpression(expression) = &mut statement.expression else {
        return false;
    };
    let Expression::FunctionExpression(function) = &mut expression.expression else {
        return false;
    };
    let valid = function.id.as_ref().is_some_and(|identifier| identifier.name == DYNAMIC_FUNCTION_PUBLIC_NAME)
        && function.body.is_some()
        && function.params.span.end as usize == parameter_list_end;
    // The generated public source name is not a dynamic Function self binding. OXC still owns
    // the grammar.
    if valid {
        function.id = None;
    }
    valid
}

/// The private names a wrapped eval's class declares as fields: the ids the inherited class
/// keeps, which an eval's wrapper context makes visible.
fn wrapped_class_private_ids(program: &Program<'_>, private_name_ids: &FxHashMap<(u32, u32), u32>) -> FxHashSet<u32> {
    let mut ids = FxHashSet::default();
    if let [Statement::ExpressionStatement(statement)] = program.body.as_slice()
        && let Expression::ClassExpression(class) = statement.expression.without_parentheses()
    {
        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(field) = element {
                let span = field.key.span();
                if let Some(id) = private_name_ids.get(&(span.start, span.end)) {
                    ids.insert(*id);
                }
            }
        }
    }
    ids
}

/// The private names a compilation aliases to the identities it was given (`overrides`, by
/// label), among those visible to it: an eval's inherited ids when it is wrapped. Indexed by
/// the dense private name ids, each `None` where no alias applies.
fn alias_private_names(
    private_name_ids: &FxHashMap<(u32, u32), u32>,
    private_name_labels: &FxHashMap<(u32, u32), String>,
    inherited_private_ids: Option<&FxHashSet<u32>>,
    overrides: &[(String, String)],
) -> Vec<Option<String>> {
    let mut aliases: Vec<Option<String>> = Vec::new();
    for (span, id) in private_name_ids {
        if inherited_private_ids.is_some_and(|ids| !ids.contains(id)) {
            continue;
        }
        let Some(label) = private_name_labels.get(span) else { continue };
        let Some((_, identity)) = overrides.iter().find(|(name, _)| name == label) else { continue };
        let slot = *id as usize;
        if aliases.len() <= slot {
            aliases.resize(slot + 1, None);
        }
        aliases[slot] = Some(identity.clone());
    }
    aliases
}
