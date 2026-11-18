//! First-call stubs of nested functions (TOD-196, TOD-1481): a function inside another, whose
//! body was parsed lazily and that nothing at load calls, compiles as a stub. Its enclosing
//! functions' layouts need only its captures, which its free names give (`free_names`),
//! resolved through the scopes it closes over: exact on every plain function of JetStream's
//! sources (TOD-196 stage 1). Its chunk compiles its text against those scopes, kept as
//! names (`FirstCallScopes`), so its code reads its captures where the eager compile would.
use super::*;
use capture_scope::{CaptureScope, ScopeChain};
use std::rc::Rc;

/// The names a lazily parsed function's subtree reads from outside it, by the function's
/// start, and where its body ends.
#[derive(Default, Debug)]
pub(in crate::compile) struct FreeNames {
    names: FxHashMap<u32, Box<[Box<str>]>>,
    /// Each body's end, by its function's start, where it is known.
    ends: std::collections::BTreeMap<u32, u32>,
}

impl FreeNames {
    /// Notes the free names of the function at `start`, whose body ends at `end`.
    pub(in crate::compile) fn insert(&mut self, start: u32, names: Box<[Box<str>]>, end: Option<u32>) {
        self.names.insert(start, names);
        match end {
            Some(end) => self.ends.insert(start, end),
            None => self.ends.remove(&start),
        };
    }

    pub(in crate::compile) fn get(&self, start: &u32) -> Option<&Box<[Box<str>]>> {
        self.names.get(start)
    }

    pub(in crate::compile) fn contains_key(&self, start: &u32) -> bool {
        self.names.contains_key(start)
    }

    #[cfg(test)]
    pub(in crate::compile) fn iter(&self) -> impl Iterator<Item = (&u32, &Box<[Box<str>]>)> {
        self.names.iter()
    }

    /// The names with every position `delta` further on: a chunk's, found in its text alone.
    pub(in crate::compile) fn shifted(self, delta: u32) -> Self {
        Self {
            names: self.names.into_iter().map(|(start, names)| (start + delta, names)).collect(),
            ends: self.ends.into_iter().map(|(start, end)| (start + delta, end + delta)).collect(),
        }
    }

    /// The functions starting in `range` whose free names are known, with their bodies' ends.
    pub(in crate::compile) fn ends_in(&self, range: std::ops::Range<u32>) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.ends.range(range).map(|(start, end)| (*start, *end))
    }
}

/// Where the body of the function `program` holds alone (`alone_start`) ends.
pub(in crate::compile) fn alone_body_end(program: &Program<'_>) -> Option<u32> {
    let function = match program.body.first()? {
        Statement::FunctionDeclaration(function) => function,
        Statement::ExpressionStatement(statement) => match statement.expression.without_parentheses() {
            Expression::FunctionExpression(function) => function,
            _ => return None,
        },
        _ => return None,
    };
    function.body.as_ref().map(|body| body.span.end)
}

/// The bindings a stub's function reads from the scopes it closes over, by name: each with the
/// level of its scope (the innermost is 0) and its slot, the names packed into one text so the
/// stub costs two allocations however many names it reads.
#[derive(Debug)]
pub(crate) struct ScopeNames {
    text: Box<str>,
    /// Each binding's end in `text`, its scope level and its slot.
    ends: Box<[(u32, u16, u16)]>,
}

impl ScopeNames {
    pub(crate) fn len(&self) -> usize {
        self.ends.len()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, u16, u16)> {
        let starts = std::iter::once(0).chain(self.ends.iter().map(|&(end, ..)| end));
        starts.zip(self.ends.iter()).map(|(start, &(end, level, slot))| (&self.text[start as usize..end as usize], level, slot))
    }
}

impl<'n> FromIterator<(&'n str, u16, u16)> for ScopeNames {
    fn from_iter<I: IntoIterator<Item = (&'n str, u16, u16)>>(bindings: I) -> Self {
        let (mut text, mut ends) = (String::new(), Vec::new());
        for (name, level, slot) in bindings {
            text.push_str(name);
            ends.push((text.len() as u32, level, slot));
        }
        Self { text: text.into(), ends: ends.into() }
    }
}

/// A program's nested stubs by id, sorted, each with the scopes its chunk compiles against; the
/// chunks of its stubs append theirs.
pub(crate) type FirstCallStubs = crate::bytecode::append_vec::AppendVec<(u32, Rc<FirstCallScopes>)>;

/// What a nested stub's chunk compiles its function against: the bindings it reads from the
/// scopes it closes over (its free names, and the capture markers of those), and the strictness
/// it inherits.
#[derive(Debug)]
pub(crate) struct FirstCallScopes {
    /// How many scopes it closes over, innermost first.
    levels: u16,
    names: ScopeNames,
    strict: bool,
}

/// A nested stub's predicted captures, as (distance, slot), and the scopes its chunk compiles
/// against.
pub(in crate::compile) struct NestedStub {
    captures: Box<[(u32, u16)]>,
    scopes: FirstCallScopes,
}

/// The free names of `program`, a lazily parsed function alone (`BodyFacts::check`): the
/// references its own analysis leaves unresolved, but its own `arguments`, and a declaration's
/// own name, which the scope around it binds. None for a function that may reach its scopes
/// by name (direct `eval`, `with`), whose captures its names do not bound.
pub(in crate::compile) fn free_names(program: &Program<'_>, scoping: &oxc_semantic::Scoping, released: &[Box<str>]) -> Option<(u32, Box<[Box<str>]>)> {
    let text = program.source_text.get(program.span.start as usize..program.span.end as usize)?;
    if reaches_scopes_by_name(text) {
        return None;
    }
    let (start, own) = match program.body.first()? {
        Statement::FunctionDeclaration(function) => (function.span.start, function.id.as_ref().map(|id| id.name.as_str())),
        _ => (alone_start(program)?, None),
    };
    let mut names: Vec<Box<str>> = scoping
        .root_unresolved_references()
        .keys()
        .map(|name| name.as_str())
        .filter(|name| *name != "arguments")
        .chain(own)
        .map(Box::from)
        .chain(released.iter().filter(|name| &***name != "arguments").cloned())
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

/// The free name of a class method's that reads `super.x`: no binding's, a reserved word.
const SUPER_NAME: &str = "super";

/// The free names of `body`, a function the parser skimmed: those of `free_names` of its tree.
/// None where that has none: a direct `eval` reaches its scopes by name, and the skim decides
/// no body with a `with` statement. A method that reads `super.x` names `super`
/// (`SUPER_NAME`): its home binding, which its stub captures.
pub(in crate::compile) fn skimmed_free_names(body: &oxc_parser::SkimmedBody<'_>) -> Option<Box<[Box<str>]>> {
    if body.free_names.contains(&"eval") {
        return None;
    }
    let home = body.names_super.then_some(SUPER_NAME);
    let mut names: Vec<Box<str>> = body.free_names.iter().copied().filter(|name| *name != "arguments").chain(body.id).chain(home).map(Box::from).collect();
    names.sort_unstable();
    names.dedup();
    Some(names.into_boxed_slice())
}

/// The start of the function `program` holds alone, as `free_names` reads it.
/// A method checks alone as an object literal's one method (`ParseOptions::lazy_nested_bodies`).
pub(in crate::compile) fn alone_start(program: &Program<'_>) -> Option<u32> {
    match program.body.first()? {
        Statement::FunctionDeclaration(function) => Some(function.span.start),
        Statement::ExpressionStatement(statement) => match statement.expression.without_parentheses() {
            Expression::FunctionExpression(function) => Some(function.span.start),
            Expression::ObjectExpression(object) => match object.properties.as_slice() {
                [oxc_ast::ast::ObjectPropertyKind::ObjectProperty(property)] if property.method => match &property.value {
                    Expression::FunctionExpression(function) => Some(function.span.start),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// The free names of the bodies a parse of one function's text releases, added to `found` at
/// their places in the program's text, `delta` past their places in the parsed text.
pub(in crate::compile) struct FreeNameCheck<'f> {
    found: &'f mut FreeNames,
    delta: u32,
    /// The functions may compile on first call, arrow functions too, and the parser leaves the
    /// bodies it does not skim whole, with no check of its own (`Stubs::direct`).
    direct: bool,
}

impl<'f> FreeNameCheck<'f> {
    pub(in crate::compile) fn new(found: &'f mut FreeNames, delta: u32, direct: bool) -> Self {
        Self { found, delta, direct }
    }

    /// Whether the parser checks the bodies it does not skim alone, releasing them: unless the
    /// functions may compile on first call, which leaves them whole.
    pub(in crate::compile) fn checks(&self) -> bool {
        !self.direct
    }

    /// Adds the free names of `body`, a function the parser skimmed in `text` (only where the
    /// functions compile on first call, `checks` false), if it takes it: every one it can name
    /// the captures of, in a place that is not about to call it.
    pub(in crate::compile) fn skim(&mut self, body: oxc_parser::SkimmedBody<'_>, text: &str) -> bool {
        let names = skimmed_free_names(&body);
        if body.in_function && names.is_none() || crate::compile::call_only::invoked_where_made(text, body.end) {
            return false;
        }
        if let Some(names) = names {
            self.found.insert(body.start.wrapping_add(self.delta), names, Some(body.end.wrapping_add(self.delta)));
        }
        true
    }

    /// Adds the free names of `body`, as the parser releases it.
    pub(in crate::compile) fn check(&mut self, body: oxc_parser::LazyBody<'_, '_>) {
        // The parse's free names where it found them (`LazyBody::early`), else the analysis'.
        let found = match body.early {
            Some(early) => crate::compile::lazy_bodies::parse_free_names(body.program, early),
            None => {
                let built = crate::compile::semantic_stats::builder(body.program).build(body.program);
                free_names(body.program, built.semantic.scoping(), &[])
            }
        };
        if let Some((start, names)) = found {
            let end = alone_body_end(body.program).map(|end| end.wrapping_add(self.delta));
            self.found.insert(start.wrapping_add(self.delta), names, end);
        }
    }
}

impl FirstCallScopes {
    pub(crate) fn new(levels: u16, names: ScopeNames, strict: bool) -> Self {
        Self { levels, names, strict }
    }

    pub(crate) fn levels(&self) -> u16 {
        self.levels
    }

    pub(crate) fn names(&self) -> &ScopeNames {
        &self.names
    }

    pub(crate) fn strict(&self) -> bool {
        self.strict
    }

    /// The chain the chunk's function closes over, its names interned in `atoms`.
    fn chain(&self, atoms: &mut CompilerAtoms<'_>) -> ScopeChain {
        let mut levels = vec![Vec::new(); usize::from(self.levels)];
        for (name, level, slot) in self.names.iter() {
            levels[usize::from(level)].push((atoms.intern(name), slot));
        }
        ScopeChain::new(levels.into_iter().map(|bindings| Rc::new(bindings.into_iter().collect::<CaptureScope>())).collect())
    }
}

impl Compiler<'_> {
    /// The stub `function`, whose body was parsed lazily, may compile as: a plain function not
    /// about to run, naming no private member, in a place that lets one (`nested_stub_place`).
    pub(super) fn nested_stub_captures(&self, function: &oxc_ast::ast::Function<'_>, context: &replay::PlainFunctionContext) -> Option<NestedStub> {
        if !self.nested_stub_function(function, context) {
            return None;
        }
        self.stub_captures(function.span.start, &context.scopes, context.with_depth, context.inherited_strict, None)
    }

    /// The stub of `arrow`, whose body was parsed lazily: one that reads no `this`,
    /// `arguments`, `new.target` or `super` of the code around it (the skim decides no other),
    /// not about to run, naming no private member.
    pub(in crate::compile) fn nested_arrow_stub_captures(&self, arrow: &oxc_ast::ast::ArrowFunctionExpression<'_>, context: &replay::ArrowContext) -> Option<NestedStub> {
        let span = arrow.span.start..arrow.span.end;
        let plain = context.lexical_this_atom.is_none()
            && !context.class_initializer
            && !self.invoked_functions.contains(&arrow.span.start)
            && !self.private_name_ids.keys().any(|(start, _)| span.contains(start));
        plain.then(|| self.stub_captures(arrow.span.start, &context.scopes, context.with_depth, context.inherited_strict, None)).flatten()
    }

    /// The stub of `method`, a class's method whose body was parsed lazily: a plain method,
    /// getter or setter named by a key neither computed nor private (the parser skims no
    /// other), strict as its class is. One that reads `super.x` captures its home object too.
    pub(in crate::compile) fn nested_method_stub_captures(&self, method: &oxc_ast::ast::MethodDefinition<'_>, place: &lazy_bodies::MethodPlace<'_>) -> Option<NestedStub> {
        let plain = !method.value.r#async
            && !method.value.generator
            && !method.computed
            && method.kind != oxc_ast::ast::MethodDefinitionKind::Constructor
            && !matches!(method.key, oxc_ast::ast::PropertyKey::PrivateIdentifier(_));
        plain.then(|| self.stub_captures(method.value.span.start, place.scopes, place.with_depth, true, Some(place.home_atom))).flatten()
    }

    /// The captures of the function at `start`, from its free names resolved through `scopes`,
    /// and the bindings its chunk reads: those names', and the capture markers of them
    /// (`captured_binding_kind`); a method's that reads `super.x`, its `home` binding's too.
    fn stub_captures(&self, start: u32, scopes: &capture_scope::ScopeChain, with_depth: u16, strict: bool, home: Option<Atom>) -> Option<NestedStub> {
        if !self.nested_stub_place(scopes, with_depth) || self.first_call.chunk_function == Some(start) {
            return None;
        }
        let names = self.first_call.free_names.get(&start)?;
        let home = match names.iter().any(|name| &**name == SUPER_NAME) {
            true => Some(home.filter(|&home| scopes.find(home).is_some())?),
            false => None,
        };
        let mut captures = Vec::new();
        let mut bindings: Vec<(&str, u16, u16)> = Vec::new();
        for atom in names.iter().filter(|name| &***name != SUPER_NAME).filter_map(|name| self.atoms.find(name)).chain(home) {
            if let Some((depth, slot)) = scopes.find(atom) {
                captures.push((depth as u32 + 1, slot));
                bindings.push((&self.atoms[atom as usize], depth as u16, slot));
            }
            let kinds = crate::bytecode::LexicalBindingKind::ALL.map(Some).into_iter().chain([None]);
            for marker in kinds.filter_map(|kind| self.capture_markers.get(&(kind, atom)).copied()) {
                if let Some((depth, slot)) = scopes.find(marker) {
                    bindings.push((&self.atoms[marker as usize], depth as u16, slot));
                }
            }
        }
        captures.sort_unstable();
        captures.dedup();
        let levels = scopes.levels().len() as u16;
        Some(NestedStub { captures: captures.into_boxed_slice(), scopes: FirstCallScopes::new(levels, bindings.into_iter().collect(), strict) })
    }

    /// Whether a place lets a function compile as a nested stub: its source's first compile,
    /// in no scope reached by name; a replayable source's root functions are its roots'
    /// (`first_call_roots`).
    fn nested_stub_place(&self, scopes: &capture_scope::ScopeChain, with_depth: u16) -> bool {
        self.first_call.nested_allowed
            && self.replay_target.is_none()
            && self.dynamic_binding_scopes == 0
            && with_depth == 0
            && (!self.replayable_source || !scopes.levels().is_empty())
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

    /// Notes stub `id` of a nested function: its predicted captures decide the layouts around
    /// it, and its chunk compiles against its scopes.
    pub(in crate::compile) fn note_nested_stub(&mut self, id: u32, stub: NestedStub) {
        self.function_facts.at_mut(id).captures = stub.captures;
        self.first_call.nested.push((id, Rc::new(stub.scopes)));
    }

    /// Whether a nested stub lies among the functions after `id` up to `exit`: its layout
    /// rests on predicted captures, so no replay of an enclosing function compiles it again.
    pub(in crate::compile) fn encloses_nested_stub(&self, id: u32, exit: u32) -> bool {
        let nested = &self.first_call.nested;
        let after = nested.partition_point(|(stub, _)| *stub <= id);
        nested.get(after).is_some_and(|(stub, _)| *stub < exit)
    }

    /// Gives `adopt` the chain, and the strictness, a chunk compiles its stub's function at
    /// `start` against in place of the chunk's own scopes, once: the stub's (`Stubs::outer`).
    /// Takes the stub's scopes for its chunk's `context`, a class method's compiled as an object
    /// literal's (`ChunkText::of`): its home the class's binding the stub kept, where its body
    /// reads `super.x` (`stub_captures`), not the literal's.
    pub(in crate::compile) fn adopt_outer_method(&mut self, context: &mut replay::ObjectMethodContext) {
        let start = context.property_span.start;
        let home = self.first_call.outer.as_ref().filter(|(target, _)| *target == start).and_then(|(_, scopes)| {
            scopes.names.iter().map(|(name, ..)| name).find(|name| name.starts_with(crate::bytecode::HOME_BINDING_PREFIX)).map(str::to_owned)
        });
        if let Some(home) = home {
            context.home_atom = self.atom(&home);
        }
        self.take_outer(start, |scopes, strict| (context.scopes, context.inherited_strict, context.with_depth) = (scopes, strict, 0));
    }

    pub(in crate::compile) fn take_outer(&mut self, start: u32, adopt: impl FnOnce(ScopeChain, bool)) {
        if self.first_call.outer.as_ref().is_some_and(|(target, _)| *target == start)
            && let Some((_, scopes)) = self.first_call.outer.take()
        {
            adopt(scopes.chain(&mut self.atoms), scopes.strict);
        }
    }
}
