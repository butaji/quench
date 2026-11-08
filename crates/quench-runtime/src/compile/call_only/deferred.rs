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
        || nested_unbounded || private_names || invoked_where_made(program)
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

/// Whether the source after the function calls it where it is made.
fn invoked_where_made(program: &Program<'_>) -> bool {
    let after = program.source_text.get(program.span.end as usize..).unwrap_or_default().trim_start();
    after.starts_with('(') || after.starts_with(".call") || after.starts_with(".apply")
}

impl Compiler<'_> {
    /// Adds the call-only facts of `function`, whose body was parsed lazily and its facts
    /// deferred, as its check would have found them: its text parsed alone as `context` gives
    /// it, unless a function enclosing it already gave them.
    pub(in crate::compile) fn find_deferred_call_only_facts(&mut self, function: &Function<'_>, context: &replay::PlainFunctionContext) {
        let span = function.span;
        let found = &mut self.first_call.call_only_found;
        if !self.first_call.nested_allowed || found.iter().any(|(start, end)| *start <= span.start && span.end <= *end) {
            return;
        }
        found.push((span.start, span.end));
        let snippet = &self.text[span.start as usize..span.end as usize];
        let wrapper = replay::plain_function_wrapper(context);
        let (text, offset) = lazy_bodies::text_alone(snippet, wrapper, context.inherited_strict, self.source_type);
        let allocator = lazy_bodies::arena(&text);
        let parsed = oxc_parser::Parser::new(&allocator, &text, self.source_type).parse();
        // The body checked whole at load; a parse that fails now claims nothing.
        if parsed.stack_exhausted || !parsed.diagnostics.is_empty() || crate::compile::stack::validate(&parsed.program).is_err() {
            return;
        }
        let built = oxc_semantic::SemanticBuilder::new().build(&parsed.program);
        let delta = span.start.wrapping_sub(offset);
        let facts = super::prototype_unobservable_functions(&built.semantic, &parsed.program);
        self.prototype_unobservable.extend(facts.into_iter().map(|start| start.wrapping_add(delta)));
    }
}
