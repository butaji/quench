pub(crate) fn reduce_expression(
    class: &Class<'_>,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    let (names, labels) = private_definitions(class, facts);
    let inferred = facts.inferred_name.clone();
    let mut body = Vec::new();
    let class_locals = class_scope_locals(class, locals);
    let inherited_strict = facts.strict;
    facts.strict = true;
    let heritage = reduce_heritage(class, &mut body, facts, next, &class_locals);
    let constructor = heritage.and_then(|heritage| {
        let (constructor, default_constructor) =
            reduce_constructor(class, &mut body, facts, next, &class_locals)?;
        finish_class(
            class,
            heritage,
            (constructor, default_constructor),
            &mut body,
            facts,
            next,
            &class_locals,
            inferred.as_deref(),
        )?;
        Some((constructor, heritage))
    });
    facts.strict = inherited_strict;
    let (constructor, _) = constructor?;
    ops.push(Op::PrivateScope {
        names,
        labels,
        class_name: class.id.as_ref().map(|id| id.name.to_string()),
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
    inferred: Option<&str>,
) -> Option<()> {
    let (constructor, default_constructor) = constructor;
    set_class_name(class, constructor, inferred, body);
    let prototype = configure_class(heritage, constructor, default_constructor, body, next);
    define_class_prototype(body, next, constructor, prototype);
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
        key: "\0home_object".to_string(),
        src: prototype,
        strict: true,
    });
    body.push(Op::SetProperty {
        object: constructor,
        key: "\0class_constructor".to_string(),
        src: constructor,
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

fn private_definitions(
    class: &Class<'_>,
    facts: &ProgramDb,
) -> (Vec<crate::facts::PrivateNameId>, Vec<String>) {
    let mut names = Vec::new();
    let mut labels = Vec::new();
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
                labels.push(name.name.to_string());
            }
        }
    }
    (names, labels)
}
