use std::collections::HashMap;

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
}

pub(crate) struct Analysis {
    pub(crate) scope_count: usize,
    pub(crate) symbol_count: usize,
    pub(crate) private_names: Vec<(Span, crate::facts::PrivateNameId)>,
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
    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    let errors = semantic
        .errors
        .iter()
        .filter(|error| !context.permits(error, &semantic.semantic))
        .map(|error| format!("SyntaxError: {error}"))
        .collect::<Vec<_>>();
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Analysis {
        scope_count: semantic.semantic.scopes().len(),
        symbol_count: semantic.semantic.symbols().len(),
        private_names: private_name_ids(&semantic.semantic),
    })
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
