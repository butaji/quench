//! Syntax-owned projection of the configured SDK crypto factory declarations.
use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey, Statement};
use oxc_span::GetSpan;
pub(crate) fn factory(source: &str) -> Result<String, &'static str> {
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
                .filter(|name| matches!(*name, "createHash" | "createHmac"))
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
            if matches!(name, "Hash" | "Hmac")
                && classes
                    .insert(name, property.span().source_text(source))
                    .is_some()
            {
                return Err("Ambiguous configured crypto class export");
            }
        }
    }
    if declarations.len() != 2 || classes.len() != 2 {
        return Err("Configured crypto hash factory/class exports missing");
    }
    let factory = format!(
        "(function(Hash,Hmac,deprecate){{'use strict';\n{}\n{}\nreturn{{{},{},createHash,createHmac}};}})",
        declarations["createHash"], declarations["createHmac"], classes["Hash"], classes["Hmac"]
    );

    Ok(factory)
}
