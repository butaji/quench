//! The hot-path cost model: an operation on a hot path does O(1) work and allocates nothing.
//!
//! Every function in a [`HOT_MODULES`] file (outside `#[cfg(test)]`) is counted for:
//! - `hot-hash`: a hashed or tree map/set (lookups belong in flat arrays indexed by a proof);
//! - `hot-scan`: a linear search (`find`, `position`, `any`, `all`, ...) per operation;
//! - `hot-alloc`: a heap allocation (`Vec`, `Box`, `String`, `format!`, `collect`, ...).
//!
//! The counts join the shrink-only measures in `oversized.txt`: a new use fails, an old one
//! may only go away.

use syn::visit::Visit;

use crate::shape::Measures;

/// Files whose every function runs per JavaScript operation: the interpreters, property
/// access, calls and the staged layout reads the JIT derives from.
const HOT_MODULES: &[&str] = &[
    "crates/quench-runtime/src/staged.rs",
    "crates/quench-runtime/src/vm/dispatch",
    "crates/quench-runtime/src/vm/activation.rs",
    "crates/quench-runtime/src/vm/call_arguments.rs",
    "crates/quench-runtime/src/vm/environment.rs",
    "crates/quench-runtime/src/vm/field_cache.rs",
    "crates/quench-runtime/src/vm/method_cache",
    "crates/quench-runtime/src/vm/object_get.rs",
    "crates/quench-runtime/src/vm/operations.rs",
    "crates/quench-runtime/src/vm/property_key.rs",
    "crates/quench-runtime/src/vm/shape_transitions.rs",
];

const HASHED: &[&str] = &["HashMap", "HashTable", "HashSet", "BTreeMap", "BTreeSet", "FxHashMap", "FxHashSet", "IndexMap"];
const SCANS: &[&str] = &["find", "find_map", "position", "rposition", "rfind", "any", "all"];
const ALLOCATING_METHODS: &[&str] = &["to_string", "to_owned", "to_vec", "collect", "into_boxed_slice"];
const ALLOCATING_MACROS: &[&str] = &["format", "vec"];
const ALLOCATING_TYPES: &[&str] = &["Vec", "Box", "String", "Rc", "Arc"];
const CONSTRUCTORS: &[&str] = &["new", "with_capacity", "from"];

pub fn is_hot(file: &str) -> bool {
    HOT_MODULES.iter().any(|module| file.contains(module))
}

/// Records, by `hot-<rule> subject`, how often a hot function breaks each rule.
pub fn scan(subject: &str, body: &syn::Block, measures: &mut Measures) {
    let mut costs = Costs::default();
    costs.visit_block(body);
    for (rule, count) in [("hot-hash", costs.hash), ("hot-scan", costs.scan), ("hot-alloc", costs.alloc)] {
        if count > 0 {
            measures.insert(format!("{rule} {subject}"), count);
        }
    }
}

#[derive(Default)]
struct Costs {
    hash: usize,
    scan: usize,
    alloc: usize,
}

fn named(list: &[&str], ident: &syn::Ident) -> bool {
    list.iter().any(|name| ident == name)
}

impl<'ast> Visit<'ast> for Costs {
    fn visit_path(&mut self, path: &'ast syn::Path) {
        let segments: Vec<_> = path.segments.iter().map(|segment| &segment.ident).collect();
        self.hash += segments.iter().filter(|ident| named(HASHED, ident)).count();
        if let [.., owner, constructor] = segments[..] {
            self.alloc += usize::from(named(ALLOCATING_TYPES, owner) && named(CONSTRUCTORS, constructor));
        }
        syn::visit::visit_path(self, path);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.scan += usize::from(named(SCANS, &call.method));
        self.alloc += usize::from(named(ALLOCATING_METHODS, &call.method));
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        self.alloc += usize::from(mac.path.get_ident().is_some_and(|ident| named(ALLOCATING_MACROS, ident)));
    }

    /// A nested function is its own subject.
    fn visit_item(&mut self, _: &'ast syn::Item) {}
}
