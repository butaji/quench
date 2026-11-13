//! What the checks of the lazily parsed bodies leave for the compilation.
use oxc_allocator::Allocator;
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
    /// The functions whose call-only facts are found, by source range.
    pub(in crate::compile) call_only_found: Vec<(u32, u32)>,
    next_private_id: u32,
    /// Some body failed an early check alone.
    pub(in crate::compile) failed: bool,
    /// Each body's free names (`first_call::free_names`).
    pub(in crate::compile) free_names: crate::compile::first_call::FreeNames,
    #[cfg(feature = "aot")]
    pub(in crate::compile) native_source_requests: Option<(bool, Vec<crate::compile::NativeSourceKey>)>,
}

impl BodyFacts {
    /// No facts yet, the call-only ones `deferred` or not.
    pub(in crate::compile) fn deferring(call_only_deferred: bool) -> Self {
        Self { call_only_deferred, ..Self::default() }
    }

    /// Checks one function the parser hands over before releasing its body
    /// (`ParseOptions::lazy_body_check`), alone with the strictness it inherits and the
    /// bodies inside it whole, and adds its facts. The facts are gathered before code
    /// generation: an analysis per function between the records code generation keeps left
    /// the heap their holes (JetStream +2-3% max RSS).
    pub(in crate::compile) fn check(&mut self, oxc_parser::LazyBody { program, call_assignment_targets, in_function }: oxc_parser::LazyBody<'_, '_>, #[cfg(feature = "aot")] global_script_calls: &[(String, String)]) {
        if self.failed {
            return;
        }
        // Its early errors are OXC's alone: the program carries the strictness the body
        // inherits as a directive, which `early::program_early_errors` exists to supply where
        // OXC cannot see it (a whole program in inherited strict code).
        if crate::compile::stack::validate(program).is_err()
            || call_assignment_targets && crate::compile::annex_b_targets::invalid_target(program).is_some()
        {
            self.failed = true;
            return;
        }
        let built = crate::compile::semantic_stats::builder(program).with_check_syntax_error(true).build(program);
        if !built.diagnostics.is_empty() {
            self.failed = true;
            return;
        }
        #[cfg(feature = "aot")]
        if let Some((specialized, requests)) = &mut self.native_source_requests {
            requests.extend(crate::compile::Engine::native_source_requests(program, built.semantic.scoping(), *specialized, global_script_calls));
        }
        self.add_facts(&built.semantic, program, in_function);
    }

    /// Takes a function whose body the parser skimmed (`ParseOptions::lazy_body_skim`, given only
    /// where the functions may stub): valid, with its free names. It does what `check` and
    /// `add_facts` do for a body that stubs need nothing more of, and declines the rest, whose
    /// bodies the parser then checks: a function that compiles at load anyway needs the facts of
    /// `call_only::compiles_at_load`.
    pub(in crate::compile) fn skim(&mut self, body: oxc_parser::SkimmedBody<'_>, text: &str) -> bool {
        #[cfg(feature = "aot")]
        if self.native_source_requests.is_some() {
            return false;
        }
        let names = crate::compile::first_call::skimmed_free_names(&body, text);
        if self.failed || body.in_function && names.is_none() || crate::compile::call_only::invoked_where_made(text, body.end) {
            return false;
        }
        if let Some(names) = names {
            self.free_names.insert(body.start, names);
        }
        true
    }

    /// Adds the facts of a checked body: its private names, its free names, and its
    /// call-only facts unless they wait for a compilation that may never come.
    fn add_facts(&mut self, semantic: &oxc_semantic::Semantic<'_>, program: &oxc_ast::ast::Program<'_>, in_function: bool) {
        let first_id = self.next_private_id;
        for (span, id) in crate::compile::private_name_ids(semantic) {
            self.private_name_ids.insert(span, first_id + id);
            self.next_private_id = self.next_private_id.max(first_id + id + 1);
        }
        self.private_name_labels.extend(crate::compile::private_name_labels(semantic));
        let free_names = crate::compile::first_call::free_names(program, semantic.scoping());
        if !self.call_only_deferred || crate::compile::call_only::compiles_at_load(program, in_function && free_names.is_none(), self.next_private_id != first_id) {
            self.prototype_unobservable.extend(crate::compile::call_only::prototype_unobservable_functions(semantic, program));
            self.call_only_found.push((program.span.start, program.span.end));
        }
        self.free_names.extend(free_names);
    }
}


/// `source` parsed as a compilation parses it, the bodies of its functions released when
/// `lazy`: each is skimmed into `facts` where its functions may stub, or else checked.
pub(in crate::compile) fn parse_collecting<'a>(
    allocator: &'a Allocator,
    source: &'a str,
    source_type: SourceType,
    lazy: bool,
    facts: &RefCell<BodyFacts>,
    inherited_strict: bool,
    #[cfg(feature = "aot")] global_script_calls: &[(String, String)],
) -> oxc_parser::ParserReturn<'a> {
    let skims = lazy && facts.borrow().call_only_deferred;
    let mut check = |body: oxc_parser::LazyBody<'_, '_>| facts.borrow_mut().check(body, #[cfg(feature = "aot")] global_script_calls);
    let mut skim = |body: oxc_parser::SkimmedBody<'_>| facts.borrow_mut().skim(body, source);
    // SAFETY: `check` and `skim` live through the parse, which alone calls them.
    let (check, skim) = unsafe { (lazy.then(|| oxc_parser::LazyBodyCheck::new(&mut check)), skims.then(|| oxc_parser::LazyBodySkim::new(&mut skim))) };
    super::parse(allocator, source, source_type, lazy.then_some(0), check, skim, inherited_strict)
}
