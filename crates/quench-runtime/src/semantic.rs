use oxc::{
    ast::AstKind,
    diagnostics::OxcDiagnostic,
    semantic::{Semantic, SemanticBuilder},
    span::GetSpan,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EvalGrammarContext {
    pub(crate) new_target: bool,
    pub(crate) super_property: bool,
}

pub(crate) fn analyze(program: &oxc::ast::ast::Program<'_>) -> Result<(usize, usize), Vec<String>> {
    analyze_with_context(program, EvalGrammarContext::default())
}

pub(crate) fn analyze_eval(
    program: &oxc::ast::ast::Program<'_>,
    context: EvalGrammarContext,
) -> Result<(usize, usize), Vec<String>> {
    analyze_with_context(program, context)
}

fn analyze_with_context(
    program: &oxc::ast::ast::Program<'_>,
    context: EvalGrammarContext,
) -> Result<(usize, usize), Vec<String>> {
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
    Ok((
        semantic.semantic.scopes().len(),
        semantic.semantic.symbols().len(),
    ))
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
