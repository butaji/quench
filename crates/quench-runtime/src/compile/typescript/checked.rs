//! Checked parameter types: what a TypeScript checker says each function's parameters are
//! (aliases, enums, contextual callback parameters, inferred types the syntax alone does not
//! name). They join the declared parameter facts (`declared_parameters`), so a checked type is
//! a seed like a declared one: tiers check it once at entry and fall back exactly when it
//! does not hold, so a stale or wrong answer costs a fallback, never a wrong result.
//!
//! The host asks a checker about the sites `parameter_sites` names and registers the answer
//! for the exact text it compiles; every compile of that text (load, replay, reparse) finds it.
//! Lookup is by text length, then a hash that only narrows the candidates, then an exact text
//! comparison, so a hash collision between two texts never gives one text the other's types.

use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use oxc_span::GetSpan;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::FxHashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

/// Checked language types by function: the parameter list's start offset, then one
/// `typed_ir::Type::language` set per formal parameter (`DECLARED_ANY` where unknown).
#[derive(Debug, Default)]
pub struct CheckedParameters {
    by_formals: FxHashMap<u32, Box<[u16]>>,
}

impl CheckedParameters {
    pub fn insert(&mut self, formals_start: u32, languages: Box<[u16]>) {
        self.by_formals.insert(formals_start, languages);
    }

    pub(in crate::compile) fn get(&self, formals_start: u32) -> Option<&[u16]> {
        self.by_formals.get(&formals_start).map(|languages| &languages[..])
    }
}

/// A function whose parameter types a checker can answer: its parameter list's start (the
/// key of `CheckedParameters`) and each formal parameter's binding start, as UTF-8 offsets.
#[derive(Debug, PartialEq, Eq)]
pub struct ParameterSite {
    pub formals_start: u32,
    pub parameters: Vec<u32>,
}

static ANY_REGISTERED: AtomicBool = AtomicBool::new(false);
/// One registered text: its hash (a pre-filter only), the text itself (what a match is decided
/// by), and the checker's answer for it.
struct Entry {
    hash: u64,
    text: Arc<str>,
    checked: Arc<CheckedParameters>,
}
/// By text length, then by text hash, then by text itself: a compile of a text of another length
/// hashes nothing, and a hash collision never hands one text's answer to another text.
static REGISTRY: LazyLock<RwLock<FxHashMap<usize, Vec<Entry>>>> = LazyLock::new(Default::default);

fn hash(text: &str) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Register `checked` for `text`, the exact source a later compile receives. Replaces the answer
/// for an equal text only; a different text that happens to share the hash keeps its own entry.
pub fn register(text: &str, checked: CheckedParameters) {
    let text = super::super::early::normalize_hashbang(text);
    let hash = hash(&text);
    let mut registry = REGISTRY.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entries = registry.entry(text.len()).or_default();
    let checked = Arc::new(checked);
    match entries.iter_mut().find(|entry| entry.hash == hash && *entry.text == *text) {
        Some(entry) => entry.checked = checked,
        None => entries.push(Entry { hash, text: Arc::from(&*text), checked }),
    }
    ANY_REGISTERED.store(true, Ordering::Release);
}

/// The checked parameters registered for `text` (hashbang-normalized, as compiles see it). The
/// hash only narrows the bucket; the text must be equal for an entry to match.
pub(in crate::compile) fn lookup(text: &str) -> Option<Arc<CheckedParameters>> {
    if !ANY_REGISTERED.load(Ordering::Acquire) {
        return None;
    }
    let registry = REGISTRY.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entries = registry.get(&text.len())?;
    let hash = hash(text);
    entries
        .iter()
        .find(|entry| entry.hash == hash && &*entry.text == text)
        .map(|entry| entry.checked.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(language: u16) -> CheckedParameters {
        let mut checked = CheckedParameters::default();
        checked.insert(0, vec![language].into());
        checked
    }

    #[test]
    fn lookup_finds_the_registered_text_and_not_a_same_length_stranger() {
        let text = "function checkedTextLookup(a) { return a; }";
        let stranger = "function checkedTextLookup(b) { return b; }";
        assert_eq!(text.len(), stranger.len());
        register(text, answer(1));
        assert_eq!(lookup(text).unwrap().get(0), Some(&[1u16][..]));
        assert!(lookup(stranger).is_none());
    }

    #[test]
    fn register_replaces_an_equal_text_and_keeps_a_different_one_of_the_same_length() {
        let text = "function checkedTextReplace(a) { return a; }";
        let other = "function checkedTextReplace(b) { return b; }";
        register(text, answer(1));
        register(other, answer(2));
        register(text, answer(3));
        assert_eq!(lookup(text).unwrap().get(0), Some(&[3u16][..]));
        assert_eq!(lookup(other).unwrap().get(0), Some(&[2u16][..]));
    }
}

/// Every function and arrow of `program` with at least one formal parameter.
pub(in crate::compile) fn parameter_sites(program: &Program<'_>) -> Vec<ParameterSite> {
    #[derive(Default)]
    struct Sites(Vec<ParameterSite>);
    impl<'a> Visit<'a> for Sites {
        fn visit_formal_parameters(&mut self, formals: &FormalParameters<'a>) {
            if !formals.items.is_empty() {
                self.0.push(ParameterSite {
                    formals_start: formals.span.start,
                    parameters: formals.items.iter().map(|item| item.pattern.span().start).collect(),
                });
            }
            walk::walk_formal_parameters(self, formals);
        }
        // Types and interfaces have parameter lists but no runtime function.
        fn visit_ts_type(&mut self, _: &TSType<'a>) {}
        fn visit_ts_interface_declaration(&mut self, _: &TSInterfaceDeclaration<'a>) {}
        fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
            // A body-less signature (overload, declare) has no runtime function to inform.
            if function.body.is_some() {
                walk::walk_function(self, function, flags);
            }
        }
    }
    let mut sites = Sites::default();
    sites.visit_program(program);
    sites.0
}
