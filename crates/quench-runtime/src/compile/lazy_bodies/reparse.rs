//! A lazily parsed function's text parsed again alone, where the function compiles.
use super::{parse, stand_in};
use crate::compile::first_call::{FreeNameCheck, FreeNames};
use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, Function, Program, Statement};
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
    Parse(&'s mut FreeNames),
    /// Those of the given starts, in the program's text, the load released: a replay's,
    /// which compiles as the load did whatever the parser's choices.
    Recorded(&'s FxHashSet<u32>),
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
    let (mut names, recorded) = match nesting {
        Nesting::Parse(found) => (Some(FreeNameCheck::new(found, delta)), None),
        Nesting::Recorded(starts) => (None, Some(starts)),
    };
    let lazy_from = names.is_some().then_some(offset + 1);
    let mut check = |body: oxc_parser::LazyBody<'_, '_>| if let Some(names) = &mut names { names.check(body) };
    // SAFETY: `check` lives through the parse, which alone calls it.
    let mut parsed = parse(allocator, text, source_type, lazy_from, lazy_from.map(|_| unsafe { oxc_parser::LazyBodyCheck::new(&mut check) }), false);
    if let Some(starts) = recorded {
        // The function's own body compiles from this parse.
        let own = Reparsed::function_of(&parsed.program).and_then(|function| function.body.as_ref()).map(|body| body.span.start);
        let inside: FxHashSet<u32> = starts
            .iter()
            .map(|start| start.wrapping_sub(delta))
            .filter(|start| *start > offset && (*start as usize) < text.len() && Some(*start) != own)
            .collect();
        stand_in(allocator, &mut parsed.program, &inside);
    }
    // The early checks passed before code generation (`BodyFacts::check`); the depth of the tree
    // is checked again here, where the function compiles.
    if parsed.stack_exhausted || !parsed.diagnostics.is_empty() || crate::compile::stack::validate(&parsed.program).is_err() {
        return None;
    }
    oxc_ast_visit::VisitMut::visit_program(&mut crate::compile::replay::ShiftSpans(delta), &mut parsed.program);
    Some(Reparsed { program: parsed.program })
}
