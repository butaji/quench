//! Call-site descriptions read from one function's text, for an error raised before its
//! source's call sites are frozen.
use super::{frozen_call_sites, parse_call_sites, CallFinder, CallSites};
use oxc_ast_visit::Visit;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::SourceType;

/// The callee text of the call at `site` of a function whose text is `span`. Before the
/// source's call sites are frozen, a dynamic build reads them from the function's text alone
/// (`function_call_sites`), so an error a load raises and catches costs its function, not
/// its whole source.
pub(super) fn call_site_callee(source: &crate::bytecode::ProgramSource, span: Option<crate::bytecode::SourceSpan>, site: (u32, bool)) -> Option<String> {
    let lookup = |sites: &[crate::bytecode::CallSiteDescription]| {
        sites.binary_search_by_key(&site, |found| (found.offset, found.construct)).ok().map(|index| sites[index].callee.clone())
    };
    if crate::DYNAMIC_EXECUTION_ENABLED && source.call_sites.get().is_none()
        && let Some(callee) = span.and_then(|span| function_call_sites(source, span)).or_else(|| top_level_call_sites(source)).and_then(|sites| lookup(&sites))
    {
        return Some(callee);
    }
    lookup(frozen_call_sites(source)?)
}

/// The call sites of the function whose text is `span` of `source`, parsed alone at its
/// place: every other byte is blank (lines kept), and the function sits in parentheses, so
/// its offsets are the source's. `None` when it does not parse as an expression.
fn function_call_sites(source: &crate::bytecode::ProgramSource, span: crate::bytecode::SourceSpan) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    let text: &str = source.text.as_ref();
    let (start, end) = (span.start as usize, span.end as usize);
    if source.base != 0 || start == 0 || (start == 1 && end == text.len()) {
        return None;
    }
    let function = text.get(start..end)?;
    let blank = |part: &str| part.bytes().map(|byte| if byte == b'\n' { '\n' } else { ' ' }).collect::<String>();
    let padded = format!("{}({function})", blank(text.get(..start - 1)?));
    let goal = SourceType::script().with_typescript(source.typescript);
    match parse_call_sites(&padded, goal, CallSites::new()) {
        Ok((found, true)) => Some(found.into_values().collect()),
        _ => None,
    }
}

/// The call sites of `source`'s top-level code: it parsed with every function body it can
/// release released (`ParseOptions::lazy_function_bodies`) and without the tokens only an
/// expression's text reads, so an error a script's top level raises and catches costs its
/// top level, not its whole tree (mandreel: a caught top-level call parsed its 5 MB whole
/// with tokens, 20 MB at a warm run's peak). A call inside a released body is not listed.
fn top_level_call_sites(source: &crate::bytecode::ProgramSource) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    let text: &str = source.text.as_ref();
    if source.base != 0 {
        return None;
    }
    let allocator = oxc_allocator::Allocator::default();
    let options = ParseOptions { allow_return_outside_function: true, lazy_function_bodies: Some(0), ..ParseOptions::default() };
    let parsed = Parser::new(&allocator, text, SourceType::script().with_typescript(source.typescript)).with_options(options).parse();
    if crate::compile::stack::validate_parsed(&parsed).is_err() || !parsed.diagnostics.is_empty() {
        return None;
    }
    let mut finder = CallFinder { source: text, comments: &parsed.program.comments, found: CallSites::new() };
    finder.visit_program(&parsed.program);
    Some(finder.found.into_values().collect())
}
