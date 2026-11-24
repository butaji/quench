//! Call-only facts deferred to the compilation that reads them (TOD-1481). A source whose
//! functions may compile on first call checks each lazily parsed body at load for what the
//! load must report, its early errors, and leaves its call-only facts: a function the load
//! compiles anyway finds them here, from its own text, once for the functions inside it, and
//! a first-call chunk finds them in its own analysis. Most such functions never compile at
//! load (octane-code-load: 36 of 2,091).
use crate::compile::{Compiler, lazy_bodies, replay};
use oxc_ast::ast::{Expression, Function, Program, Statement};

/// Whether the load compiles `program`, a lazily parsed function alone, whatever stubs
/// allow: an async function or generator, a named function expression, one inside another
/// function whose free names are not known (`nested_unbounded`) or that names a private
/// member, or one called where it is made. Its check then finds its call-only facts; a
/// function this misjudges finds them when it compiles.
pub(in crate::compile) fn compiles_at_load(program: &Program<'_>, nested_unbounded: bool, private_names: bool) -> bool {
    let Some(function) = function_alone(program) else { return true };
    function.r#async || function.generator || function.is_expression() && function.id.is_some()
        || nested_unbounded || private_names || invoked_where_made(program.source_text, program.span.end)
}

/// The function `program` holds alone, as a declaration or an expression statement.
fn function_alone<'a>(program: &'a Program<'_>) -> Option<&'a Function<'a>> {
    match program.body.first()? {
        Statement::FunctionDeclaration(function) => Some(function),
        Statement::ExpressionStatement(statement) => match &statement.expression {
            Expression::FunctionExpression(function) => Some(function),
            _ => None,
        },
        _ => None,
    }
}

/// Whether the source after the function, which ends at `end`, calls it where it is made.
pub(in crate::compile) fn invoked_where_made(source_text: &str, end: u32) -> bool {
    let after = source_text.get(end as usize..).unwrap_or_default().trim_start();
    after.starts_with('(') || after.starts_with(".call") || after.starts_with(".apply")
}

impl Compiler<'_> {
    /// Adds the call-only facts of `function`, whose body was parsed lazily and its facts
    /// deferred, as its check would have found them: its text parsed alone as `context` gives
    /// it, unless a function enclosing it already gave them.
    pub(in crate::compile) fn find_deferred_call_only_facts(&mut self, function: &Function<'_>, context: &replay::PlainFunctionContext) {
        let span = function.span;
        let found = &mut self.first_call.call_only_found;
        if !self.first_call.nested_allowed || spans_contain(found, span.start, span.end) {
            return;
        }
        insert_span(found, span.start, span.end);
        let snippet = &self.text[span.start as usize..span.end as usize];
        let wrapper = replay::plain_function_wrapper(context);
        let (text, offset) = lazy_bodies::text_alone((self.leading_doc(span.start), snippet), wrapper, context.inherited_strict, self.source_type);
        let allocator = lazy_bodies::arena(&text);
        let parsed = oxc_parser::Parser::new(&allocator, &text, self.source_type).parse();
        // The body checked whole at load; a parse that fails now claims nothing.
        if parsed.stack_exhausted || !parsed.diagnostics.is_empty() || crate::compile::stack::validate(&parsed.program).is_err() {
            return;
        }
        let built = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        let delta = span.start.wrapping_sub(offset);
        let facts = super::prototype_unobservable_functions(&built.semantic, &parsed.program, &Default::default());
        self.prototype_unobservable.extend(facts.into_iter().map(|start| start.wrapping_add(delta)));
    }
}

/// `spans` sorted by start with none inside another (`spans_contain` and `insert_span` keep
/// them so): a lookup per compiled function scanned every span found before it, quadratic in
/// a bundle's functions (babel-wtb: 67M Ir of its 2.4G load).
pub(in crate::compile) fn normalize_spans(spans: &mut Vec<(u32, u32)>) {
    spans.sort_unstable_by_key(|&(start, end)| (start, std::cmp::Reverse(end)));
    let mut kept: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    for &(start, end) in spans.iter() {
        if kept.last().is_none_or(|&(_, last_end)| end > last_end) {
            kept.push((start, end));
        }
    }
    *spans = kept;
}

/// Whether a span of normalized `spans` holds `start..end`. Spans nest or are disjoint, as
/// functions' do; one that overlaps another only partly can make this answer no, which costs a
/// second analysis and nothing else.
pub(in crate::compile) fn spans_contain(spans: &[(u32, u32)], start: u32, end: u32) -> bool {
    let at = spans.partition_point(|&(span_start, _)| span_start <= start);
    at > 0 && spans[at - 1].1 >= end
}

/// Adds `start..end` to normalized `spans`, which no span holds, dropping the spans it holds.
fn insert_span(spans: &mut Vec<(u32, u32)>, start: u32, end: u32) {
    let at = spans.partition_point(|&(span_start, _)| span_start < start);
    let held = spans[at..].iter().take_while(|&&(span_start, span_end)| span_start <= end && span_end <= end).count();
    spans.splice(at..at + held, [(start, end)]);
}

#[cfg(test)]
mod span_tests {
    use super::{insert_span, normalize_spans, spans_contain};

    #[test]
    fn spans_answer_containment_as_a_scan_does() {
        let mut spans = vec![(10, 20), (0, 100), (200, 300), (210, 220), (150, 160)];
        normalize_spans(&mut spans);
        assert_eq!(spans, vec![(0, 100), (150, 160), (200, 300)]);
        assert!(spans_contain(&spans, 30, 40));
        assert!(spans_contain(&spans, 0, 100));
        assert!(!spans_contain(&spans, 100, 120));
        assert!(!spans_contain(&spans, 140, 155));
        insert_span(&mut spans, 120, 170);
        assert_eq!(spans, vec![(0, 100), (120, 170), (200, 300)]);
        assert!(spans_contain(&spans, 151, 159));
    }
}
