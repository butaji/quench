include!("reduce_eval_private.rs");

pub fn reduce_eval_source(
    source: &str,
    inherited_strict: bool,
    global: bool,
    _direct: bool,
    bindings: &[(String, u16)],
    forbidden_var_names: &[String],
) -> Result<ResidualProgram, Vec<String>> {
    reduce_eval_source_in_context(
        source,
        inherited_strict,
        global,
        false,
        bindings,
        &[],
        forbidden_var_names,
        crate::semantic::EvalGrammarContext::default(),
    )
}
pub(crate) fn reduce_eval_source_in_context(
    source: &str,
    inherited_strict: bool,
    global: bool,
    dynamic_scope: bool,
    bindings: &[(String, u16)],
    reusable_var_names: &[String],
    forbidden_var_names: &[String],
    grammar: crate::semantic::EvalGrammarContext,
) -> Result<ResidualProgram, Vec<String>> {
    if has_top_level_eval_control(source) {
        return Err(vec!["SyntaxError: break or continue outside of loop".into()]);
    }
    let strict_source = inherited_strict.then(|| format!("\"use strict\";\n{source}"));
    let source = strict_source.as_deref().unwrap_or(source);
    let source = crate::reduce_support::prepare_source(source);
    let wrapped = wrap_eval_for_private(source.as_ref());
    let source = wrapped.as_deref().unwrap_or(source.as_ref());
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
    if dynamic_scope {
        facts.enter_dynamic_scope();
    }
    install_eval_facts(&mut facts, source, &analysis);
    if wrapped.is_some() {
        remap_eval_private_ids(&mut facts, &parsed.program);
    }
    let statements = if wrapped.is_some() {
        wrapped_method_body(&parsed.program).unwrap_or(&parsed.program.body)
    } else {
        &parsed.program.body
    };
    let (locals, next_slot, prefix, behavior, deletable) = if wrapped.is_some() {
        crate::reduce_support::eval_bindings_without_program(
            bindings,
            reusable_var_names,
            strict,
            global,
        )
    } else {
        crate::reduce_support::eval_bindings(
            &parsed.program,
            bindings,
            reusable_var_names,
            strict,
            global,
        )
    };
    facts.eval_deletable = deletable;
    reduce_eval_program(
        statements,
        facts,
        prefix,
        locals,
        next_slot,
        behavior,
        directive_completion,
    )
}

fn has_top_level_eval_control(source: &str) -> bool {
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, SourceType::cjs()).parse();
    parsed.errors.is_empty()
        && parsed.program.body.iter().any(|statement| {
            matches!(
                statement,
                oxc::ast::ast::Statement::BreakStatement(_)
                    | oxc::ast::ast::Statement::ContinueStatement(_)
            )
        })
}

fn reduce_eval_program(
    statements: &[oxc::ast::ast::Statement<'_>],
    mut facts: ProgramDb,
    prefix: Vec<crate::ops::Op>,
    locals: HashMap<String, u16>,
    next_slot: u16,
    behavior: crate::reduce_support::EvalBehavior,
    directive_completion: Option<String>,
) -> Result<ResidualProgram, Vec<String>> {
    facts
        .eval_var_barrier
        .extend(crate::semantic_early::lexically_declared_names_in(
            statements,
        ));
    let local_slots = locals.clone();
    let reduced = reduce_eval_body(
        statements,
        &mut facts,
        locals,
        next_slot,
        behavior,
        directive_completion,
    )?;
    Ok(finish_eval_reduction(facts, prefix, reduced, local_slots))
}

fn finish_eval_reduction(
    mut facts: ProgramDb,
    mut prefix: Vec<crate::ops::Op>,
    mut reduced: ReducedStatementOps,
    local_slots: HashMap<String, u16>,
) -> ResidualProgram {
    prefix.append(&mut reduced.ops);
    facts.finish_reduction();
    ResidualProgram::with_frame_register_count(
        facts,
        prefix,
        reduced.frame_register_count,
        None,
        local_slots,
    )
}

fn reduce_eval_body(
    statements: &[oxc::ast::ast::Statement<'_>],
    facts: &mut ProgramDb,
    locals: HashMap<String, u16>,
    next_slot: u16,
    behavior: crate::reduce_support::EvalBehavior,
    directive_completion: Option<String>,
) -> Result<ReducedStatementOps, Vec<String>> {
    reduce_statements_opt(
        statements,
        facts,
        locals,
        next_slot,
        StatementsOptions {
            tail: true,
            eval_behavior: behavior,
            directive_completion,
        },
    )
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

fn install_eval_facts(facts: &mut ProgramDb, source: &str, analysis: &crate::semantic::Analysis) {
    facts.install_reduction_source(source);
    facts.install_fact_sites(analysis.fact_sites.clone());
}

#[cfg(test)]
mod tests {
    #[test]
    fn eval_accepts_nul_inside_a_line_comment() {
        let program = super::reduce_eval_source(
            "//var \0yy = -1",
            false,
            true,
            true,
            &[("globalThis".to_string(), 0)],
            &[],
        );
        assert!(program.is_ok(), "{program:?}");
    }

    #[test]
    fn eval_freezes_reducer_owned_frame_width() {
        let body = (0..96)
            .map(|index| format!("var value{index} = {index};"))
            .collect::<String>();
        let source = format!("function wide(){{{body}}} external + 1");
        let program = super::reduce_eval_source(
            &source,
            false,
            true,
            true,
            &[("external".to_string(), 80)],
            &[],
        )
        .expect("eval reduction");
        let width = program.code().frame_register_count();
        assert!((81..128).contains(&width), "unexpected eval width {width}");
    }
}
