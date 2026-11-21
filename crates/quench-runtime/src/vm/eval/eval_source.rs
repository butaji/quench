//! What an eval's program is compiled with and keeps of its source.
use super::*;

/// The storage of an eval's string `value` that its program may keep in place of a copy of
/// the text it compiled (`Vm::keep_eval_text`).
pub(super) struct SharedText {
    value: Value,
    /// The string as an unflattened ASCII rope, which the program keeps as it is when it
    /// compiled the rope's whole text (`SourceText::Rope`).
    rope: Option<crate::bytecode::RopeText>,
    /// The string's shared ASCII storage and the bytes of it the string covers.
    ascii: Option<(std::rc::Rc<str>, std::ops::Range<usize>)>,
}

impl SharedText {
    /// The string's ASCII storage, when the string covers all of it: the text a program
    /// compiled from the string may keep as it is (`Engine::specialize_eval_with_context`).
    pub(super) fn whole_ascii(&self) -> Option<std::rc::Rc<str>> {
        let (storage, range) = self.ascii.as_ref()?;
        (*range == (0..storage.len())).then(|| storage.clone())
    }
}

/// What the program compiled from `text`, all of the eval string `source` (the heap string
/// `value`), may keep of that string's storage instead of its own copy.
pub(super) fn shared_text(value: Value, source: &JsString, text: &str) -> Option<SharedText> {
    if let Some(rope) = crate::bytecode::RopeText::of(source) {
        return (source.len() == text.len()).then_some(SharedText { value, rope: Some(rope), ascii: None });
    }
    let ascii = source.existing_flat().and_then(crate::vm::wtf16::FlatUnits::shared_ascii)?;
    (ascii.1.len() == text.len()).then(|| SharedText { value, rope: None, ascii: Some((ascii.0.clone(), ascii.1)) })
}

/// Longest compiler wrapper around an eval's text (a method eval's strict directive and
/// method head, `Engine::specialize_eval_with_context`) whose text `keep_eval_text` finds.
const WRAPPER_BYTES: usize = 64;

/// Where `body` sits in `text`, a compiler wrapper of at most `WRAPPER_BYTES` around it.
fn wrapped_offset(text: &str, body: &str) -> Option<usize> {
    let extra = text.len().checked_sub(body.len()).filter(|extra| *extra <= WRAPPER_BYTES)?;
    let head = &body.as_bytes()[..body.len().min(WRAPPER_BYTES)];
    (0..=extra).find(|&start| text.as_bytes()[start..].starts_with(head) && text.get(start..start + body.len()) == Some(body))
}

impl Vm {
    /// Makes `residual`, compiled from all of an eval string's text, and that string keep one
    /// buffer, as V8's Script keeps its source string (inspector code-load: 2 MB per eval;
    /// babylonjs-startup: 8.5 MB per iteration, each program kept to the end of the job by a
    /// WeakRef target). The program keeps the string's storage when it compiled exactly that
    /// text (a rope by length: comparing would make it flat); a program that compiled the text
    /// inside a wrapper (an eval in a method) keeps its own text, and the string, with the
    /// eval caches' copies of it in `key` and `cached`, views the text inside it instead.
    pub(super) fn keep_eval_text(&mut self, residual: &mut ResidualProgram, shared: Option<SharedText>, key: Option<&mut DirectEvalKey>, cached: Option<&mut EvalSource>) {
        let (Some(shared), Some(source)) = (shared, residual.source.as_mut()) else { return };
        if source.base != 0 {
            return;
        }
        if let Some(rope) = shared.rope {
            if source.text.len() == rope.len_units() {
                source.text = crate::bytecode::SourceText::Rope(rope);
            }
            return;
        }
        let Some((storage, range)) = shared.ascii else { return };
        if *source.text == *storage {
            source.text = crate::bytecode::SourceText::Local(storage);
            return;
        }
        let crate::bytecode::SourceText::Local(own) = &source.text else { return };
        let Some(start) = (range == (0..storage.len())).then(|| wrapped_offset(own, &storage)).flatten() else { return };
        let view = JsString::from_shared_text(own).substring(start..start + storage.len());
        if self.heap.share_string_storage(shared.value, view.clone()) {
            key.into_iter().map(|key| &mut key.source).chain(cached).for_each(|source| source.text = view.clone());
        }
    }
}

/// Whether the compiled eval may serve a later eval of the same text: sloppy and without
/// tagged templates, whose site identities are per evaluation.
pub(super) fn is_cacheable(residual: &ResidualProgram, source: &str) -> bool {
    residual.functions.first().is_some_and(|root| !root.strict)
        && residual.replay.as_ref().map_or_else(|| !crate::Engine::eval_has_tagged_template(source), |replay| replay.template_sites == 0)
}

/// Whether the compiled strict eval's code may serve a later strict eval of the same text, as
/// a program of its own. Tagged templates do not matter: a program's template objects are
/// its own (the realm keys them by program, function and site).
pub(super) fn is_strict_code_cacheable(residual: &ResidualProgram) -> bool {
    residual.functions.first().is_some_and(|root| root.strict)
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

impl Vm {
    /// The caller facts of the eval of `source`; a direct eval of a cached text also gets its
    /// cache key.
    pub(super) fn eval_caller_facts(&mut self, p: &ResidualProgram, source: &str, strict: bool, text: Option<&EvalSource>) -> Result<CallerFacts, JsError> {
        let context = self.direct_eval_context(p).unwrap_or_default();
        let private_names = if self.direct_eval {
            self.direct_eval_private_names(p).map_or_else(Vec::new, |(_, names)| names)
        } else {
            Vec::new()
        };
        // Eval code that can name private names is shared by every caller of its key (the
        // direct eval cache), whatever class evaluation made the caller: its sites may meet
        // more than one home, so the names it sees are shared (`PrivateClasses::share`).
        self.share_eval_private_names(&private_names);
        let caller_has_global_variables = self.eval_caller().is_none() || self.running_global_code();
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
                caller: self.eval_caller_identity(p)?,
            }),
            None => None,
        };
        Ok(CallerFacts { context, private_names, annex_b_forbidden_names, key })
    }

    /// The host's kept compilation of the script `source`, which compiles from its text alone:
    /// it is no direct eval, so it has no caller context, private names or Annex B restrictions.
    pub(super) fn kept_host_script(&mut self, source_name: &str, mode: &str, source: &str) -> Option<ResidualProgram> {
        let entry = self.host.kept_compiled_program(source_name, &[mode, source])?;
        // An entry that keeps its source (`append_kept_source`) is read in place, as a
        // compiled host script's is; any other holds none, and the program keeps one copy.
        let (bytes, text) = crate::bytecode::split_kept_source(entry, source.len())
            .unwrap_or_else(|entry| (entry, crate::bytecode::SourceText::Shared(std::sync::Arc::from(source))));
        let specialized = self.specialized;
        self.decode_cached_program(bytes, text, 0, source_name, specialized).ok()
    }

    /// The caller identity the eval's program is installed with.
    pub(super) fn eval_caller_identity(&mut self, p: &ResidualProgram) -> Result<EvalCaller, JsError> {
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
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct DirectEvalKey {
    pub(crate) source: EvalSource,
    pub(crate) strict: bool,
    pub(crate) context: crate::compile::EvalContext,
    pub(crate) private_names: Vec<(String, String)>,
    pub(crate) annex_b_forbidden_names: Vec<String>,
    pub(crate) caller: EvalCaller,
}
