//! Support variants: the program's demands pick the support archive an image links against
//! (`NativeImageSupport`: the full archive keeps the interpreter and JIT for dynamic source,
//! the static one drops them; both keep the Wasm frontend and the Node host), and a demand the
//! chosen support cannot serve is refused. A strict compile (`aot::strict`) links none when the
//! program folds, and one of these when it reads a dynamic input.

use super::{DependencyGraph, GraphRequirement};
use quench_runtime::NativeImageSupport;

/// The report name of a support archive.
pub const fn name(support: NativeImageSupport) -> &'static str {
    match support {
        NativeImageSupport::DynamicSource => "full",
        NativeImageSupport::Static => "static",
    }
}

/// The support a compile links: the explicit support choice and the demands decide, where a
/// dynamic load counts as a demand for dynamic source.
pub fn select(explicit: Option<NativeImageSupport>, graph: &DependencyGraph) -> NativeImageSupport {
    let dynamic_load = graph.requirements.iter().any(GraphRequirement::is_dynamic_load);
    NativeImageSupport::select(explicit, graph.requires_dynamic_source || dynamic_load)
}

/// Whether a requirement blocks a compile with this support. Dynamic source support loads
/// dynamic requires and imports; every other unresolved requirement still blocks.
pub fn blocks(support: NativeImageSupport, requirement: &GraphRequirement) -> bool {
    !(support.allows_dynamic_source() && requirement.is_dynamic_load())
}
