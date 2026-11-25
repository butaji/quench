//! Support variants: the program's demands pick the support archive an image
//! links against, and a demand the chosen variant cannot serve is refused.

use super::{DependencyGraph, GraphRequirement};
use quench_runtime::NativeImageSupport;

/// Support archives, from most capable to least. A strict compile (`aot::strict`) links none
/// when the program folds, and one of these when it reads a dynamic input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Interpreter and JIT for dynamic source, the Wasm frontend and the Node host.
    Full,
    /// No interpreter or JIT; the Wasm frontend and the Node host remain.
    Static,
}

impl Variant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Static => "static",
        }
    }

    /// The runtime support an image of this variant executes with.
    pub const fn image_support(self) -> NativeImageSupport {
        match self {
            Self::Full => NativeImageSupport::DynamicSource,
            Self::Static => NativeImageSupport::Static,
        }
    }
}

/// The variant a compile links: the explicit support choice and the demands decide, where a
/// dynamic load counts as a demand for full support.
pub fn select(explicit: Option<NativeImageSupport>, graph: &DependencyGraph) -> Variant {
    let dynamic_load = graph.requirements.iter().any(GraphRequirement::is_dynamic_load);
    match NativeImageSupport::select(explicit, graph.requires_dynamic_source || dynamic_load) {
        NativeImageSupport::DynamicSource => Variant::Full,
        NativeImageSupport::Static => Variant::Static,
    }
}

/// Whether a requirement blocks a compile with this variant. Full support loads
/// dynamic requires and imports; every other unresolved requirement still blocks.
pub fn blocks(variant: Variant, requirement: &GraphRequirement) -> bool {
    !(variant == Variant::Full && requirement.is_dynamic_load())
}
