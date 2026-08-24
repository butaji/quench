use std::collections::{HashMap, HashSet};

use oxc::{
    ast::AstKind,
    diagnostics::OxcDiagnostic,
    semantic::{Semantic, SemanticBuilder},
    span::{GetSpan, Span},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EvalGrammarContext {
    pub(crate) new_target: bool,
    pub(crate) super_property: bool,
    pub(crate) arguments: bool,
}

pub(crate) struct Analysis {
    pub(crate) scope_count: usize,
    pub(crate) symbol_count: usize,
    pub(crate) private_names: Vec<(Span, crate::facts::PrivateNameId)>,
    pub(crate) fact_sites: HashMap<Span, crate::facts::FactSiteId>,
}

pub(crate) fn analyze(program: &oxc::ast::ast::Program<'_>) -> Result<Analysis, Vec<String>> {
    analyze_with_context(program, EvalGrammarContext::default())
}

pub(crate) fn private_name_ids(
    semantic: &Semantic<'_>,
) -> Vec<(Span, crate::facts::PrivateNameId)> {
    let classes = semantic.classes();
    let mut definitions = HashMap::new();
    let mut next_id = 0;
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                definitions
                    .entry((class_id, element.name.clone()))
                    .or_insert_with(|| {
                        let id = crate::facts::PrivateNameId(next_id);
                        next_id += 1;
                        id
                    });
            }
        }
    }
    let mut names = Vec::new();
    for (class_id, _) in classes.iter_enumerated() {
        for element in &classes.elements[class_id] {
            if element.is_private {
                names.push((element.span, definitions[&(class_id, element.name.clone())]));
            }
        }
        for reference in classes.iter_private_identifiers(class_id) {
            if let Some(id) = classes.ancestors(class_id).find_map(|ancestor| {
                definitions
                    .get(&(ancestor, reference.name.clone()))
                    .copied()
            }) {
                names.push((reference.span, id));
            }
        }
    }
    names
}

pub(crate) fn analyze_eval(
    program: &oxc::ast::ast::Program<'_>,
    context: EvalGrammarContext,
) -> Result<Analysis, Vec<String>> {
    analyze_with_context(program, context)
}

fn analyze_with_context(
    program: &oxc::ast::ast::Program<'_>,
    context: EvalGrammarContext,
) -> Result<Analysis, Vec<String>> {
    crate::semantic_early::validate(program)?;
    for statement in &program.body {
        if matches!(
            statement,
            oxc::ast::ast::Statement::BreakStatement(_)
                | oxc::ast::ast::Statement::ContinueStatement(_)
        ) {
            return Err(vec!["SyntaxError: break or continue outside of loop".into()]);
        }
    }
    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    let using_for_of_names = using_for_of_bound_names(program);
    let mut errors = semantic
        .errors
        .iter()
        .filter(|error| !context.permits(error, &semantic.semantic))
        .filter(|error| {
            !using_for_of_names.iter().any(|name| {
                error
                    .message
                    .contains(&format!("Identifier `{name}` has already been declared"))
            })
        })
        .map(|error| format!("SyntaxError: {error}"))
        .collect::<Vec<_>>();
    if context.arguments && contains_arguments(&semantic.semantic) {
        errors.push("SyntaxError: 'arguments' is not allowed in this eval".to_string());
    }
    errors.extend(class_name_errors(program, &semantic.semantic));
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Analysis {
        scope_count: semantic.semantic.scopes().len(),
        symbol_count: semantic.semantic.symbols().len(),
        private_names: private_name_ids(&semantic.semantic),
        fact_sites: semantic
            .semantic
            .nodes()
            .iter()
            .enumerate()
            .map(|(index, node)| (node.kind().span(), crate::facts::FactSiteId(index as u32)))
            .collect(),
    })
}

fn using_for_of_bound_names(program: &oxc::ast::ast::Program<'_>) -> HashSet<String> {
    struct Finder {
        names: HashSet<String>,
    }
    impl<'a> oxc::ast::visit::Visit<'a> for Finder {
        fn visit_for_of_statement(&mut self, statement: &oxc::ast::ast::ForOfStatement<'a>) {
            if let oxc::ast::ast::ForStatementLeft::VariableDeclaration(declaration) =
                &statement.left
            {
                if matches!(
                    declaration.kind,
                    oxc::ast::ast::VariableDeclarationKind::Using
                        | oxc::ast::ast::VariableDeclarationKind::AwaitUsing
                ) {
                    for declarator in &declaration.declarations {
                        self.names
                            .extend(crate::binding_patterns::names(&declarator.id));
                    }
                }
            }
            oxc::ast::visit::walk::walk_for_of_statement(self, statement);
        }
    }
    let mut finder = Finder {
        names: HashSet::new(),
    };
    oxc::ast::visit::walk::walk_program(&mut finder, program);
    finder.names
}

impl EvalGrammarContext {
    fn permits(self, error: &OxcDiagnostic, semantic: &Semantic<'_>) -> bool {
        const NEW_TARGET: &str = "Unexpected new.target expression";
        const SUPER_PROPERTY: &str =
            "'super' can only be referenced in members of derived classes or object literal expressions.";
        let message = error.message.as_ref();
        if self.new_target && message == NEW_TARGET {
            return true;
        }
        self.super_property && message == SUPER_PROPERTY && !has_function_boundary(error, semantic)
    }
}

fn contains_arguments(semantic: &Semantic<'_>) -> bool {
    semantic.nodes().iter().any(|node| match node.kind() {
        AstKind::IdentifierReference(id) => id.name == "arguments",
        AstKind::BindingIdentifier(id) => id.name == "arguments",
        _ => false,
    })
}

fn has_function_boundary(error: &OxcDiagnostic, semantic: &Semantic<'_>) -> bool {
    let Some(offset) = error
        .labels
        .as_ref()
        .and_then(|labels| labels.first())
        .map(|label| label.offset())
    else {
        return true;
    };
    let nodes = semantic.nodes();
    let Some(node) = nodes.iter().find(|node| {
        matches!(node.kind(), AstKind::Super(_)) && node.kind().span().start as usize == offset
    }) else {
        return true;
    };
    nodes
        .ancestor_kinds(node.id())
        .any(|kind| matches!(kind, AstKind::Function(_)))
}

/// Class declarations and expressions are strict mode code, so their binding
/// identifier cannot be a strict-reserved name, `eval`, or `arguments`; in
/// modules `await` is reserved as well. OXC's checker decides strictness from
/// the enclosing scope and misses the class binding itself, so check it here.
fn class_name_errors(program: &oxc::ast::ast::Program<'_>, semantic: &Semantic<'_>) -> Vec<String> {
    let module = program.source_type.is_module();
    let mut errors = Vec::new();
    for node in semantic.nodes().iter() {
        let AstKind::Class(class) = node.kind() else {
            continue;
        };
        let Some(identifier) = &class.id else {
            continue;
        };
        if reserved_class_name(identifier.name.as_str(), module) {
            errors.push(format!(
                "SyntaxError: `{}` is not a valid class binding identifier",
                identifier.name
            ));
        }
    }
    errors
}

fn reserved_class_name(name: &str, module: bool) -> bool {
    matches!(
        name,
        "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "yield"
            | "eval"
            | "arguments"
    ) || (module && name == "await")
}

#[cfg(test)]
mod tests {
    use oxc::{allocator::Allocator, parser::Parser, span::SourceType};

    fn ids(source: &str, module: bool) -> Vec<(u32, u32, u32)> {
        let allocator = Allocator::default();
        let source_type = if module {
            SourceType::mjs()
        } else {
            SourceType::cjs()
        };
        let parsed = Parser::new(&allocator, source, source_type).parse();
        let analysis = super::analyze(&parsed.program).expect("analyze");
        analysis
            .private_names
            .iter()
            .map(|(span, id)| (span.start, span.end, id.0))
            .collect()
    }

    #[test]
    fn module_and_script_share_private_name_ids() {
        let source = "class outer { #x = 42; f() { return this.#x; } }";
        assert_eq!(ids(source, true), ids(source, false));
    }
}
