//! Imports that bind only types. An import specifier no value position reads names a
//! type (tsc's import elision): it joins the declared environment and makes no link
//! edge, so `import { Shape } from "./shapes"` links whether or not the module exports a
//! value `Shape`. An import whose specifiers all bind types makes no edge at all; one
//! that never named anything (`import "./setup"`) keeps its evaluation edge.

use oxc_ast::ast::*;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

/// The import specifiers of `program` that no value position reads, and the imports
/// all of whose specifiers are such: by source range.
pub(in crate::compile) fn type_only_imports(program: &Program<'_>, scoping: &Scoping) -> Vec<Span> {
    let mut spans = Vec::new();
    // A direct eval reads any binding by name: every import is a value.
    if !program.source_type.is_typescript() || program.source_text.contains("eval") { return spans; }
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else { continue };
        let Some(specifiers) = import.specifiers.as_ref().filter(|specifiers| !specifiers.is_empty()) else { continue };
        let unread: Vec<Span> = specifiers.iter()
            .filter(|specifier| !value_read(scoping, specifier.local().symbol_id.get()))
            .map(GetSpan::span)
            .collect();
        if unread.len() == specifiers.len() { spans.push(import.span); } else { spans.extend(unread); }
    }
    spans
}

fn value_read(scoping: &Scoping, symbol: Option<oxc_syntax::symbol::SymbolId>) -> bool {
    let Some(symbol) = symbol else { return true };
    scoping.get_resolved_references(symbol).any(|reference| {
        let flags = reference.flags();
        (flags.is_read() || flags.is_write()) && !flags.is_value_as_type()
    })
}

/// Move the type-only imports (`spans`, from `type_only_imports`) into the declared
/// environment.
pub(in crate::compile) fn elide(program: &mut Program<'_>, spans: &[Span], declarations: &mut super::Declarations) {
    if spans.is_empty() { return; }
    program.body.retain_mut(|statement| {
        let Statement::ImportDeclaration(import) = statement else { return true };
        if spans.contains(&import.span) {
            declarations.declare(super::DeclaredKind::ModuleType, None, import.span);
            return false;
        }
        if let Some(specifiers) = &mut import.specifiers {
            specifiers.retain(|specifier| {
                let span = specifier.span();
                if !spans.contains(&span) { return true; }
                declarations.declare(super::DeclaredKind::ModuleType, Some(specifier.local().name.as_str()), span);
                false
            });
        }
        true
    });
}

