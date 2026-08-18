fn set_class_name(class: &oxc::ast::ast::Class<'_>, constructor: u16, ops: &mut Vec<crate::ops::Op>) {
    let Some(identifier) = &class.id else { return };
    ops.push(crate::ops::Op::SetFunctionName {
        function: constructor,
        name: identifier.name.to_string(),
    });
    ops.push(crate::ops::Op::SetName {
        key: identifier.name.to_string(),
        src: constructor,
        strict: true,
    });
}

/// Class scope hides an outer `var`/`let` of the same name so members resolve
/// the inner immutable class binding via `ResolveName`.
fn class_scope_locals(
    class: &oxc::ast::ast::Class<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> std::collections::HashMap<String, u16> {
    let Some(identifier) = &class.id else {
        return locals.clone();
    };
    let mut class_locals = locals.clone();
    class_locals.remove(identifier.name.as_str());
    class_locals
}
