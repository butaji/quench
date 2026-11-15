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

    /// The function parsed again, to change.
    pub(in crate::compile) fn function_mut(&mut self) -> Option<&mut Function<'a>> {
        match self.program.body.first_mut()? {
            Statement::FunctionDeclaration(function) => Some(function),
            Statement::ExpressionStatement(statement) => match &mut statement.expression {
                Expression::FunctionExpression(function) => Some(function),
                Expression::ParenthesizedExpression(parenthesized) => match &mut parenthesized.expression {
                    Expression::FunctionExpression(function) => Some(function),
                    _ => None,
                },
                _ => None,
            },
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
) -> Result<Reparsed<'a>, ReparseFailure> {
    // The bodies inside whose free names and ends a parse before found are skipped unread: a
    // body nested deep is otherwise read again by each function around it that compiles.
    let mut known = rustc_hash::FxHashMap::default();
    let (mut names, recorded) = match nesting {
        Nesting::Parse(found) => {
            let from = delta.wrapping_add(offset + 1);
            if let Some(to) = from.checked_add((text.len() as u32).saturating_sub(offset + 1)) {
                known.extend(found.ends_in(from..to).map(|(start, end)| (start.wrapping_sub(delta), end.wrapping_sub(delta))));
            }
            (Some(FreeNameCheck::new(found, delta)), None)
        }
        Nesting::Recorded(starts) => (None, Some(starts)),
    };
    let skip = |start: u32| known.get(&start).copied();
    let lazy_from = names.is_some().then_some(offset + 1);
    let mut check = |body: oxc_parser::LazyBody<'_, '_>| {
        if let Some(names) = &mut names {
            names.check(body);
        }
        true
    };
    let skips = (lazy_from.is_some() && !known.is_empty() && crate::trace::Dev::Recognize.value() != Some("noskip"))
        // SAFETY: `skip` lives through the parse, which alone calls it.
        .then(|| unsafe { oxc_parser::LazyBodySkip::new(&skip) });
    // SAFETY: `check` lives through the parse, which alone calls it.
    let mut parsed = parse(allocator, text, source_type, lazy_from, lazy_from.map(|_| unsafe { oxc_parser::LazyBodyCheck::new(&mut check) }), false, false, crate::compile::annex_b_targets::Recognize::All, skips);
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
    // A body whose check at load took the parse's verdict has its depth checked only here.
    if parsed.stack_exhausted || crate::compile::stack::validate(&parsed.program).is_err() {
        return Err(ReparseFailure::StackExhausted);
    }
    if !parsed.diagnostics.is_empty() {
        return Err(ReparseFailure::Diagnostics);
    }
    oxc_ast_visit::VisitMut::visit_program(&mut crate::compile::replay::ShiftSpans(delta), &mut parsed.program);
    Ok(Reparsed { program: parsed.program })
}

/// Why a function's text does not parse again alone (`reparse`).
pub(in crate::compile) enum ReparseFailure {
    /// Its tree is deeper than the stack admits.
    StackExhausted,
    /// Its parse reports an error.
    Diagnostics,
}
