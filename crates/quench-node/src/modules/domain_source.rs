//! Structural projection of the configured SDK's domain callback registration.
use oxc_ast::ast::Statement;
use oxc_span::GetSpan;
pub(crate) fn trampoline(source: &str) -> Result<String, &'static str> {
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err("Configured async hooks source does not parse");
    }
    let mut registration = None;
    let mut cell = None;
    for statement in &parsed.program.body {
        match statement {
            Statement::FunctionDeclaration(function)
                if function
                    .id
                    .as_ref()
                    .is_some_and(|id| id.name == "useDomainTrampoline") =>
            {
                if registration
                    .replace(function.span().source_text(source))
                    .is_some()
                {
                    return Err("Ambiguous configured domain trampoline");
                }
            }
            Statement::VariableDeclaration(declaration) => {
                for entry in &declaration.declarations {
                    if entry
                        .id
                        .get_identifier_name()
                        .is_some_and(|name| name == "domain_cb")
                    {
                        if declaration.declarations.len() != 1
                            || cell
                                .replace(declaration.span().source_text(source))
                                .is_some()
                        {
                            return Err("Ambiguous configured domain callback cell");
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let registration = registration.ok_or("Configured domain trampoline missing")?;
    let cell = cell.ok_or("Configured domain callback cell missing")?;
    // The accessor is a structural projection of the source-owned lexical cell,
    // not another implementation of callback selection or exception policy.
    Ok(format!(
        "(function(){{'use strict';\n{cell}\n{registration}\nreturn [useDomainTrampoline,()=>domain_cb];}})"
    ))
}
