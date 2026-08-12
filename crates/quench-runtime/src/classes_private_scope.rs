pub(crate) fn reduce_expression(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let names = private_definitions(class, facts);
    let mut body = Vec::new();
    let heritage = reduce_heritage(class, &mut body, facts, next, locals)?;
    let (constructor, default_constructor) =
        reduce_constructor(class, &mut body, facts, next, locals)?;
    finish_class(
        class,
        heritage,
        (constructor, default_constructor),
        &mut body,
        facts,
        next,
        locals,
    )?;
    ops.push(Op::PrivateScope {
        names,
        body: crate::machine::FunctionCode::from_ops(body),
    });
    Some(constructor)
}

fn finish_class(
    class: &Class<'_>,
    heritage: Option<u16>,
    constructor: (u16, bool),
    body: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<()> {
    let (constructor, default_constructor) = constructor;
    set_class_name(class, constructor, body);
    let prototype = configure_class(heritage, constructor, default_constructor, body, next);
    let static_fields = reduce_elements(
        class,
        prototype,
        constructor,
        body,
        facts,
        next,
        locals,
    )?;
    body.push(Op::SetProperty {
        object: constructor,
        key: "prototype".to_string(),
        src: prototype,
        strict: true,
    });
    body.extend(static_fields);
    Some(())
}

fn configure_class(
    heritage: Option<u16>,
    constructor: u16,
    default_constructor: bool,
    ops: &mut Vec<Op>,
    next: &mut u16,
) -> u16 {
    let prototype = emit_object(ops, next, Vec::new());
    configure_heritage(
        heritage,
        constructor,
        prototype,
        default_constructor,
        ops,
        next,
    );
    define_static_key(
        ops,
        next,
        prototype,
        "constructor",
        constructor,
        PropertyDefinitionKind::Data,
    );
    prototype
}

fn private_definitions(class: &Class<'_>, facts: &ProgramDb) -> Vec<crate::facts::PrivateNameId> {
    let mut names = Vec::new();
    for element in &class.body.body {
        let key = match element {
            ClassElement::MethodDefinition(method) => Some(&method.key),
            ClassElement::PropertyDefinition(field) => Some(&field.key),
            _ => None,
        };
        let Some(PropertyKey::PrivateIdentifier(name)) = key else {
            continue;
        };
        if let Some(id) = facts.private_name(name.span) {
            if !names.contains(&id) {
                names.push(id);
            }
        }
    }
    names
}
