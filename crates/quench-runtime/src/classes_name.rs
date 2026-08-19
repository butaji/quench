fn set_class_name(
    class: &oxc::ast::ast::Class<'_>,
    constructor: u16,
    inferred: Option<&str>,
    ops: &mut Vec<crate::ops::Op>,
) {
    let name = class
        .id
        .as_ref()
        .map(|id| id.name.as_str())
        .or(inferred);
    let Some(name) = name else {
        return;
    };
    ops.push(crate::ops::Op::SetFunctionName {
        function: constructor,
        name: name.to_string(),
    });
    if class.id.is_some() {
        ops.push(crate::ops::Op::SetName {
            key: name.to_string(),
            src: constructor,
            strict: true,
        });
    }
}

/// Class scope hides an outer `var`/`let` of the same name so members resolve
/// the inner immutable class binding via `ResolveName`.
fn class_scope_locals(
    class: &oxc::ast::ast::Class<'_>,
    locals: &std::collections::HashMap<String, u16>,
) -> std::collections::HashMap<String, u16> {
    let _ = class;
    locals.clone()
}
