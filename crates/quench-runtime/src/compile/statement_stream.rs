//! A huge function body of straight-line statements compiled a run of statements at a time
//! from its text (`oxc_parser::StatementStream`), so its compilation holds one run's tree, not
//! the body's: mandreel's `global_init`, 2.7 MB of 150K `emit_32(...)` calls, parsed again
//! whole into a ~45 MB tree where it compiled at load.
//!
//! Only a body whose function-wide analyses find nothing in it streams: expression statements
//! that declare nothing, make no function, write no binding and name no `arguments`, `eval`,
//! `this` or `new.target`. The analyses then run over an empty body, and each run compiles as
//! the statements would in the whole body.
use oxc_ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentTarget, Class, Expression, Function, IdentifierReference,
    ImportExpression, ImportMeta, NewTarget, PrivateFieldExpression, PrivateInExpression, Program, RegExpLiteral, SimpleAssignmentTarget,
    Statement, Super, TaggedTemplateExpression, ThisExpression, UnaryExpression, UnaryOperator, UpdateExpression,
};
use oxc_ast_visit::{Visit, walk};

use super::FunctionCompiler;

/// The least body, in bytes of source, that streams: below it the tree is small.
const STREAM_MIN_BODY: u32 = 256 << 10;
/// The statements, and the arena bytes, of one run.
pub(crate) const RUN_STATEMENTS: usize = 4096;
pub(crate) const RUN_BYTES: usize = 1 << 20;
/// The stream's arena: a run releases what it took, so one chunk holds every run.
pub(crate) const STREAM_ARENA: usize = 2 << 20;

/// A function body that compiles from its text as a stream, by its source range between the
/// braces (offsets into the program's text).
#[derive(Clone, Copy)]
pub(super) struct StreamedBody {
    pub(super) start: u32,
    pub(super) end: u32,
    pub(super) strict: bool,
    /// Only the first run compiles (`StreamRun`): the body runs a run at a time.
    pub(super) one_run: bool,
}

/// A first-call chunk of a function whose body streams, compiled one run of its statements
/// at a time (`Vm::call_streamed_runs`): each run compiles as a chunk of its own that runs
/// once and is then dropped, so a body that runs once (mandreel's `global_init`, 614K
/// instructions) never has its code resident whole.
pub(crate) struct StreamRun {
    /// The start of the function in the program's text.
    pub(crate) start: u32,
    /// Where the run starts, for a run after the first, whose chunk holds the function's
    /// header alone; the first run's chunk compiles the function from its text.
    pub(crate) from: Option<(u32, StreamedRun)>,
    /// What the chunk's compilation found: the body, when it streams, and where its next run
    /// starts, if any.
    pub(crate) found: std::cell::Cell<Option<(StreamedRun, Option<u32>)>>,
}

/// The body of a function whose runs compile one at a time.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StreamedRun {
    /// The offset of the body's opening brace, and of its closing one.
    pub(crate) open: u32,
    pub(crate) close: u32,
    pub(crate) strict: bool,
}

impl StreamRun {
    pub(crate) fn first(start: u32) -> Self {
        Self { start, from: None, found: std::cell::Cell::new(None) }
    }

    pub(crate) fn after(start: u32, from: u32, body: StreamedRun) -> Self {
        Self { start, from: Some((from, body)), found: std::cell::Cell::new(None) }
    }
}

/// The function `program` holds alone (as `BodyFacts::check` sees it) when its body may
/// stream by its size and kind: plain, past the streaming size, without directives.
pub(super) fn streaming_function<'p, 'a>(program: &'p Program<'a>) -> Option<&'p Function<'a>> {
    let function = crate::compile::first_call::nested::alone_function(program)?;
    let body = function.body.as_ref()?;
    (body.span.size() >= STREAM_MIN_BODY && body.directives.is_empty() && !function.generator && !function.r#async).then_some(function)
}

/// The start of the function `program` holds alone when its body streams.
pub(super) fn streamable_start(program: &Program<'_>) -> Option<u32> {
    let function = streaming_function(program)?;
    let body = function.body.as_ref()?;
    straight_line(&body.statements).then_some(function.span.start)
}

/// Whether `statements` leave every function-wide analysis of their body unchanged.
fn straight_line(statements: &[Statement<'_>]) -> bool {
    let mut check = StraightLine { fine: true };
    statements.iter().all(|statement| {
        let Statement::ExpressionStatement(statement) = statement else { return false };
        check.visit_expression(&statement.expression);
        check.fine
    })
}

/// Whether the body of the source range `body` streams, read from its text a run at a time.
pub(super) fn body_streams(text: &str, source_type: oxc_span::SourceType, body: StreamedBody) -> bool {
    let allocator = oxc_allocator::Allocator::with_capacity(STREAM_ARENA);
    let options = oxc_parser::ParseOptions { inherited_strict: body.strict, allow_return_outside_function: true, ..Default::default() };
    let mut stream = oxc_parser::Parser::new(&allocator, &text[body.start as usize..body.end as usize], source_type)
        .with_options(options)
        .statement_stream();
    let mut fine = true;
    loop {
        match stream.next(RUN_STATEMENTS, RUN_BYTES, &mut |_| {}, &mut |statements, _| fine &= straight_line(statements)) {
            oxc_parser::StreamStep::More if fine => {}
            oxc_parser::StreamStep::Done => return fine,
            _ => return false,
        }
    }
}

/// Whether expressions leave every function-wide analysis of their body unchanged.
struct StraightLine {
    fine: bool,
}

impl<'a> Visit<'a> for StraightLine {
    fn visit_function(&mut self, _: &Function<'a>, _: oxc_syntax::scope::ScopeFlags) {
        self.fine = false;
    }
    fn visit_arrow_function_expression(&mut self, _: &ArrowFunctionExpression<'a>) {
        self.fine = false;
    }
    fn visit_class(&mut self, _: &Class<'a>) {
        self.fine = false;
    }
    fn visit_this_expression(&mut self, _: &ThisExpression) {
        self.fine = false;
    }
    fn visit_super(&mut self, _: &Super) {
        self.fine = false;
    }
    fn visit_new_target(&mut self, _: &NewTarget) {
        self.fine = false;
    }
    fn visit_import_meta(&mut self, _: &ImportMeta) {
        self.fine = false;
    }
    fn visit_import_expression(&mut self, _: &ImportExpression<'a>) {
        self.fine = false;
    }
    fn visit_tagged_template_expression(&mut self, _: &TaggedTemplateExpression<'a>) {
        self.fine = false;
    }
    fn visit_reg_exp_literal(&mut self, _: &RegExpLiteral<'a>) {
        self.fine = false;
    }
    fn visit_private_field_expression(&mut self, _: &PrivateFieldExpression<'a>) {
        self.fine = false;
    }
    fn visit_private_in_expression(&mut self, _: &PrivateInExpression<'a>) {
        self.fine = false;
    }
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if matches!(identifier.name.as_str(), "arguments" | "eval") {
            self.fine = false;
        }
    }
    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'a>) {
        if !matches!(expression.left, AssignmentTarget::StaticMemberExpression(_) | AssignmentTarget::ComputedMemberExpression(_)) {
            self.fine = false;
            return;
        }
        walk::walk_assignment_expression(self, expression);
    }
    fn visit_update_expression(&mut self, expression: &UpdateExpression<'a>) {
        if !matches!(expression.argument, SimpleAssignmentTarget::StaticMemberExpression(_) | SimpleAssignmentTarget::ComputedMemberExpression(_)) {
            self.fine = false;
            return;
        }
        walk::walk_update_expression(self, expression);
    }
    fn visit_unary_expression(&mut self, expression: &UnaryExpression<'a>) {
        if expression.operator == UnaryOperator::Delete && matches!(expression.argument.without_parentheses(), Expression::Identifier(_)) {
            self.fine = false;
            return;
        }
        walk::walk_unary_expression(self, expression);
    }
}

impl FunctionCompiler<'_, '_> {
    /// Compiles `body`'s statements from its text a run at a time; false if the text does not
    /// parse as the load's check found it would.
    pub(super) fn streamed_body(&mut self, body: StreamedBody) -> bool {
        let text = &self.owner.text[body.start as usize..body.end as usize];
        let allocator = oxc_allocator::Allocator::with_capacity(STREAM_ARENA);
        let options = oxc_parser::ParseOptions { inherited_strict: body.strict, allow_return_outside_function: true, ..Default::default() };
        let mut stream = oxc_parser::Parser::new(&allocator, text, self.owner.source_type).with_options(options).statement_stream();
        let mut shift = super::replay::ShiftSpans(body.start);
        let mut last_end = body.start;
        loop {
            let step = stream.next(
                RUN_STATEMENTS,
                RUN_BYTES,
                &mut |statement| oxc_ast_visit::VisitMut::visit_statement(&mut shift, statement),
                &mut |statements, _| {
                    if let Some(last) = statements.last() {
                        last_end = oxc_span::GetSpan::span(last).end;
                    }
                    self.statements(statements)
                },
            );
            match step {
                oxc_parser::StreamStep::More if body.one_run => {
                    self.owner.found_next_run(Some(last_end));
                    return true;
                }
                oxc_parser::StreamStep::More => {}
                oxc_parser::StreamStep::Done => {
                    if body.one_run {
                        self.owner.found_next_run(None);
                    }
                    return true;
                }
                oxc_parser::StreamStep::Failed => return false,
            }
        }
    }
}
