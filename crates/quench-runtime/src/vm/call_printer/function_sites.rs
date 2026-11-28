//! Call-site descriptions read from one function's text, for an error raised before its
//! source's call sites are frozen.
use super::{frozen_call_sites, CallFinder, CallSites};
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
        && let Some(routes) = crate::compile::source_routes::routes()
        && let Some(callee) = (routes.unfrozen_call_sites)(source, span, site.0).and_then(|sites| lookup(&sites))
    {
        return Some(callee);
    }
    lookup(frozen_call_sites(source)?)
}

/// The call sites of the throwing function's text, else of the top level at `offset`, parsed
/// now (`source_routes`).
pub(crate) fn unfrozen_call_sites(source: &crate::bytecode::ProgramSource, span: Option<crate::bytecode::SourceSpan>, offset: u32) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    span.and_then(|span| function_call_sites(source, span)).or_else(|| top_level_call_sites(source, offset))
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
    // Only the callee is read, never the expression text, so the parse takes no tokens, and
    // the bodies of the functions inside it are released as they parse: the call is the
    // function's own (an inner function's call raises in its own frame). jsdom-d3-startup
    // threw and caught twice per iteration in a 2.4 MB module function, whose whole tree with
    // tokens took 39 MB of the run's peak each time.
    let allocator = crate::compile::lazy_bodies::sized_arena(&padded);
    let options = ParseOptions { allow_return_outside_function: true, lazy_function_bodies: Some(start as u32 + 1), ..ParseOptions::default() };
    let parsed = Parser::new(&allocator, &padded, goal).with_options(options).parse();
    if crate::compile::stack::validate_parsed(&parsed).is_err() || !parsed.diagnostics.is_empty() {
        return None;
    }
    let mut finder = CallFinder { source: &padded, comments: &parsed.program.comments, found: CallSites::new() };
    finder.visit_program(&parsed.program);
    Some(finder.found.into_values().collect())
}

/// The call sites of the top-level statement of `source` that holds `offset`: the source's
/// statements are parsed a run at a time, each run released once read
/// (`oxc_parser::StatementStream`), with every function body released as it parses
/// (`ParseOptions::lazy_function_bodies`) and without tokens, so an error a script's top level
/// raises and catches costs a run of its statements, not its top level's tree, its bodies
/// recognized without a tree (mandreel: a
/// caught top-level call took 29 MB at the run's peak to parse its 5 MB top level). A call
/// inside a released body is not listed.
fn top_level_call_sites(source: &crate::bytecode::ProgramSource, offset: u32) -> Option<Vec<crate::bytecode::CallSiteDescription>> {
    let text: &str = source.text.as_ref();
    if source.base != 0 {
        return None;
    }
    let allocator = oxc_allocator::Allocator::with_capacity(STREAM_ARENA);
    // Each body is recognized, not built: built then released, a 2.7 MB body took 33 MB.
    let mut accept = |_: oxc_parser::LazyBody<'_, '_>| true;
    // SAFETY: `accept` lives through the stream, which alone calls it.
    let check = unsafe { oxc_parser::LazyBodyCheck::new(&mut accept) };
    let options = ParseOptions {
        allow_return_outside_function: true,
        lazy_function_bodies: Some(0),
        lazy_recognize: true,
        lazy_body_check: Some(check),
        ..ParseOptions::default()
    };
    let mut stream = Parser::new(&allocator, text, SourceType::script().with_typescript(source.typescript)).with_options(options).statement_stream();
    let mut found = None;
    while found.is_none() {
        let step = stream.next(RUN_STATEMENTS, RUN_BYTES, &mut |_| {}, &mut |statements, comments| {
            let holding = statements.iter().filter(|statement| oxc_span::GetSpan::span(*statement).contains_inclusive(oxc_span::Span::empty(offset)));
            let mut finder = CallFinder { source: text, comments, found: CallSites::new() };
            for statement in holding {
                finder.visit_statement(statement);
            }
            if !finder.found.is_empty() {
                found = Some(finder.found.into_values().collect());
            }
        });
        if step != oxc_parser::StreamStep::More {
            break;
        }
    }
    found
}

/// The statements, and the arena bytes, of one run of `top_level_call_sites`, and its arena.
const RUN_STATEMENTS: usize = 4096;
const RUN_BYTES: usize = 1 << 20;
const STREAM_ARENA: usize = 2 << 20;
