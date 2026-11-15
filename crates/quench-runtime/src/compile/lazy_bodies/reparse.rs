//! A lazily parsed function's text parsed again alone, where the function compiles.
use super::{parse, stand_in};
use crate::compile::first_call::{FreeNameCheck, FreeNames};
use oxc_allocator::Allocator;
use oxc_ast::ast::{ArrowFunctionBody, ArrowFunctionExpression, Expression, Function, Program, Statement};
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

/// A function's text parsed again alone, with every position an offset into the program's
/// text.
pub(in crate::compile) struct Reparsed<'a> {
    pub(in crate::compile) program: Program<'a>,
}

impl<'a> Reparsed<'a> {
    /// The function parsed again.
    pub(in crate::compile) fn function(&self) -> Option<&Function<'a>> {
        Self::function_of(&self.program)
    }

    /// The arrow function parsed again, in its wrapper (`replay::arrow_wrapper`).
    pub(in crate::compile) fn arrow(&self) -> Option<&ArrowFunctionExpression<'a>> {
        crate::compile::replay::wrapped_arrow(&self.program)
    }

    /// Where the body of the function, or of the arrow, parsed again starts.
    fn own_body_start(program: &Program<'a>) -> Option<u32> {
        use crate::compile::replay::{wrapped_arrow, wrapped_class_method, wrapped_object_method};
        if let Some(function) = Self::function_of(program).or_else(|| wrapped_object_method(program)) {
            return function.body.as_ref().map(|body| body.span.start);
        }
        if let Some(method) = wrapped_class_method(program) {
            return method.value.body.as_ref().map(|body| body.span.start);
        }
        match &wrapped_arrow(program)?.body {
            ArrowFunctionBody::FunctionBody(body) => Some(body.span.start),
            _ => None,
        }
    }

    fn function_of<'p>(program: &'p Program<'a>) -> Option<&'p Function<'a>> {
        match program.body.first()? {
            Statement::FunctionDeclaration(function) => Some(function),
            Statement::ExpressionStatement(statement) => match statement.expression.without_parentheses() {
                Expression::FunctionExpression(function) => Some(function),
                _ => None,
            },
            _ => None,
        }
    }
}

/// Which bodies inside a function parsed again are released.
pub(in crate::compile) enum Nesting<'s> {
    /// Those the parser releases (`ParseOptions::lazy_function_bodies`): a load's, whose
    /// free names (`first_call::free_names`) join the given ones.
    Parse(&'s mut FreeNames, bool),
    /// Those of the given starts, in the program's text, the load released: a replay's,
    /// which compiles as the load did whatever the parser's choices.
    Recorded(&'s FxHashSet<u32>),
}

/// A first-call chunk's `text`, a function's source with its function at `offset`, parsed with the
/// bodies inside the function skimmed where they may compile on first call too: their free names
/// join `found` at their places in the program's text, `delta` past their places in `text`.
pub(in crate::compile) fn parse_chunk<'a>(
    allocator: &'a Allocator,
    text: &'a str,
    offset: u32,
    delta: u32,
    inherited_strict: bool,
    found: &mut FreeNames,
) -> oxc_parser::ParserReturn<'a> {
    let names = std::cell::RefCell::new(FreeNameCheck::new(found, delta, true));
    let mut skim = |body: oxc_parser::SkimmedBody<'_>| names.borrow_mut().skim(body, text);
    // SAFETY: `skim` lives through the parse, which alone calls it.
    let skim = unsafe { oxc_parser::LazyBodySkim::new(&mut skim) };
    parse(allocator, text, SourceType::script(), Some(offset + 1), None, Some(skim), inherited_strict)
}

/// The bodies of `program`, a function's text parsed again at `offset`, which the load released
/// (`starts`, in the program's text, `delta` past it) stand in, but the function's own.
fn stand_in_recorded<'a>(allocator: &'a Allocator, program: &mut Program<'a>, starts: impl Iterator<Item = u32>, (offset, delta): (u32, u32), len: usize) {
    let own = Reparsed::own_body_start(program);
    let inside: FxHashSet<u32> = starts
        .map(|start| start.wrapping_sub(delta))
        .filter(|start| *start > offset && (*start as usize) < len && Some(*start) != own)
        .collect();
    stand_in(allocator, program, &inside);
}

/// `text`, a function's source at `offset` wrapped for the context it is compiled in, parsed
/// again with the bodies inside it released as `nesting` says; positions move by `delta`
/// into the program's text.
pub(in crate::compile) fn reparse<'a>(
    allocator: &'a Allocator,
    text: &'a str,
    offset: u32,
    delta: u32,
    source_type: SourceType,
    nesting: Nesting<'_>,
) -> Option<Reparsed<'a>> {
    let (names, recorded) = match nesting {
        Nesting::Parse(found, direct) => (Some(FreeNameCheck::new(found, delta, direct)), None),
        Nesting::Recorded(starts) => (None, Some(starts)),
    };
    let lazy_from = names.is_some().then_some(offset + 1);
    let names = std::cell::RefCell::new(names);
    let mut check = |body: oxc_parser::LazyBody<'_, '_>| if let Some(names) = names.borrow_mut().as_mut() { names.check(body) };
    let mut skim = |body: oxc_parser::SkimmedBody<'_>| names.borrow_mut().as_mut().is_some_and(|names| names.skim(body, text));
    // SAFETY: `check` and `skim` live through the parse, which alone calls them.
    let hooks = lazy_from.map(|_| unsafe { (oxc_parser::LazyBodyCheck::new(&mut check), oxc_parser::LazyBodySkim::new(&mut skim)) });
    let checks = names.borrow().as_ref().is_none_or(FreeNameCheck::checks);
    let mut parsed = parse(allocator, text, source_type, lazy_from, hooks.and_then(|hooks| checks.then_some(hooks.0)), hooks.map(|hooks| hooks.1), false);
    if let Some(starts) = recorded {
        stand_in_recorded(allocator, &mut parsed.program, starts.iter().copied(), (offset, delta), text.len());
    }
    // The early checks passed before code generation (`BodyFacts::check`); the depth of the tree
    // is checked again here, where the function compiles.
    if parsed.stack_exhausted || !parsed.diagnostics.is_empty() || crate::compile::stack::validate(&parsed.program).is_err() {
        return None;
    }
    oxc_ast_visit::VisitMut::visit_program(&mut crate::compile::replay::ShiftSpans(delta), &mut parsed.program);
    Some(Reparsed { program: parsed.program })
}
