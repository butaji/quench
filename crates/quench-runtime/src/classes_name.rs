fn set_class_name(class: &oxc::ast::ast::Class<'_>, constructor: u16, ops: &mut Vec<crate::ops::Op>) {
    let Some(identifier) = &class.id else { return };
    ops.push(crate::ops::Op::SetFunctionName {
        function: constructor,
        name: identifier.name.to_string(),
    });
}
