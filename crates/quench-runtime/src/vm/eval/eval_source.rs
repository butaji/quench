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

impl<H: Host> Vm<H> {
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
