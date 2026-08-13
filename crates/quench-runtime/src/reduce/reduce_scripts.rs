use oxc::{allocator::Allocator, parser::Parser, span::SourceType};

use crate::facts::ProgramDb;

use super::reduce_statements::{ResidualProgram, StatementReducer};

#[derive(Clone, Copy)]
pub struct ScriptSource<'a> {
    pub source: &'a str,
    pub strict: bool,
}

pub fn reduce_script_sources(sources: &[ScriptSource<'_>]) -> Result<ResidualProgram, Vec<String>> {
    let units = sources
        .iter()
        .map(|source| SourceUnit {
            source: source.source,
            source_type: SourceType::cjs(),
            strict: source.strict,
            program_scope: true,
        })
        .collect::<Vec<_>>();
    reduce_units(&units)
}

pub fn reduce_module_sequence(
    prefix_scripts: &[&str],
    source: &str,
) -> Result<ResidualProgram, Vec<String>> {
    let mut units = prefix_scripts
        .iter()
        .map(|source| SourceUnit {
            source,
            source_type: SourceType::cjs(),
            strict: false,
            program_scope: true,
        })
        .collect::<Vec<_>>();
    units.push(SourceUnit {
        source,
        source_type: SourceType::mjs(),
        strict: true,
        program_scope: false,
    });
    reduce_units(&units)
}

#[derive(Clone, Copy)]
struct SourceUnit<'a> {
    source: &'a str,
    source_type: SourceType,
    strict: bool,
    program_scope: bool,
}

fn reduce_units(units: &[SourceUnit<'_>]) -> Result<ResidualProgram, Vec<String>> {
    let allocator = Allocator::default();
    let mut state = StatementReducer::new_with_global(SourceType::cjs(), false);
    let mut totals = (0, 0);
    let mut last = None;
    let mut facts_out = ProgramDb::default();
    let mut module_metadata = None;
    for unit in units {
        let strict_source;
        let source = if unit.strict && unit.source_type.is_script() {
            strict_source = format!("\"use strict\";\n{}", unit.source);
            strict_source.as_str()
        } else {
            unit.source
        };
        let parsed = Parser::new(&allocator, source, unit.source_type).parse();
        reject_parse_errors(&parsed)?;
        crate::reduce_support::validate_program(&parsed.program)?;
        let analysis = crate::semantic::analyze(&parsed.program)?;
        if unit.source_type.is_module() {
            module_metadata = Some(super::ModuleMetadata::from_statements(&parsed.program.body));
        }
        totals.0 += analysis.scope_count;
        totals.1 += analysis.symbol_count;
        let mut facts = ProgramDb {
            strict: unit.strict || has_strict_directive(&parsed.program),
            private_names: analysis.private_names.into_iter().collect(),
            ..ProgramDb::default()
        };
        facts.install_reduction_source(source);
        facts.install_fact_sites(analysis.fact_sites);
        last = state.append(&parsed.program.body, &mut facts, unit.program_scope)?;
        facts.finish_reduction();
        merge_facts(&mut facts_out, facts);
    }
    finish(state, totals, last, facts_out, module_metadata)
}

fn merge_facts(target: &mut ProgramDb, source: ProgramDb) {
    target.site_facts.merge(source.site_facts);
    target.eval_var_barrier.extend(source.eval_var_barrier);
    target.eval_deletable.extend(source.eval_deletable);
    target.strict |= source.strict;
    target.in_function = source.in_function;
    target.tail_calls = source.tail_calls;
    target.private_names.extend(source.private_names);
}

fn reject_parse_errors(parsed: &oxc::parser::ParserReturn<'_>) -> Result<(), Vec<String>> {
    if parsed.panicked || !parsed.errors.is_empty() {
        return Err(vec!["SyntaxError: OXC parser rejected source".to_string()]);
    }
    Ok(())
}

fn has_strict_directive(program: &oxc::ast::ast::Program<'_>) -> bool {
    program
        .directives
        .iter()
        .any(|directive| directive.directive.as_str() == "use strict")
}

fn finish(
    state: StatementReducer,
    totals: (usize, usize),
    last: Option<u16>,
    mut facts: ProgramDb,
    module_metadata: Option<super::ModuleMetadata>,
) -> Result<ResidualProgram, Vec<String>> {
    facts.scope_count = totals.0;
    facts.symbol_count = totals.1;
    let local_slots = state.local_slots();
    Ok(ResidualProgram::new(
        facts,
        crate::reduce_support::finish_program(state.ops, last)?,
        module_metadata,
        local_slots,
    ))
}
