//! The support build an image links: its feature set, and a core image's intrinsic families.

use super::{CompiledSources, DependencyGraph};

/// The features a static image's support is built with: `native-execution`, plus the intl layer
/// and the WebAssembly frontend only where the program reaches them. A dynamic-source support
/// links the standard build's default features instead.
pub(super) fn features(graph: &DependencyGraph, support: quench_runtime::NativeImageSupport) -> String {
    if support.allows_dynamic_source() { return "default".into(); }
    let mut features = String::from("native-execution");
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
