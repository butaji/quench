use oxc::semantic::SemanticBuilder;

pub(crate) fn analyze(program: &oxc::ast::ast::Program<'_>) -> Result<(usize, usize), Vec<String>> {
    let semantic = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(program);
    if !semantic.errors.is_empty() {
        return Err(semantic
            .errors
            .iter()
            .map(|error| format!("SyntaxError: {error}"))
            .collect());
    }
    Ok((
        semantic.semantic.scopes().len(),
        semantic.semantic.symbols().len(),
    ))
}
