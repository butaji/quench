//! What the checks of the lazily parsed bodies leave for the compilation.
use oxc_allocator::Allocator;
use oxc_ast::ast::{Program, Statement};
use oxc_span::SourceType;
use rustc_hash::FxHashSet;
use std::cell::RefCell;

/// What the whole program's analysis would have found inside its lazily parsed bodies, with
/// every position an offset into the program's text.
#[derive(Default)]
pub(in crate::compile) struct BodyFacts {
    /// Private names, numbered from 0 in source order; a compilation numbers them on from
    /// the program's (`private_name_ids`).
    pub(in crate::compile) private_name_ids: rustc_hash::FxHashMap<(u32, u32), u32>,
    pub(in crate::compile) private_name_labels: rustc_hash::FxHashMap<(u32, u32), String>,
    /// Function expressions whose `prototype` no program text observes (`call_only`), unless
    /// `call_only_deferred`.
    pub(in crate::compile) prototype_unobservable: FxHashSet<u32>,
    /// The functions may compile on first call: their call-only facts wait for the
    /// compilation that reads them (`call_only::deferred`), unless their load compiles them.
    pub(in crate::compile) call_only_deferred: bool,
    /// Every body is checked alone by its semantic analysis, whatever its parse's verdict
    /// (`LazyBody::early`) says. A module compiles without an analysis of its own first
    /// (`Vm::collect_static_module_programs`), so its compilation's checks are what rule out
    /// its early errors.
    pub(in crate::compile) checks_every_body: bool,
    /// Arrow functions' bodies may be taken (`first_call::Stubs::arrows`).
    arrows: bool,
    /// The bodies the skim takes where it decides them (`parse_collecting`), a body it declines
    /// parsing whole and checked, unless `arrows` (a direct eval's, unchecked).
    skim: bool,
    /// The skimmed bodies a script may stream (`statement_stream`), which only a check finds: a
    /// body of `STREAM_MIN_BODY` bytes or more is declined by the skim.
    streams: bool,
    /// The functions whose call-only facts are found, by source range.
    pub(in crate::compile) call_only_found: Vec<(u32, u32)>,
    next_private_id: u32,
    /// Some body failed an early check alone.
    pub(in crate::compile) failed: bool,
    /// Each body's free names (`first_call::free_names`).
    pub(in crate::compile) free_names: crate::compile::first_call::FreeNames,
    /// The bodies checked whose free names do not bound what they reach by name.
    unbounded: FxHashSet<u32>,
    /// The functions whose bodies the semantic analysis of `check` found free of early errors, by
    /// start. A first-call chunk of one takes that verdict instead of checking its text again
    /// (`ProgramReplay::checked_bodies`).
    pub(in crate::compile) checked: Vec<u32>,
    /// The functions whose bodies compile as a stream of statement runs (`statement_stream`).
    pub(in crate::compile) streamed: FxHashSet<u32>,
    /// The functions checked without their tree whose bodies may stream, which their
    /// compilation finds by reading their statements first (`statement_stream`).
    pub(in crate::compile) stream_candidates: FxHashSet<u32>,
    pub(in crate::compile) native_source_requests: Option<(bool, Vec<crate::compile::NativeSourceKey>)>,
}

impl BodyFacts {
    /// No facts yet, for a source compiled as `stubbing` says: its call-only facts deferred where
    /// its functions compile on first call, its arrow functions' bodies taken, its bodies skimmed.
    /// `checks_every_body`: each body is checked alone, not by its parse's verdict.
    pub(in crate::compile) fn deferring(stubbing: crate::compile::program_shape::Stubbing, checks_every_body: bool) -> Self {
        // A direct eval's streamed roots are none (its source is not replayable), so only a
        // script's large bodies are declined.
        let streams = stubbing.skim && !stubbing.arrows;
        Self { call_only_deferred: stubbing.functions, checks_every_body, arrows: stubbing.arrows, skim: stubbing.skim, streams, ..Self::default() }
    }

    /// Whether a body's parse verdict stands for its check (`checked_by_parse`): not where every
    /// body is checked (`checks_every_body`), nor under `QUENCH_DEV=early-verify`.
    fn takes_parse_verdict(&self) -> bool {
        !self.checks_every_body && !verify_early()
    }

    /// Checks one function the parser hands over before releasing its body
    /// (`ParseOptions::lazy_body_check`), alone with the strictness it inherits and the
    /// bodies inside it whole, and adds its facts. The facts are gathered before code
    /// generation: an analysis per function between the records code generation keeps left
    /// the heap their holes (JetStream +2-3% max RSS).
    /// False where `stub` and the check needs the body's tree (`LazyBody::stub`).
    /// A TypeScript body is checked as its source is, before admission (which runs where it compiles,
    /// `lazy_bodies::parse`): where admission would rewrite or reject a construct in it, the body
    /// fails the check (`needs_admission`), and the program compiles eagerly, which reports the errors
    /// at load. Its declarations are collected where it compiles, with the program's type names
    /// (`reparse`), since the program's names are not known until its whole tree is parsed; a stub has
    /// no tree to check, so it is parsed whole.
    pub(in crate::compile) fn check(&mut self, oxc_parser::LazyBody { program, call_assignment_targets, in_function, early, stub, functions_inside }: oxc_parser::LazyBody<'_, '_>, global_script_calls: &[(String, String)]) -> bool {
        if self.failed {
            return true;
        }
        let typescript = program.source_type.is_typescript();
        if stub {
            return !typescript && early.is_some_and(|early| self.takes_parse_verdict() && self.checked_by_parse(program, call_assignment_targets, in_function, early, functions_inside));
        }
        if typescript {
            if crate::compile::typescript::needs_admission(program) {
                self.failed = true;
                return true;
            }
        } else {
            self.streamed.extend(crate::compile::statement_stream::streamable_start(program));
        }
        if verify_early() && crate::trace::Dev::EarlyVerify.value() == Some("stat") {
            eprintln!("early-stat: {} {} {} deferred={} at_load={}", if early.is_some() { "sure" } else { "unsure" }, program.span.start, program.span.end - program.span.start, self.call_only_deferred, crate::compile::call_only::compiles_at_load(program, false, false));
        }
        if let Some(early) = early
            && self.takes_parse_verdict()
            && self.checked_by_parse(program, call_assignment_targets, in_function, early, true)
        {
            return true;
        }
        // Its early errors are OXC's alone: the program carries the strictness the body
        // inherits as a directive, which `early::program_early_errors` exists to supply where
        // OXC cannot see it (a whole program in inherited strict code).
        if crate::compile::stack::validate(program).is_err()
            || call_assignment_targets && crate::compile::annex_b_targets::invalid_target(program).is_some()
        {
            self.failed = true;
            return true;
        }
        let built = crate::compile::semantic_stats::builder(program).with_check_syntax_error(true).build(program);
        if verify_early() && early.is_some() && !built.diagnostics.is_empty() {
            eprintln!("early-verify: parse sure but semantic reports at {}: {:?}", program.span.start, built.diagnostics.iter().map(ToString::to_string).collect::<Vec<_>>());
        }
        if !built.diagnostics.is_empty() {
            self.failed = true;
            return true;
        }
        self.checked.push(program.span.start);
        let inner = self.compose_released(program, built.semantic.scoping());
        if verify_early()
            && let Some(early) = early
        {
            let semantic = inner.bounded.then(|| crate::compile::first_call::free_names(program, built.semantic.scoping(), &inner.free, &|start| self.bounds_names(start))).flatten();
            let tracked = parse_free_names(program, early, &|start| self.bounds_names(start));
            if semantic.as_ref().map(|names| &names.1) != tracked.as_ref().map(|names| &names.1) {
                eprintln!("early-verify: free names differ at {}: semantic {:?} parse {:?}", program.span.start, semantic.map(|names| names.1), tracked.map(|names| names.1));
            }
        }
        if let Some((specialized, requests)) = &mut self.native_source_requests {
            requests.extend(crate::compile::Engine::native_source_requests(program, built.semantic.scoping(), *specialized, global_script_calls));
        }
        self.add_facts(&built.semantic, program, in_function, &inner);
        true
    }

    /// Takes a function whose body the parser skimmed (`ParseOptions::lazy_body_skim`, given where
    /// the functions compile on first call, `parse_collecting`): valid, with its free names. A
    /// function about to be called where it is made, or whose names it cannot bound inside a
    /// function, is declined and its body parses whole. A script's large body is declined too
    /// (`streams`): it may stream, which only its check finds. A skimmed body has no facts of
    /// its own: it is not checked (its first-call chunk checks its text), and its call-only facts
    /// and streams are found where it compiles.
    pub(in crate::compile) fn skim(&mut self, body: oxc_parser::SkimmedBody<'_>, text: &str) -> bool {
        if self.native_source_requests.is_some() {
            return false;
        }
        let names = crate::compile::first_call::skimmed_free_names(&body);
        if self.failed
            || body.arrow && !self.arrows
            || body.in_function && names.is_none()
            || self.streams && body.end.wrapping_sub(body.start) >= crate::compile::statement_stream::STREAM_MIN_BODY
            || crate::compile::call_only::invoked_where_made(text, body.end)
        {
            return false;
        }
        if let Some(names) = names {
            self.free_names.insert(body.start, names, Some(body.end));
        }
        true
    }

    /// Whether the body of the function at `start`, released and checked alone, bounds what it
    /// reaches by name with its free names.
    fn bounds_names(&self, start: u32) -> bool {
        self.free_names.contains_key(&start) && !self.unbounded.contains(&start)
    }

    /// Takes the parse's verdict on a body it found no early error could lie in
    /// (`LazyBody::early`) where its facts need no analysis: no private name, and call-only
    /// facts waiting for its compilation or none to find, no function lying inside
    /// (`LazyBody::functions_inside`). Then its free names are the parse's, and no semantic
    /// analysis runs. False where the full check must run.
    fn checked_by_parse(&mut self, program: &Program<'_>, call_assignment_targets: bool, in_function: bool, early: &[&str], functions_inside: bool) -> bool {
        if self.native_source_requests.is_some() {
            return false;
        }
        // A private name makes the parse unsure: its facts need the analysis.
        if !self.call_only_deferred && functions_inside {
            return false;
        }
        let free_names = parse_free_names(program, early, &|start| self.bounds_names(start));
        if functions_inside && crate::compile::call_only::compiles_at_load(program, in_function && free_names.is_none(), false) {
            return false;
        }
        // No walk over the body runs here but the call assignment target check, so only that
        // needs the depth bound; the compilation that parses it again validates its depth
        // (`reparse`), as V8 reports a body too deep where it compiles it.
        if call_assignment_targets
            && (crate::compile::stack::validate(program).is_err() || crate::compile::annex_b_targets::invalid_target(program).is_some())
        {
            self.failed = true;
            return true;
        }
        if !functions_inside {
            // No function inside: no call-only facts to find, at load or where it compiles.
            self.call_only_found.push((program.span.start, program.span.end));
            // A typed body does not stream: its compilation parses it again with the program's names
            // (`reparse`), which the stream's own compilation does not.
            if !program.source_type.is_typescript() {
                self.stream_candidates.extend(crate::compile::statement_stream::streaming_function(program).map(|function| function.span.start));
            }
        }
        match free_names {
            Some((start, names)) => {
                self.free_names.insert(start, names, crate::compile::first_call::alone_body_end(program));
            }
            None => {
                if let Some(start) = crate::compile::first_call::alone_start(program) {
                    self.unbounded.insert(start);
                }
            }
        }
        true
    }

    /// What the bodies released inside `program` (`ParseOptions::lazy_nested_bodies`), each
    /// checked as the parser released it, reach outside themselves: `program`'s analysis sees
    /// each with its directives only.
    fn compose_released(&self, program: &oxc_ast::ast::Program<'_>, scoping: &oxc_semantic::Scoping) -> Released {
        let mut found = ReleasedInside { facts: self, inside: Vec::new() };
        oxc_ast_visit::Visit::visit_program(&mut found, program);
        let mut released = Released { bounded: true, reached: FxHashSet::default(), free: Vec::new() };
        for (start, scope) in found.inside {
            let Some(names) = self.free_names.get(&start).filter(|_| !self.unbounded.contains(&start)) else {
                released.bounded = false;
                continue;
            };
            let around = scope.and_then(|scope| scoping.scope_parent_id(scope));
            for name in names.iter() {
                released.reached.insert(name.clone());
                if around.is_none_or(|around| scoping.find_binding(around, (&**name).into()).is_none()) {
                    released.free.push(name.clone());
                }
            }
        }
        released
    }

    /// Adds the facts of a checked body: its private names, its free names, and its
    /// call-only facts unless they wait for a compilation that may never come.
    fn add_facts(&mut self, semantic: &oxc_semantic::Semantic<'_>, program: &oxc_ast::ast::Program<'_>, in_function: bool, inner: &Released) {
        let first_id = self.next_private_id;
        for (span, id) in crate::compile::private_name_ids(semantic) {
            self.private_name_ids.insert(span, first_id + id);
            self.next_private_id = self.next_private_id.max(first_id + id + 1);
        }
        self.private_name_labels.extend(crate::compile::private_name_labels(semantic));
        let free_names = inner.bounded.then(|| crate::compile::first_call::free_names(program, semantic.scoping(), &inner.free, &|start| self.bounds_names(start))).flatten();
        if free_names.is_none() && let Some(start) = crate::compile::first_call::alone_start(program) {
            self.unbounded.insert(start);
        }
        if !self.call_only_deferred || crate::compile::call_only::compiles_at_load(program, in_function && free_names.is_none(), self.next_private_id != first_id) {
            // A binding a released body reaches may be used there in any way.
            if inner.bounded {
                self.prototype_unobservable.extend(crate::compile::call_only::prototype_unobservable_functions(semantic, program, &inner.reached));
            }
            self.call_only_found.push((program.span.start, program.span.end));
        }
        if let Some((start, names)) = free_names {
            self.free_names.insert(start, names, crate::compile::first_call::alone_body_end(program));
        }
    }
}

/// Whether each checked body is checked both ways and the parse's verdict compared with the
/// semantic analysis' (`QUENCH_DEV=early-verify`), instead of taking the parse's where it is sure.
fn verify_early() -> bool {
    static VERIFY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *VERIFY.get_or_init(|| crate::trace::Dev::EarlyVerify.on())
}

/// A body's free names from the parse's (`LazyBody::early`), as `first_call::free_names`
/// gives them: with a declaration's own name, None where the body may reach its scopes by
/// name.
pub(in crate::compile) fn parse_free_names(program: &Program<'_>, early: &[&str], bounded: &dyn Fn(u32) -> bool) -> Option<(u32, Box<[Box<str>]>)> {
    if crate::compile::first_call::reaches_scopes_by_name(program, bounded) {
        return None;
    }
    let (start, own) = match program.body.first()? {
        Statement::FunctionDeclaration(function) => (function.span.start, function.id.as_ref().map(|id| id.name.as_str())),
        _ => (crate::compile::first_call::alone_start(program)?, None),
    };
    let mut names: Vec<Box<str>> = early.iter().copied().chain(own).map(Box::from).collect();
    names.sort_unstable();
    names.dedup();
    Some((start, names.into_boxed_slice()))
}

/// What the bodies released inside a checked body reach outside themselves
/// (`BodyFacts::compose_released`).
struct Released {
    /// Every released body's free names bound what it reaches by name.
    bounded: bool,
    /// Every free name of a released body.
    reached: FxHashSet<Box<str>>,
    /// Those the checked body does not bind where the released body lies: its own free names.
    free: Vec<Box<str>>,
}

/// The functions of a checked body whose bodies were released and checked inside it, by
/// start, with their scopes.
struct ReleasedInside<'f> {
    facts: &'f BodyFacts,
    inside: Vec<(u32, Option<oxc_syntax::scope::ScopeId>)>,
}

impl<'a> oxc_ast_visit::Visit<'a> for ReleasedInside<'_> {
    fn visit_function(&mut self, function: &oxc_ast::ast::Function<'a>, flags: oxc_syntax::scope::ScopeFlags) {
        let start = function.span.start;
        let released = function.body.as_ref().is_some_and(|body| body.statements.is_empty())
            && (self.facts.free_names.contains_key(&start) || self.facts.unbounded.contains(&start));
        if released {
            self.inside.push((start, function.scope_id.get()));
        } else {
            oxc_ast_visit::walk::walk_function(self, function, flags);
        }
    }
}

/// `source` parsed as a compilation parses it, the bodies of its functions starting at or
/// after `lazy` released when set. Where the functions compile on first call (`skim`), each
/// body is skimmed into `facts` where the skim takes it. A body the skim declines parses whole,
/// and is checked and recognized without its tree where it can be (`recognize`), unless the
/// arrows compile on first call too (`arrows`: a direct eval's, whose declined bodies stay
/// unchecked, as its chunks check them).
pub(in crate::compile) fn parse_collecting<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
    declared: &mut crate::bytecode::DeclaredTypes,
    lazy: Option<u32>,
    facts: &RefCell<BodyFacts>,
    inherited_strict: bool,
    recognize: crate::compile::annex_b_targets::Recognize,
    global_script_calls: &[(String, String)],
) -> oxc_parser::ParserReturn<'a> {
    let (direct, skimming) = {
        let facts = facts.borrow();
        (lazy.is_some() && facts.arrows, lazy.is_some() && facts.skim)
    };
    let mut check = |body: oxc_parser::LazyBody<'_, '_>| facts.borrow_mut().check(body, global_script_calls);
    let mut skim = |body: oxc_parser::SkimmedBody<'_>| facts.borrow_mut().skim(body, source);
    // SAFETY: `check` and `skim` live through the parse, which alone calls them.
    let (check, skim) = unsafe { ((lazy.is_some() && !direct).then(|| oxc_parser::LazyBodyCheck::new(&mut check)), skimming.then(|| oxc_parser::LazyBodySkim::new(&mut skim))) };
    super::parse(allocator, source, source_type, lazy, check, skim, inherited_strict, true, recognize, None, Some(declared))
}
