//! Syntax-owned projection of the configured SDK crypto factory declarations.
use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey, Statement};
use oxc_span::GetSpan;
pub(crate) fn factory(source: &str) -> Result<String, &'static str> {
    project(source, ["Hash", "Hmac"], ["createHash", "createHmac"])
}
pub(crate) fn signature_factory(source: &str) -> Result<String, &'static str> {
    project(source, ["Sign", "Verify"], ["createSign", "createVerify"])
}
fn project(
    source: &str,
    class_names: [&str; 2],
    factory_names: [&str; 2],
) -> Result<String, &'static str> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err("Configured crypto source does not parse");
    }
    let mut declarations = std::collections::BTreeMap::new();
    let mut classes = std::collections::BTreeMap::new();
    for statement in &parsed.program.body {
        if let Statement::FunctionDeclaration(function) = statement {
            if let Some(name) = function
                .id
                .as_ref()
                .map(|id| id.name.as_str())
                .filter(|name| factory_names.contains(name))
            {
                if declarations
                    .insert(name, function.span().source_text(source))
                    .is_some()
                {
                    return Err("Ambiguous configured crypto factory");
                }
            }
        }
        let Statement::ExpressionStatement(statement) = statement else {
            continue;
        };
        let Expression::AssignmentExpression(assignment) = &statement.expression else {
            continue;
        };
        let Some(oxc_ast::ast::SimpleAssignmentTarget::StaticMemberExpression(member)) =
            assignment.left.as_simple_assignment_target()
        else {
            continue;
        };
        if !matches!(&member.object,Expression::Identifier(id) if id.name=="module")
            || member.property.name != "exports"
        {
            continue;
        }
        let Expression::ObjectExpression(object) = &assignment.right else {
            continue;
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let PropertyKey::StaticIdentifier(key) = &property.key else {
                continue;
            };
            let name = key.name.as_str();
            if class_names.contains(&name)
                && classes
                    .insert(name, property.span().source_text(source))
                    .is_some()
            {
                return Err("Ambiguous configured crypto class export");
            }
        }
    }
    if declarations.len() != 2 || classes.len() != 2 {
        return Err("Configured crypto factory/class exports missing");
    }
    let factory = format!(
        "(function({},{},deprecate){{'use strict';\n{}\n{}\nreturn{{{},{},{},{}}};}})",
        class_names[0],
        class_names[1],
        declarations[factory_names[0]],
        declarations[factory_names[1]],
        classes[class_names[0]],
        classes[class_names[1]],
        factory_names[0],
        factory_names[1]
    );

    Ok(factory)
}

/// Original SDK random alias getters retain warning policy and first-read
/// descriptor transitions; only their syntax-owned declarations are projected.
pub(crate) fn random_alias_factory(source: &str) -> Result<String, &'static str> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err("Configured crypto source does not parse");
    }
    let mut declaration = None;
    let mut aliases = std::collections::BTreeMap::new();
    for statement in &parsed.program.body {
        if let Statement::FunctionDeclaration(function) = statement {
            if function
                .id
                .as_ref()
                .is_some_and(|id| id.name == "getRandomBytesAlias")
            {
                if declaration
                    .replace(function.span().source_text(source))
                    .is_some()
                {
                    return Err("Ambiguous random alias declaration");
                }
            }
        }
        let Statement::ExpressionStatement(statement) = statement else {
            continue;
        };
        let Expression::CallExpression(call) = &statement.expression else {
            continue;
        };
        if !matches!(&call.callee, Expression::Identifier(id) if id.name == "ObjectDefineProperties")
        {
            continue;
        }
        let Some(Expression::ObjectExpression(object)) = call
            .arguments
            .get(1)
            .and_then(|argument| argument.as_expression())
        else {
            continue;
        };
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                continue;
            };
            let PropertyKey::StaticIdentifier(key) = &property.key else {
                continue;
            };
            if ["prng", "pseudoRandomBytes", "rng"].contains(&key.name.as_str()) {
                if aliases
                    .insert(key.name.as_str(), property.span().source_text(source))
                    .is_some()
                {
                    return Err("Ambiguous random alias descriptor");
                }
            }
        }
    }
    let declaration = declaration.ok_or("Configured random alias declaration missing")?;
    if aliases.len() != 3 {
        return Err("Configured random alias descriptors missing");
    }
    Ok(format!(
        "(function(randomBytes,deprecate,getOptionValue,ObjectDefineProperty){{'use strict';\n{declaration}\nreturn{{{}}};}})",
        aliases.values().copied().collect::<Vec<_>>().join(",")
    ))
}
