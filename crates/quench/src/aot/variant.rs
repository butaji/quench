//! Support variants: the program's demands pick the support archive an image links against
//! (`NativeImageSupport`: the full archive keeps the interpreter and JIT for dynamic source,
//! the static one drops them; both keep the Wasm frontend and the Node host), and a demand the
//! chosen support cannot serve is refused. A strict compile (`aot::strict`) links none when the
//! program folds, and one of these when it reads a dynamic input.

use super::{DependencyGraph, GraphRequirement};
use quench_runtime::{NativeImageSupport, SourceKind};

/// Support tiers, from most capable to least: `Full` and `Static` link the matching
/// `NativeImageSupport` archive; `Core` executes with static support on the runtime's own host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Interpreter and JIT for dynamic source, the Wasm frontend and the Node host.
    Full,
    /// No interpreter or JIT; the Wasm frontend and the Node host remain.
    Static,
    /// Standard ECMAScript and the runtime's own `console`, on `SystemHost`: no Node host, no
    /// bootstrap snapshot, no operation table. The smallest executable an image can be.
    Core,
}

impl Variant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Static => "static",
            Self::Core => "core",
        }
    }

    /// The runtime support an image of this variant executes with.
    pub const fn image_support(self) -> NativeImageSupport {
        match self {
            Self::Full => NativeImageSupport::DynamicSource,
            Self::Static | Self::Core => NativeImageSupport::Static,
        }
    }
}

/// The variant a compile links: the explicit support choice and the demands decide, where a
/// dynamic load counts as a demand for full support. A program that touches no host global,
/// imports nothing and has one script entry takes the pure-ECMAScript core.
pub fn select(explicit: Option<NativeImageSupport>, graph: &DependencyGraph) -> Variant {
    let dynamic_load = graph
        .requirements
        .iter()
        .any(GraphRequirement::is_dynamic_load);
    match NativeImageSupport::select(explicit, graph.requires_dynamic_source || dynamic_load) {
        NativeImageSupport::DynamicSource => Variant::Full,
        NativeImageSupport::Static if core_eligible(graph) => Variant::Core,
        NativeImageSupport::Static => Variant::Static,
    }
}

/// Whether the program is a closed, single-file ECMAScript program that runs on the runtime's
/// own host: no module or package import, no dynamic load, no Node or web host global.
pub fn core_eligible(graph: &DependencyGraph) -> bool {
    closed(graph) && pure_ecmascript(graph) && single_entry(graph)
}

/// No module, package, addon or dynamic load reaches beyond the entry.
fn closed(graph: &DependencyGraph) -> bool {
    !graph.requires_dynamic_source
        && graph.requirements.is_empty()
        && graph.resolutions.is_empty()
        && graph.builtins.iter().all(|name| name == "node:console")
        && graph.esm_builtins.is_empty()
}

/// Every global the source reads is ECMAScript or the runtime's own `console`.
fn pure_ecmascript(graph: &DependencyGraph) -> bool {
    graph.host_globals.is_empty()
        && graph.host_only_globals.is_empty()
        && graph.dynamic_eval_sites.is_empty()
        && graph.dynamic_function_sites.is_empty()
        && graph.dynamic_reflective_sites.is_empty()
}

/// One JavaScript entry file and nothing else.
fn single_entry(graph: &DependencyGraph) -> bool {
    graph.files.len() == 1
        && graph
            .files
            .values()
            .all(|file| matches!(file.kind, SourceKind::Script | SourceKind::Module))
}

/// Whether a requirement blocks a compile with this support. Dynamic source support loads
/// dynamic requires and imports; every other unresolved requirement still blocks.
pub fn blocks(support: NativeImageSupport, requirement: &GraphRequirement) -> bool {
    !(support.allows_dynamic_source() && requirement.is_dynamic_load())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::SourceManifest;
    use std::fs;

    fn select_for(source: &str) -> Variant {
        let directory = std::env::temp_dir().join(format!(
            "quench-core-variant-{}-{}",
            std::process::id(),
            source.len()
        ));
        fs::create_dir_all(&directory).unwrap();
        let entry = directory.join("entry.cjs");
        fs::write(&entry, source).unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        let variant = select(None, &graph);
        fs::remove_dir_all(directory).unwrap();
        variant
    }

    #[test]
    fn closed_ecmascript_takes_the_core_host() {
        assert_eq!(select_for("console.log('hello');"), Variant::Core);
        // A shadowed host global is the program's own binding.
        assert_eq!(
            select_for("function read(process) { return process; } console.log(read(1));"),
            Variant::Core
        );
    }

    #[test]
    fn node_and_web_host_demands_keep_the_node_host() {
        for source in [
            "console.log(process.argv.length);",
            "console.log(typeof Buffer);",
            "console.log(typeof URL);",
            "console.log(typeof setTimeout);",
            "console.log(typeof structuredClone);",
            "require('node:fs');",
            "eval('1');",
            "new Function('return 1');",
        ] {
            assert_ne!(select_for(source), Variant::Core, "{source}");
        }
    }

    #[test]
    fn explicit_dynamic_source_and_dynamic_loads_stay_full() {
        let directory =
            std::env::temp_dir().join(format!("quench-core-variant-full-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let entry = directory.join("entry.cjs");
        fs::write(&entry, "const load = require; load(process.argv[2]);").unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        assert_eq!(select(None, &graph), Variant::Full);
        assert_eq!(
            select(Some(NativeImageSupport::DynamicSource), &graph),
            Variant::Full
        );
        fs::remove_dir_all(directory).unwrap();
    }
}
