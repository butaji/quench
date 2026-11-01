//! Declaration closure projection of unchanged configured SDK policy.
//! OXC owns syntax. Selected declarations retain their exact source bytes and
//! positions; native host adapters supply only their genuine capabilities.
use oxc_ast::ast::{BindingIdentifier, IdentifierReference, Statement};
use oxc_ast_visit::Visit;
use oxc_span::GetSpan;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct Names {
    bindings: BTreeSet<String>,
    references: BTreeSet<String>,
}
impl<'a> Visit<'a> for Names {
    fn visit_binding_identifier(&mut self, identifier: &BindingIdentifier<'a>) {
        self.bindings.insert(identifier.name.to_string());
    }
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        self.references.insert(identifier.name.to_string());
    }
}

pub(crate) fn project(source: &str, exports: &[&str]) -> Result<String, String> {
    project_with_bindings(source, exports, &[])
}

/// Supplied names are real native capabilities or the original SDK captures.
/// Fail closed if a required declaration would redeclare a supplied binding.
pub(crate) fn project_with_bindings(
    source: &str,
    exports: &[&str],
    supplied: &[&str],
) -> Result<String, String> {
    if exports.iter().any(|name| supplied.contains(name)) {
        return Err("SDK policy export cannot be replaced by a supplied binding".to_owned());
    }
    let allocator = oxc_allocator::Allocator::default();
    let parsed = oxc_parser::Parser::new(&allocator, source, oxc_span::SourceType::cjs()).parse();
    if !parsed.diagnostics.is_empty() {
        return Err(format!(
            "SDK policy syntax errors: {:?}",
            parsed.diagnostics
        ));
    }
    let mut owners = BTreeMap::<String, usize>::new();
    let mut references = Vec::new();
    let mut bindings = Vec::new();
    for (index, statement) in parsed.program.body.iter().enumerate() {
        let mut names = Names::default();
        match statement {
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    names.visit_binding_pattern(&declarator.id);
                }
            }
            Statement::FunctionDeclaration(function) => {
                if let Some(id) = &function.id {
                    names.visit_binding_identifier(id);
                }
            }
            Statement::ClassDeclaration(class) => {
                if let Some(id) = &class.id {
                    names.visit_binding_identifier(id);
                }
            }
            _ => {}
        }
        for name in &names.bindings {
            if owners.insert(name.clone(), index).is_some() {
                return Err(format!("SDK policy duplicate top-level binding: {name}"));
            }
        }
        bindings.push(names.bindings.clone());
        names.references.clear();
        names.visit_statement(statement);
        references.push(names.references);
    }
    let mut selected = BTreeSet::new();
    let mut pending = Vec::new();
    for name in exports {
        pending.push(
            *owners
                .get(*name)
                .ok_or_else(|| format!("SDK policy binding missing: {name}"))?,
        );
    }
    while let Some(index) = pending.pop() {
        if !selected.insert(index) {
            continue;
        }
        if bindings[index]
            .iter()
            .any(|name| supplied.contains(&name.as_str()))
        {
            return Err("SDK policy declaration mixes supplied and required bindings".to_owned());
        }
        for name in &references[index] {
            if supplied.contains(&name.as_str()) {
                continue;
            }
            if let Some(owner) = owners.get(name) {
                pending.push(*owner);
            }
        }
    }
    let mut projected = source
        .as_bytes()
        .iter()
        .map(|byte| {
            if matches!(byte, b'\n' | b'\r') {
                *byte
            } else {
                b' '
            }
        })
        .collect::<Vec<_>>();
    for directive in &parsed.program.directives {
        let span = directive.span();
        projected[span.start as usize..span.end as usize]
            .copy_from_slice(&source.as_bytes()[span.start as usize..span.end as usize]);
    }
    for index in selected {
        let span = parsed.program.body[index].span();
        projected[span.start as usize..span.end as usize]
            .copy_from_slice(&source.as_bytes()[span.start as usize..span.end as usize]);
    }
    let mut projected = String::from_utf8(projected).map_err(|error| error.to_string())?;
    projected.push_str("\nmodule.exports = { ");
    projected.push_str(&exports.join(", "));
    projected.push_str(" };\n");
    Ok(projected)
}
