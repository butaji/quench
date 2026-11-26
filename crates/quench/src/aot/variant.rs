//! Support variants: the program's demands pick the support archive an image
//! links against, and a demand the chosen variant cannot serve is refused.

use super::{DependencyGraph, GraphRequirement};
use quench_runtime::{NativeImageSupport, SourceKind};

/// Support tiers, from most capable to least. A strict compile (`aot::strict`) links none
/// when the program folds, and one of these when it reads a dynamic input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    /// Interpreter and JIT for dynamic source, the Wasm frontend and the Node host.
    Full,
    /// No interpreter or JIT; the Wasm frontend and the Node host remain.
    Static,
    /// Standard ECMAScript on `SystemHost`, with the source frontend for `eval` and the
    /// `Function` constructor: no Node host, bootstrap snapshot or operation table. A program
    /// that makes code but touches no host API (a V8-v7 benchmark) takes the core host and keeps
    /// the interpreter.
    CoreDynamic,
    /// Standard ECMAScript and the runtime's own `console`, on `SystemHost`: no Node host, no
    /// bootstrap snapshot, no operation table, no source frontend. The smallest executable an
    /// image can be.
    Core,
}

impl Variant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Static => "static",
            Self::CoreDynamic => "core-dynamic",
            Self::Core => "core",
        }
    }

    /// Whether the image executes on the runtime's own host rather than the Node host.
    pub const fn runs_on_core(self) -> bool {
        matches!(self, Self::CoreDynamic | Self::Core)
    }

    /// The runtime support an image of this variant executes with.
    pub const fn image_support(self) -> NativeImageSupport {
        match self {
            Self::Full | Self::CoreDynamic => NativeImageSupport::DynamicSource,
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
    // An explicit choice wins. Without one, a program that makes code keeps the interpreter,
    // on the Node host only when it can reach one (`process`, a module, a dynamic load).
    match explicit {
        Some(NativeImageSupport::DynamicSource) => Variant::Full,
        Some(NativeImageSupport::Static) if core_static_eligible(graph) => Variant::Core,
        Some(NativeImageSupport::Static) => Variant::Static,
        None if graph.requires_dynamic_source || dynamic_load => {
            if core_eligible(graph) { Variant::CoreDynamic } else { Variant::Full }
        }
        None if core_eligible(graph) => Variant::Core,
        None => Variant::Static,
    }
}

/// Whether the program is a closed, single-file ECMAScript program that runs on the runtime's
/// own host: no module or package import, no dynamic load, no Node or web host global.
pub fn core_eligible(graph: &DependencyGraph) -> bool {
    closed(graph) && pure_ecmascript(graph) && single_entry(graph)
}

/// A closed program that makes no code: the source frontend stays out of its image.
fn core_static_eligible(graph: &DependencyGraph) -> bool {
    core_eligible(graph) && !graph.requires_dynamic_source
}

/// No module, package, addon or dynamic load reaches beyond the entry.
fn closed(graph: &DependencyGraph) -> bool {
    graph.requirements.is_empty()
        && graph.resolutions.is_empty()
        && graph.builtins.iter().all(|name| name == "node:console")
        && graph.esm_builtins.is_empty()
}

/// Every global the source reads is ECMAScript or the runtime's own `console`. Code the source
/// makes at run time (`eval`, the `Function` constructor, a computed key that may name one) is
/// ECMAScript too; it only decides which core variant runs it.
fn pure_ecmascript(graph: &DependencyGraph) -> bool {
    graph.host_globals.is_empty() && graph.host_only_globals.is_empty()
}

/// One JavaScript entry file and nothing else.
fn single_entry(graph: &DependencyGraph) -> bool {
    graph.files.len() == 1
        && graph
            .files
            .values()
            .all(|file| matches!(file.kind, SourceKind::Script | SourceKind::Module))
}

/// Whether a requirement blocks a compile with this variant. Full support loads
/// dynamic requires and imports; every other unresolved requirement still blocks.
pub fn blocks(variant: Variant, requirement: &GraphRequirement) -> bool {
    !(variant == Variant::Full && requirement.is_dynamic_load())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::aot::SourceManifest;
    use std::fs;

    fn select_for(source: &str) -> Variant {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "quench-core-variant-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
        ] {
            assert_eq!(select_for(source), Variant::Static, "{source}");
        }
    }

    #[test]
    fn dynamic_source_without_a_host_global_takes_the_core_host() {
        for source in [
            "console.log(eval('1 + 1'));",
            "console.log(new Function('return 1')());",
            "const key = Math.random() > 2 ? 'a' : 'b'; console.log(({a: 1, b: 2})[key]);",
        ] {
            assert_eq!(select_for(source), Variant::CoreDynamic, "{source}");
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
