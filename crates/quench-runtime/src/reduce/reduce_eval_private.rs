fn wrap_eval_for_private(source: &str) -> Option<String> {
    let current = crate::private::environment::current();
    if !current.has_names() {
        return None;
    }
    let labels = private_labels(source);
    if labels.is_empty() {
        return None;
    }
    let fields = labels
        .iter()
        .map(|label| format!("  #{label};"))
        .collect::<Vec<_>>()
        .join("\n");
    Some(format!(
        "class __QuenchEvalPrivate {{\n{fields}\n  __body() {{ {source}\n  }}\n}}"
    ))
}

fn private_labels(source: &str) -> Vec<String> {
    let mut labels = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' && index + 1 < bytes.len() && is_ident_start(bytes[index + 1]) {
            let start = index + 1;
            let mut end = start + 1;
            while end < bytes.len() && is_ident_continue(bytes[end]) {
                end += 1;
            }
            let name = source[start..end].to_string();
            if !labels.iter().any(|label| label == &name) {
                labels.push(name);
            }
            index = end;
        } else {
            index += 1;
        }
    }
    labels
}

fn is_ident_start(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphabetic()
}

fn is_ident_continue(byte: u8) -> bool {
    is_ident_start(byte) || byte.is_ascii_digit()
}

fn wrapped_method_body<'a>(
    program: &'a oxc::ast::ast::Program<'a>,
) -> Option<&'a [oxc::ast::ast::Statement<'a>]> {
    let oxc::ast::ast::Statement::ClassDeclaration(class) = program.body.first()? else {
        return None;
    };
    for element in &class.body.body {
        let oxc::ast::ast::ClassElement::MethodDefinition(method) = element else {
            continue;
        };
        if let Some(body) = &method.value.body {
            return Some(&body.statements);
        }
    }
    None
}

fn remap_eval_private_ids(facts: &mut ProgramDb, program: &oxc::ast::ast::Program<'_>) {
    collect_private_idents(&program.body, facts);
}

fn collect_private_idents(statements: &[oxc::ast::ast::Statement<'_>], facts: &mut ProgramDb) {
    for statement in statements {
        if let oxc::ast::ast::Statement::ClassDeclaration(class) = statement {
            remap_class_privates(class, facts);
        }
    }
}

fn remap_class_privates(class: &oxc::ast::ast::Class<'_>, facts: &mut ProgramDb) {
    for element in &class.body.body {
        let key = match element {
            oxc::ast::ast::ClassElement::MethodDefinition(method) => Some(&method.key),
            oxc::ast::ast::ClassElement::PropertyDefinition(field) => Some(&field.key),
            _ => None,
        };
        if let Some(oxc::ast::ast::PropertyKey::PrivateIdentifier(name)) = key {
            remap_span(facts, name.span, name.name.as_str());
        }
        if let oxc::ast::ast::ClassElement::MethodDefinition(method) = element {
            if let Some(body) = &method.value.body {
                remap_statement_privates(&body.statements, facts);
            }
        }
    }
}

fn remap_statement_privates(statements: &[oxc::ast::ast::Statement<'_>], facts: &mut ProgramDb) {
    for statement in statements {
        if let oxc::ast::ast::Statement::ExpressionStatement(expression) = statement {
            remap_expression_privates(&expression.expression, facts);
        }
        if let oxc::ast::ast::Statement::ReturnStatement(returned) = statement {
            if let Some(argument) = &returned.argument {
                remap_expression_privates(argument, facts);
            }
        }
    }
}

fn remap_expression_privates(expression: &oxc::ast::ast::Expression<'_>, facts: &mut ProgramDb) {
    match expression {
        oxc::ast::ast::Expression::PrivateFieldExpression(field) => {
            remap_span(facts, field.field.span, field.field.name.as_str());
            remap_expression_privates(&field.object, facts);
        }
        oxc::ast::ast::Expression::PrivateInExpression(inner) => {
            remap_span(facts, inner.left.span, inner.left.name.as_str());
            remap_expression_privates(&inner.right, facts);
        }
        oxc::ast::ast::Expression::CallExpression(call) => {
            remap_expression_privates(&call.callee, facts);
        }
        oxc::ast::ast::Expression::StaticMemberExpression(member) => {
            remap_expression_privates(&member.object, facts);
        }
        oxc::ast::ast::Expression::ComputedMemberExpression(member) => {
            remap_expression_privates(&member.object, facts);
        }
        oxc::ast::ast::Expression::ParenthesizedExpression(inner) => {
            remap_expression_privates(&inner.expression, facts);
        }
        _ => {}
    }
}

fn remap_span(facts: &mut ProgramDb, span: oxc::span::Span, label: &str) {
    let Some(real) = crate::private::environment::current().id_for_label(label) else {
        return;
    };
    facts.insert_private_name(span, real);
}
