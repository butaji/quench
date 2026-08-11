pub fn reduce_eval_source(
    source: &str,
    inherited_strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
) -> Result<ResidualProgram, Vec<String>> {
    reduce_eval_source_in_context(
        source,
        inherited_strict,
        global,
        bindings,
        forbidden_var_names,
        crate::semantic::EvalGrammarContext::default(),
    )
}
pub(crate) fn reduce_eval_source_in_context(
    source: &str,
    inherited_strict: bool,
    global: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
    grammar: crate::semantic::EvalGrammarContext,
) -> Result<ResidualProgram, Vec<String>> {
    let strict_source = inherited_strict.then(|| format!("\"use strict\";\n{source}"));
    let source = strict_source.as_deref().unwrap_or(source);
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    crate::reduce_support::validate_parse(&parsed)?;
    crate::reduce_support::validate_program(&parsed.program)?;
    let analysis = crate::semantic::analyze_eval(&parsed.program, grammar)?;
    let strict = inherited_strict || crate::reduce_support::has_strict_directive(&parsed.program);
    crate::reduce_support::validate_eval_var_names(&parsed.program, strict, forbidden_var_names)?;
    let directive_completion =
        super::reduce_eval::directive_completion(&parsed.program, inherited_strict);
    let mut facts = eval_facts(&analysis, strict);
    let (locals, next_slot, mut prefix, behavior, deletable) =
        crate::reduce_support::eval_bindings(&parsed.program, bindings, strict, global);
    facts.eval_deletable = deletable;
    let mut ops = reduce_statements_opt(
        &parsed.program.body,
        &mut facts,
        locals,
        next_slot,
        true,
        behavior,
        directive_completion,
    )?;
    prefix.append(&mut ops);
    Ok(ResidualProgram { facts, ops: prefix })
}
fn eval_facts(analysis: &crate::semantic::Analysis, strict: bool) -> ProgramDb {
    ProgramDb {
        strict,
        scope_count: analysis.scope_count,
        symbol_count: analysis.symbol_count,
        private_names: analysis.private_names.clone(),
        ..ProgramDb::default()
    }
}
