//! Checked parameter types: what a TypeScript checker says each function's parameters are
//! (aliases, enums, contextual callback parameters, inferred types the syntax alone does not
//! name). They join the declared parameter facts (`declared_parameters`), so a checked type is
//! a seed like a declared one: tiers check it once at entry and fall back exactly when it
//! does not hold, so a stale or wrong answer costs a fallback, never a wrong result.
//!
//! The host asks a checker about the sites `parameter_sites` names and registers the answer
//! for the exact text it compiles; every compile of that text (load, replay, reparse) finds it.

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
/// By text length, then text hash: a compile of a text of another length hashes nothing.
static REGISTRY: LazyLock<RwLock<FxHashMap<usize, Vec<(u64, Arc<CheckedParameters>)>>>> = LazyLock::new(Default::default);

fn hash(text: &str) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    text.hash(&mut hasher);
    hasher.finish()
}

/// Register `checked` for `text`, the exact source a later compile receives.
pub fn register(text: &str, checked: CheckedParameters) {
    let text = super::super::early::normalize_hashbang(text);
    let hash = hash(&text);
    let mut registry = REGISTRY.write().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entries = registry.entry(text.len()).or_default();
    entries.retain(|(registered, _)| *registered != hash);
    entries.push((hash, Arc::new(checked)));
    ANY_REGISTERED.store(true, Ordering::Release);
}

/// The checked parameters registered for `text` (hashbang-normalized, as compiles see it).
pub(in crate::compile) fn lookup(text: &str) -> Option<Arc<CheckedParameters>> {
    if !ANY_REGISTERED.load(Ordering::Acquire) {
        return None;
    }
    let registry = REGISTRY.read().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entries = registry.get(&text.len())?;
    let hash = hash(text);
    entries.iter().find(|(registered, _)| *registered == hash).map(|(_, checked)| checked.clone())
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
