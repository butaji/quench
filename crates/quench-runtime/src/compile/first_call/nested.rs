//! First-call stubs of nested functions (TOD-196, TOD-1481): a function inside another, whose
//! body was parsed lazily and that nothing at load calls, compiles as a stub. Its enclosing
//! functions' layouts need only its captures, which its free names give (`free_names`),
//! resolved through the scopes it closes over: exact on every plain function of JetStream's
//! sources (TOD-196 stage 1). Its chunk compiles its text against those scopes, kept as
//! names (`FirstCallScopes`), so its code reads its captures where the eager compile would.
use super::*;
use capture_scope::{CaptureScope, ScopeChain};
use std::rc::Rc;

/// The names a lazily parsed function's subtree reads from outside it, by the function's start.
pub(in crate::compile) type FreeNames = FxHashMap<u32, Box<[Box<str>]>>;

/// One scope level a stub's function closes over: its bindings by name and slot, the names
/// packed into one text so a level costs three allocations however many names it binds.
#[derive(Debug)]
pub(crate) struct ScopeNames {
    text: Box<str>,
    /// Each binding's end in `text`, and its slot.
    ends: Box<[(u32, u16)]>,
}

pub(crate) type ScopeLevel = Rc<ScopeNames>;

impl ScopeNames {
    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, u16)> {
        let starts = std::iter::once(0).chain(self.ends.iter().map(|&(end, _)| end));
        starts.zip(self.ends.iter()).map(|(start, &(end, slot))| (&self.text[start as usize..end as usize], slot))
    }
}

impl<'n> FromIterator<(&'n str, u16)> for ScopeNames {
    fn from_iter<I: IntoIterator<Item = (&'n str, u16)>>(bindings: I) -> Self {
        let (mut text, mut ends) = (String::new(), Vec::new());
        for (name, slot) in bindings {
            text.push_str(name);
            ends.push((text.len() as u32, slot));
        }
        Self { text: text.into(), ends: ends.into() }
    }
}

/// A program's nested stubs by id, sorted, each with the scopes its chunk compiles against.
pub(crate) type FirstCallStubs = Box<[(u32, Rc<FirstCallScopes>)]>;

/// What a nested stub's chunk compiles its function against: the scopes it closes over,
/// innermost first, and the strictness it inherits.
#[derive(Debug)]
pub(crate) struct FirstCallScopes {
    levels: Box<[ScopeLevel]>,
    strict: bool,
}

/// The free names of `program`, a lazily parsed function alone (`BodyFacts::check`): the
/// references its own analysis leaves unresolved, but its own `arguments`, and a declaration's
/// own name, which the scope around it binds. None for a function that may reach its scopes
/// by name (direct `eval`, `with`), whose captures its names do not bound.
pub(in crate::compile) fn free_names(program: &Program<'_>, scoping: &oxc_semantic::Scoping) -> Option<(u32, Box<[Box<str>]>)> {
    let text = program.source_text.get(program.span.start as usize..program.span.end as usize)?;
    if reaches_scopes_by_name(text) {
        return None;
    }
    let (start, own) = match program.body.first()? {
        Statement::FunctionDeclaration(function) => (function.span.start, function.id.as_ref().map(|id| id.name.as_str())),
        Statement::ExpressionStatement(statement) => match statement.expression.without_parentheses() {
            Expression::FunctionExpression(function) => (function.span.start, None),
            _ => return None,
        },
        _ => return None,
    };
    let mut names: Vec<Box<str>> = scoping
        .root_unresolved_references()
        .keys()
        .map(|name| name.as_str())
        .filter(|name| *name != "arguments")
        .chain(own)
        .map(Box::from)
        .collect();
    names.sort_unstable();
    names.dedup();
    Some((start, names.into_boxed_slice()))
}

/// Whether the source of a function may reach its scopes by name (direct `eval`, `with`), which
/// its free names do not bound.
fn reaches_scopes_by_name(text: &str) -> bool {
    text.contains("eval") || text.contains("with")
}

/// The free names of `body`, a function the parser skimmed in `text`: those of `free_names` of
/// its tree. None where that has none.
pub(in crate::compile) fn skimmed_free_names(body: &oxc_parser::SkimmedBody<'_>, text: &str) -> Option<Box<[Box<str>]>> {
    if text.get(body.start as usize..body.end as usize).is_none_or(reaches_scopes_by_name) {
        return None;
    }
    let mut names: Vec<Box<str>> = body.free_names.iter().copied().filter(|name| *name != "arguments").chain(body.id).map(Box::from).collect();
    names.sort_unstable();
    names.dedup();
    Some(names.into_boxed_slice())
}

/// The free names of the bodies a parse of one function's text releases, added to `found` at
/// their places in the program's text, `delta` past their places in the parsed text.
pub(in crate::compile) struct FreeNameCheck<'f> {
    found: &'f mut FreeNames,
    delta: u32,
}

impl<'f> FreeNameCheck<'f> {
    pub(in crate::compile) fn new(found: &'f mut FreeNames, delta: u32) -> Self {
        Self { found, delta }
    }

    /// Adds the free names of `body`, a function the parser skimmed in `text`: it takes it.
    pub(in crate::compile) fn skim(&mut self, body: oxc_parser::SkimmedBody<'_>, text: &str) -> bool {
        if let Some(names) = skimmed_free_names(&body, text) {
            self.found.insert(body.start.wrapping_add(self.delta), names);
        }
        true
    }

    /// Adds the free names of `body`, as the parser releases it.
    pub(in crate::compile) fn check(&mut self, body: oxc_parser::LazyBody<'_, '_>) {
        let built = crate::compile::semantic_stats::builder(body.program).build(body.program);
        if let Some((start, names)) = free_names(body.program, built.semantic.scoping()) {
            self.found.insert(start.wrapping_add(self.delta), names);
        }
    }
}

impl FirstCallScopes {
    pub(crate) fn new(levels: Box<[ScopeLevel]>, strict: bool) -> Self {
        Self { levels, strict }
    }

    pub(crate) fn levels(&self) -> &[ScopeLevel] {
        &self.levels
    }

    pub(crate) fn strict(&self) -> bool {
        self.strict
    }

    /// The chain the chunk's function closes over, its names interned in `atoms`.
    fn chain(&self, atoms: &mut CompilerAtoms<'_>) -> ScopeChain {
        ScopeChain::new(
            self.levels
                .iter()
                .map(|level| Rc::new(level.iter().map(|(name, slot)| (atoms.intern(name), slot)).collect::<CaptureScope>()))
                .collect(),
        )
    }
}

impl Compiler<'_> {
    /// The captures `function`, whose body was parsed lazily, would make, as (distance, slot),
    /// when it may compile as a nested stub: a plain function of an indirect eval's source,
    /// not about to run, whose free names are known, inside no scope reached by name and
    /// naming no private member.
    pub(super) fn nested_stub_captures(&self, function: &oxc_ast::ast::Function<'_>, context: &replay::PlainFunctionContext) -> Option<Box<[(u32, u16)]>> {
        if !self.nested_stub_place(context) || !self.nested_stub_function(function, context) {
            return None;
        }
        let names = self.first_call.free_names.get(&function.span.start)?;
        let mut captures: Vec<(u32, u16)> = names
            .iter()
            .filter_map(|name| self.atoms.find(name))
            .filter_map(|atom| context.scopes.find(atom))
            .map(|(depth, slot)| (depth as u32 + 1, slot))
            .collect();
        captures.sort_unstable();
        captures.dedup();
        Some(captures.into_boxed_slice())
    }

    /// Whether the place of `context` lets a function compile as a nested stub: its source's
    /// first compile, inside some scope, none reached by name.
    fn nested_stub_place(&self, context: &replay::PlainFunctionContext) -> bool {
        self.first_call.nested_allowed
            && self.replay_target.is_none()
            && self.dynamic_binding_scopes == 0
            && context.with_depth == 0
            && !context.scopes.levels().is_empty()
    }

    /// Whether `function` itself may compile as a nested stub: plain, unnamed as an
    /// expression, not about to run, naming no private member.
    fn nested_stub_function(&self, function: &oxc_ast::ast::Function<'_>, context: &replay::PlainFunctionContext) -> bool {
        let span = function.span.start..function.span.end;
        !function.r#async
            && !function.generator
            && !(context.expression && function.id.is_some())
            && !self.invoked_functions.contains(&function.span.start)
            && !self.private_name_ids.keys().any(|(start, _)| span.contains(start))
    }

    /// Notes stub `id` of a nested function: its predicted `captures` decide the layouts
    /// around it, and its chunk compiles against the scopes of `context`.
    pub(super) fn note_nested_stub(&mut self, id: u32, captures: Box<[(u32, u16)]>, context: &replay::PlainFunctionContext) {
        self.function_facts.at_mut(id).captures = captures;
        let levels = context.scopes.levels().iter().map(|scope| self.scope_level(scope)).collect();
        self.first_call.nested.push((id, Rc::new(FirstCallScopes { levels, strict: context.inherited_strict })));
    }

    /// `scope`'s bindings by name, made once per scope and shared by the stubs closing over it.
    fn scope_level(&mut self, scope: &Rc<CaptureScope>) -> ScopeLevel {
        let key = Rc::as_ptr(scope) as usize;
        if let Some((_, level)) = self.first_call.levels.get(&key) {
            return level.clone();
        }
        let level: ScopeLevel = Rc::new(scope.iter().map(|(atom, slot)| (&self.atoms[atom as usize], slot)).collect());
        // The scope stays alive with the key, so its address names no other scope.
        self.first_call.levels.insert(key, (scope.clone(), level.clone()));
        level
    }

    /// Whether a nested stub lies among the functions after `id` up to `exit`: its layout
    /// rests on predicted captures, so no replay of an enclosing function compiles it again.
    pub(in crate::compile) fn encloses_nested_stub(&self, id: u32, exit: u32) -> bool {
        let nested = &self.first_call.nested;
        let after = nested.partition_point(|(stub, _)| *stub <= id);
        nested.get(after).is_some_and(|(stub, _)| *stub < exit)
    }

    /// The chain a chunk compiles its stub's function at `start` against, in place of the
    /// chunk's own scopes, once: the stub's (`Stubs::outer`).
    pub(in crate::compile) fn take_outer_scopes(&mut self, start: u32, context: &mut replay::PlainFunctionContext) {
        if self.first_call.outer.as_ref().is_some_and(|(target, _)| *target == start)
            && let Some((_, scopes)) = self.first_call.outer.take()
        {
            context.scopes = scopes.chain(&mut self.atoms);
            context.inherited_strict = scopes.strict;
            context.with_depth = 0;
        }
    }
}
