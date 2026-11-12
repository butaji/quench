//! What an eval's program is compiled with and keeps of its source.
use super::*;

/// The shared ASCII storage of `source`, the eval's string, when `text`, the text it compiles,
/// is all of it.
pub(super) fn shared_text(source: &JsString, text: &str) -> Option<std::rc::Rc<str>> {
    source
        .existing_flat()
        .and_then(crate::vm::wtf16::FlatUnits::shared_ascii)
        .filter(|(shared, range)| *range == (0..shared.len()) && shared.len() == text.len())
        .map(|(shared, _)| shared.clone())
}

/// Makes `residual` keep `shared`, its eval string's storage, in place of its copy of the
/// same text, as V8's Script keeps its source string (inspector code-load: 2 MB per eval,
/// each kept by the function it defines).
pub(super) fn keep_shared_text(residual: &mut ResidualProgram, shared: Option<std::rc::Rc<str>>) {
    if let (Some(shared), Some(source)) = (shared, residual.source.as_mut())
        && source.base == 0
        && *source.text == *shared
    {
        source.text = crate::bytecode::SourceText::Local(shared);
    }
}

/// Whether the compiled eval may serve a later eval of the same text: sloppy and without
/// tagged templates, whose site identities are per evaluation.
pub(super) fn is_cacheable(residual: &ResidualProgram, source: &str) -> bool {
    residual.functions.first().is_some_and(|root| !root.strict)
        && residual.replay.as_ref().map_or_else(|| !crate::Engine::eval_has_tagged_template(source), |replay| replay.template_sites == 0)
}

/// What an eval's compilation reads of its caller, and the direct-eval cache key it makes.
pub(super) struct CallerFacts {
    pub(super) context: crate::compile::EvalContext,
    pub(super) private_names: Vec<(String, String)>,
    pub(super) annex_b_forbidden_names: Vec<String>,
    pub(super) key: Option<DirectEvalKey>,
}

/// The identity a direct eval's program keeps of its caller: the eval origin its stack frames
/// print and the script or module its `import()` resolves from.
#[derive(Clone, PartialEq, Eq, Hash)]
pub(in crate::vm) struct EvalCaller {
    pub(in crate::vm) origin: JsString,
    pub(in crate::vm) script_or_module: std::rc::Rc<str>,
}

impl<H: Host> Vm<H> {
    /// The caller facts of the eval of `source`; a direct eval of a cached text also gets its
    /// cache key.
    pub(super) fn eval_caller_facts(&mut self, p: &ResidualProgram, source: &str, strict: bool, text: Option<&EvalSource>) -> Result<CallerFacts, JsError> {
        let context = self.direct_eval_context(p).unwrap_or_default();
        let private_names = if self.direct_eval {
            self.direct_eval_private_names(p).map_or_else(Vec::new, |(_, names)| names)
        } else {
            Vec::new()
        };
        let caller_has_global_variables = self.frames.is_empty() || self.running_global_code();
        // Annex B only concerns function declarations, which the text then names.
        let annex_b_forbidden_names = if source.contains("function") {
            self.annex_b_forbidden_names(caller_has_global_variables)
        } else {
            Vec::new()
        };
        let key = match text.filter(|_| self.direct_eval) {
            Some(source) => Some(DirectEvalKey {
                source: source.clone(),
                strict,
                context,
                private_names: private_names.clone(),
                annex_b_forbidden_names: annex_b_forbidden_names.clone(),
                caller: self.eval_caller(p)?,
            }),
            None => None,
        };
        Ok(CallerFacts { context, private_names, annex_b_forbidden_names, key })
    }

    /// The host's kept compilation of the script `source`, which compiles from its text alone:
    /// it is no direct eval, so it has no caller context, private names or Annex B restrictions.
    pub(super) fn kept_host_script(&mut self, source_name: &str, mode: &str, source: &str) -> Option<ResidualProgram> {
        let bytes = self.host.kept_compiled_program(source_name, &[mode, source])?;
        let text = crate::bytecode::SourceText::Shared(std::sync::Arc::from(source));
        let specialized = self.specialized;
        self.decode_cached_program(bytes, text, 0, source_name, specialized).ok()
    }

    /// The caller identity the eval's program is installed with.
    pub(super) fn eval_caller(&mut self, p: &ResidualProgram) -> Result<EvalCaller, JsError> {
        Ok(EvalCaller { origin: self.capture_eval_origin(p)?, script_or_module: self.active_script_or_module(p) })
    }

    /// The names a direct eval's Annex B function declarations may not bind: the caller's
    /// global lexical declarations (when it may declare globals) and the bindings a `var`
    /// would conflict with. Indirect eval has none.
    pub(super) fn annex_b_forbidden_names(&self, caller_has_global_variables: bool) -> Vec<String> {
        if !self.direct_eval {
            return Vec::new();
        }
        (0..crate::compile::AtomPrefix::len(self) as Atom)
            .filter(|atom| {
                (caller_has_global_variables && self.realm.global_lexical_declarations.contains(atom))
                    || self.direct_eval_binding(*atom).is_some_and(|binding| binding.conflicts_with_var())
            })
            .map(|atom| self.atom_name(atom).to_owned())
            .collect()
    }
}

/// A global eval's source text with its text hash, which the heap made once for the
/// string's record (`Heap::string_hash`): the cache's lookup, insertion and growth read the
/// hash instead of hashing the text again.
#[derive(Clone)]
pub(crate) struct EvalSource {
    pub(crate) hash: crate::vm::wtf16::TextHash,
    pub(crate) text: JsString,
}

impl EvalSource {
    /// The source of an eval of the heap string `value`, whose text is `text`.
    pub(super) fn of(heap: &crate::heap::Heap, value: Value, text: &JsString) -> Self {
        Self { hash: heap.string_hash(value).expect("eval source is a string"), text: text.clone() }
    }
}

impl PartialEq for EvalSource {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash && self.text == other.text
    }
}

impl Eq for EvalSource {}

impl std::hash::Hash for EvalSource {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_u32(self.hash);
    }
}

/// Everything besides its text that a direct eval's compilation reads of its caller: two
/// direct evals of one text with equal scopes compile to the same program.
#[derive(PartialEq, Eq, Hash)]
pub(crate) struct DirectEvalKey {
    pub(crate) source: EvalSource,
    pub(crate) strict: bool,
    pub(crate) context: crate::compile::EvalContext,
    pub(crate) private_names: Vec<(String, String)>,
    pub(crate) annex_b_forbidden_names: Vec<String>,
    pub(crate) caller: EvalCaller,
}
