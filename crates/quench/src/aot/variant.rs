//! Support variants: the program's demands pick the support archive an image
//! links against, and a demand the chosen variant cannot serve is refused.

use super::{DependencyGraph, GraphRequirement};
use quench_runtime::NativeImageSupport;
use std::path::{Path, PathBuf};

/// Support archives, from most capable to least.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Interpreter and JIT for dynamic source, the Wasm frontend and the Node host.
    Full,
    /// No interpreter or JIT; the Wasm frontend and the Node host remain.
    Static,
    /// Static without the Wasm frontend or the Node host modules a graph does not reach.
    Strict,
}

impl Variant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Static => "static",
            Self::Strict => "strict",
        }
    }

    /// The archive file name `cargo build` produces for this variant.
    pub fn archive_name(self) -> String {
        format!("libquench_aot_support_{}.a", self.name())
    }

    /// The runtime support an image of this variant executes with.
    pub const fn image_support(self) -> NativeImageSupport {
        match self {
            Self::Full => NativeImageSupport::DynamicSource,
            Self::Static | Self::Strict => NativeImageSupport::Static,
        }
    }
}

/// The variant a compile links. `strict` refuses every demand that receives source at
/// run time; otherwise the explicit support choice and the demands decide, where a
/// dynamic load counts as a demand for full support.
pub fn select(strict: bool, explicit: Option<NativeImageSupport>, graph: &DependencyGraph) -> Result<Variant, String> {
    if strict {
        return match source_demand(graph) {
            Some(demand) => Err(format!("strict AOT refuses {demand}: strict support has no interpreter or JIT")),
            None => Ok(Variant::Strict),
        };
    }
    let dynamic_load = graph.requirements.iter().any(GraphRequirement::is_dynamic_load);
    match NativeImageSupport::select(explicit, graph.requires_dynamic_source || dynamic_load) {
        NativeImageSupport::DynamicSource => Ok(Variant::Full),
        NativeImageSupport::Static => Ok(Variant::Static),
    }
}

/// Whether a requirement blocks a compile with this variant. Full support loads
/// dynamic requires and imports; every other unresolved requirement still blocks.
pub fn blocks(variant: Variant, requirement: &GraphRequirement) -> bool {
    !(variant == Variant::Full && requirement.is_dynamic_load())
}

/// The variant's archive in `support_dir`, or beside the compiler executable.
pub fn archive_path(variant: Variant, support_dir: Option<&Path>) -> Result<PathBuf, String> {
    let directory = match support_dir {
        Some(directory) => directory.to_path_buf(),
        None => std::env::current_exe()
            .map_err(|error| error.to_string())?
            .parent()
            .ok_or("compiler executable has no directory")?
            .to_path_buf(),
    };
    let path = directory.join(variant.archive_name());
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("support archive {} is absent: looked in {}", variant.archive_name(), path.display()))
    }
}

/// Until the strict archive has a link path, a strict compile names its archive and stops.
pub fn require_linkable(variant: Variant, support_dir: Option<&Path>) -> Result<(), String> {
    if variant != Variant::Strict {
        return Ok(());
    }
    archive_path(variant, support_dir)?;
    Err("strict AOT link is not yet implemented: the strict archive has no cc link path".into())
}

/// The first demand that may receive source at run time, named for a refusal.
fn source_demand(graph: &DependencyGraph) -> Option<String> {
    let kinds = [
        ("eval", &graph.dynamic_eval_sites),
        ("Function constructor", &graph.dynamic_function_sites),
        ("reflective access", &graph.dynamic_reflective_sites),
    ];
    kinds
        .into_iter()
        .find_map(|(kind, sites)| sites.first().map(|(file, offset)| format!("{kind} at {}:{offset}", file.display())))
        .or_else(|| graph.requirements.iter().find_map(GraphRequirement::dynamic_load_demand))
}
