//! The support build an image links: its feature set, and a core image's intrinsic families.

use super::{variant::Variant, CompiledSources, DependencyGraph};

/// The features an image's support is built with. Full support links the standard build's
/// default features (the Node host, the JIT and the bundled sources). Every core image is
/// execution-only: `native-execution`, the source frontend where the program can make code,
/// and the intl layer and Wasm frontend only where the program reaches them. No core image
/// links the JIT, so its binary carries no Cranelift and compiles nothing while it runs.
pub(super) fn features(graph: &DependencyGraph, variant: Variant) -> String {
    if variant == Variant::Full { return "default".into(); }
    let mut features = String::from("native-execution");
    if variant.has_source_frontend() { features.push_str(",source"); }
    if graph.needs_intl() { features.push_str(",intl"); }
    if graph.needs_wasm() { features.push_str(",wasm"); }
    features
}

/// The intrinsic families a core image's realm installs: every family where the program can make
/// code (`Variant::CoreDynamic`), else exactly the families its closed program reaches. A Node
/// image installs every family.
pub(super) fn builtin_families(core: bool, dynamic: bool, compiled: &CompiledSources) -> quench_runtime::BuiltinFamilies {
    if !core || dynamic { quench_runtime::BuiltinFamilies::ALL } else { compiled.builtin_families() }
}
