pub fn reduce_eval_source(
    source: &str,
    inherited_strict: bool,
    global: bool,
    direct: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
) -> Result<ResidualProgram, Vec<String>> {
    reduce_eval_source_in_context(
        source,
        inherited_strict,
        global,
        direct,
        bindings,
        forbidden_var_names,
        crate::semantic::EvalGrammarContext::default(),
    )
}
pub(crate) fn reduce_eval_source_in_context(
    source: &str,
    inherited_strict: bool,
    global: bool,
    _direct: bool,
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
    facts.install_reduction_source(source);
    facts.install_fact_sites(analysis.fact_sites);
    let binding_state = crate::reduce_support::eval_bindings(&parsed.program, bindings, strict, global);
    let (locals, next_slot, mut prefix, behavior, deletable) = binding_state;
    facts.eval_deletable = deletable;
    let mut ops = reduce_eval_body(&parsed.program.body, &mut facts, locals, next_slot, behavior, directive_completion)?;
    prefix.append(&mut ops);
    facts.finish_reduction();
    Ok(ResidualProgram::new(
        facts,
        prefix,
        None,
        std::collections::HashMap::new(),
    ))
}

fn reduce_eval_body(
    statements: &[oxc::ast::ast::Statement<'_>], facts: &mut ProgramDb, locals: HashMap<String, u16>,
    next_slot: u16, behavior: crate::reduce_support::EvalBehavior, directive_completion: Option<String>,
) -> Result<Vec<crate::ops::Op>, Vec<String>> {
    reduce_statements_opt(statements, facts, locals, next_slot, StatementsOptions { tail: true, eval_behavior: behavior, directive_completion })
}

fn eval_facts(analysis: &crate::semantic::Analysis, strict: bool) -> ProgramDb {
    ProgramDb {
        strict,
        scope_count: analysis.scope_count,
        symbol_count: analysis.symbol_count,
        private_names: analysis.private_names.iter().copied().collect(),
        ..ProgramDb::default()
    }
}
