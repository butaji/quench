use quench_runtime::native_link::{NativeImageInputs, CATALOG_FILENAME, REQUIRED_EXPORTS_FILENAME};
use super::{DependencyGraph, ModuleAssets, ModuleAssetState, PreparedModuleSource};
use crate::NodeHost;
use quench_runtime::{Engine, NativeJsCompiler, NativeSourceKey, NativeSourceCompilation, NativeTarget, ResidualProgram, Runtime};
use quench_layers::engine::{emit_native_module_for, lower_native_js_module_for};
use std::{cell::RefCell, collections::BTreeSet, fs, io::Write, path::{Path, PathBuf}, rc::Rc};

mod source_pool;
use source_pool::SourcePool;

pub(super) const NATIVE_ENTRY_NAME: &str = "<quench:aot-entry>";
pub(super) const NATIVE_ENTRY_UNIT: &str = "entry";

/// The host an image executes on: the Node host (`quench`) or the runtime's own `SystemHost`.
/// The catalog and driver are the only places the choice appears; the compiled program, the
/// image objects and the link pipeline are one.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ImageHost {
    Node,
    Core,
}

impl ImageHost {
    pub const fn of(core: bool) -> Self {
        if core { Self::Core } else { Self::Node }
    }

    pub const fn crate_path(self) -> &'static str {
        match self {
            Self::Node => "quench",
            Self::Core => "quench_runtime",
        }
    }

    pub const fn host_type(self) -> &'static str {
        match self {
            Self::Node => "quench::NodeHost",
            Self::Core => "quench_runtime::SystemHost",
        }
    }

    /// Whether the catalog binds the program's module assets (only the Node loader reads them).
    pub const fn carries_modules(self) -> bool {
        matches!(self, Self::Node)
    }

    /// Whether the host library exports the lane helpers (the Node host does, from `aot::lane_helper_symbols`).
    pub const fn exports_lane_helpers(self) -> bool {
        matches!(self, Self::Core)
    }
}

/// Original source keys, sorted and deduplicated by full equality. This manifest
/// is compiler input, rather than a list of sources inferred from benchmark names.
#[derive(Default)]
pub struct SourceManifest {
    units: BTreeSet<NativeSourceKey>,
    pub(super) assets: ModuleAssets,
}

pub struct CompiledSource {
    pub key: NativeSourceKey,
    pub program: ResidualProgram,
}

pub struct DeferredSourceSyntaxError {
    pub key: NativeSourceKey,
    pub message: String,
}

pub struct CompiledSources {
    pub entry: ResidualProgram,
    pub units: Vec<CompiledSource>,
    pub syntax_errors: Vec<DeferredSourceSyntaxError>,
    pub assets: ModuleAssets,
}

/// Link inputs written to a fresh compiler-owned directory. Rust macros derive
/// the loader from the same source keys that selected the native compilation.
impl CompiledSources {
    pub fn emit(self, directory: &Path, target: &NativeTarget, host: ImageHost) -> Result<NativeImageInputs, String> {
        fs::create_dir(directory).map_err(|error| error.to_string())?;
        let directory = directory.canonicalize().map_err(|error| error.to_string())?;
        let mut texts = Vec::new();
        for unit in &self.units {
            texts.extend(unit.key.parts());
            texts.extend(unit.program.source_text());
        }
        for error in &self.syntax_errors {
            texts.extend(error.key.parts());
            texts.push(&error.message);
        }
        for file in self.assets.files.values() {
            if let Ok(text) = std::str::from_utf8(&file.bytes) { texts.push(text); }
            if let Some(PreparedModuleSource::Projected(source)) = &file.prepared { texts.push(source.as_ref()); }
        }
        texts.extend(self.assets.views.values().map(|source| source.as_ref()));
        texts.extend(self.assets.modules.values().map(|(source, _)| source.as_ref()));
        let pool = SourcePool::new(texts);
        let pool_path = directory.join("source_pool.utf8");
        fs::write(&pool_path, pool.text()).map_err(|error| error.to_string())?;
        let mut required_exports = BTreeSet::new();
        let mut objects = Vec::new();
        let namespace = quench_runtime::NativeProgramNamespace::from_entry(&self.entry)?;
        let mut write = |program: &ResidualProgram, name: &str, namespace: Option<&quench_runtime::NativeProgramNamespace>| -> Result<(PathBuf, String), String> {
            program.freeze_native_diagnostics();
            let module = lower_native_js_module_for(program, &format!("quench_{name}"), target)?;
            let symbol = module.entry_table_symbol().to_owned();
            for function in &module.functions {
                if let quench_layers::engine::NativeFunction::Import { name, .. } = function {
                    required_exports.insert(name.clone());
                }
            }
            let object = emit_native_module_for(name, module.functions, module.data, target)?;
            let object_path = directory.join(format!("{name}.o"));
            fs::write(&object_path, object).map_err(|error| error.to_string())?;
            objects.push(object_path);
            let metadata = directory.join(format!("{name}.bin"));
            match namespace {
                Some(namespace) => fs::write(&metadata, namespace.encode_with_external_source(program)?).map_err(|error| error.to_string())?,
                None => program.write_binary(&metadata)?,
            }
            Ok((metadata, symbol))
        };
        let (entry, entry_symbol) = write(&self.entry, NATIVE_ENTRY_UNIT, None)?;
        let mut catalog = fs::File::create(directory.join(CATALOG_FILENAME)).map_err(|error| error.to_string())?;
        writeln!(catalog, "{}::linked_source_pool!(quench_native_source_text, {:?});", host.crate_path(), pool_path.to_str().ok_or("source pool path is not UTF-8")?)
            .map_err(|error| error.to_string())?;
        writeln!(catalog, "const _: () = assert!(quench_runtime::ResidualProgram::RUNTIME_ABI_FINGERPRINT == {:#x}, \"native metadata ABI mismatch\");", ResidualProgram::RUNTIME_ABI_FINGERPRINT)
            .map_err(|error| error.to_string())?;
        let sources_macro = match host {
            ImageHost::Node => "quench::linked_native_sources! {".to_owned(),
            ImageHost::Core => "quench_runtime::linked_native_sources! {\nhost quench_runtime::SystemHost;".to_owned(),
        };
        writeln!(catalog, "{sources_macro}\nentry bind_entry: entry_table => ({entry_symbol:?}, {:?});\nresolver resolve {{", entry.to_str().ok_or("native image path is not UTF-8")?)
            .map_err(|error| error.to_string())?;
        for (index, unit) in self.units.iter().enumerate() {
            let (metadata, symbol) = write(&unit.program, &format!("unit_{index}"), Some(&namespace))?;
            let parts = unit.key.parts().map(|part| pool.reference(part)).collect::<Vec<_>>().join(", ");
            let source = unit.program.source_text().map_or_else(|| "None".to_owned(), |text| format!("Some({})", pool.reference(text)));
            writeln!(catalog, "source_{index}_table => ({symbol:?}, {:?}, {:?}, [{parts}], source {source}),", metadata.to_str().ok_or("native image path is not UTF-8")?, unit.key.name())
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} errors {{").map_err(|error| error.to_string())?;
        for error in &self.syntax_errors {
            let parts = error.key.parts().map(|part| pool.reference(part)).collect::<Vec<_>>().join(", ");
            writeln!(catalog, "({:?}, [{parts}]) => {},", error.key.name(), pool.reference(&error.message))
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}}\n}}").map_err(|error| error.to_string())?;
        if host.carries_modules() { write_module_assets(&mut catalog, &directory, &self.assets, &pool)?; }
        fs::write(directory.join(REQUIRED_EXPORTS_FILENAME), required_exports.iter().map(String::as_str).collect::<Vec<_>>().join("\n") + "\n")
            .map_err(|error| error.to_string())?;
        // The image's lane helper imports are defined by this library (`aot::lane_helper_symbols`), and the
        // attention byte its polls read by the runtime (`execution_interrupt`, exported by name).
        let mut exports = required_exports.iter()
            .filter(|name| !name.starts_with("quench_lane_helper_") && *name != "quench_execution_attention")
            .map(|name| format!("{name:?}")).collect::<Vec<_>>();
        // The Node host defines the lane helper trampolines from its own library
        // (`aot::lane_helper_symbols`); a core image has no host library, so its catalog does.
        if host.exports_lane_helpers() { exports.push("\"quench_lane_helper_symbols\"".to_owned()); }
        writeln!(catalog, "quench_runtime::export_native_js_operations!({}, semantic_exports, [{}]);", host.host_type(), exports.join(","))
            .map_err(|error| error.to_string())?;
        let host_exports = match host {
            ImageHost::Node => crate::modules::napi::exported_symbol_names().map(str::to_owned)
                .chain(std::iter::once(crate::host::resident_pages::COMPILED_ENTRY_SYMBOL.to_owned())).collect(),
            ImageHost::Core => std::collections::BTreeSet::new(),
        };
        Ok(NativeImageInputs::new(directory, objects, host_exports))
    }
}

/// Write the Node loader's module assets into the catalog: the entry, files, resolutions,
/// views and linked module sources. A core image has no loader, so it writes none.
fn write_module_assets(catalog: &mut fs::File, directory: &Path, assets: &ModuleAssets, pool: &SourcePool) -> Result<(), String> {
    let entry_path = assets.entry.as_deref().map(|path| path.to_str().ok_or("module entry is not UTF-8")).transpose()?;
    writeln!(catalog, "quench::linked_module_assets! {{ values embedded_modules; entry {entry_path:?}; files {{").map_err(|error| error.to_string())?;
    for (index, (path, file)) in assets.files.iter().enumerate() {
        let bytes = match std::str::from_utf8(&file.bytes) {
            Ok(text) => format!("{}.as_bytes()", pool.reference(text)),
            Err(_) => {
                let bytes_path = directory.join(format!("asset_{index}.bin"));
                fs::write(&bytes_path, file.bytes.as_ref()).map_err(|error| error.to_string())?;
                format!("include_bytes!({:?})", bytes_path.to_str().ok_or("native image path is not UTF-8")?)
            }
        };
        let prepared = match &file.prepared {
            Some(PreparedModuleSource::Original) => "Some(quench::aot::PreparedModuleSource::Original)".to_owned(),
            Some(PreparedModuleSource::Projected(source)) => format!("Some(quench::aot::PreparedModuleSource::Projected({}))", pool.reference(source)),
            None => "None".to_owned(),
        };
        writeln!(catalog, "{:?} => (bytes {bytes}, {:?}, {prepared}),", path.to_str().ok_or("module asset path is not UTF-8")?, file.kind).map_err(|error| error.to_string())?;
    }
    writeln!(catalog, "}} resolutions {{").map_err(|error| error.to_string())?;
    for ((parent, specifier), target) in &assets.resolutions {
        writeln!(catalog, "({:?}, {specifier:?}) => {:?},", parent.to_str().ok_or("module parent is not UTF-8")?, target.to_str().ok_or("module target is not UTF-8")?).map_err(|error| error.to_string())?;
    }
    writeln!(catalog, "}} views {{").map_err(|error| error.to_string())?;
    for ((path, key), source) in &assets.views {
        writeln!(catalog, "({:?}, {key:?}) => source {},", path.to_str().ok_or("SDK view path is not UTF-8")?, pool.reference(source)).map_err(|error| error.to_string())?;
    }
    writeln!(catalog, "}} imports {{").map_err(|error| error.to_string())?;
    for ((referrer, specifier), target) in &assets.imports {
        writeln!(catalog, "({referrer:?}, {specifier:?}) => {target:?},").map_err(|error| error.to_string())?;
    }
    writeln!(catalog, "}} modules {{").map_err(|error| error.to_string())?;
    for (name, (source, typescript)) in &assets.modules {
        writeln!(catalog, "{name:?} => source {}, typescript {typescript},", pool.reference(source)).map_err(|error| error.to_string())?;
    }
    writeln!(catalog, "}} }}").map_err(|error| error.to_string())
}

impl SourceManifest {
    pub fn insert(&mut self, name: &str, parts: &[&str]) -> Result<bool, String> {
        Ok(self.units.insert(NativeSourceKey::new(name, parts)?))
    }

    pub(in crate::aot) fn insert_key(&mut self, key: NativeSourceKey) { self.units.insert(key); }

    pub fn add_module_graph(&mut self, entry: &Path) -> Result<DependencyGraph, String> {
        let graph = super::graph::discover(self, entry)?;
        self.assets.entry = Some(entry.canonicalize().map_err(|error| error.to_string())?);
        Ok(graph)
    }

    pub fn units(&self) -> impl ExactSizeIterator<Item = &NativeSourceKey> { self.units.iter() }
    pub fn assets(&self) -> &ModuleAssets { &self.assets }

    /// Stage only Node's existing pre-entry bootstrap lifecycle. The snapshot
    /// boundary forbids process-state capture and stops before user preloads,
    /// entry loading, IPC activation and event-loop execution.
    pub fn capture_bootstrap() -> Result<Self, String> {
        Self::capture(crate::host::startup_snapshot::SnapshotKind::Bootstrap, Vec::new())
    }

    /// The existing Node snapshot preload set, staged before any application.
    pub fn capture_preloaded() -> Result<Self, String> {
        Self::capture(crate::host::startup_snapshot::SnapshotKind::Preloaded, Vec::new())
    }

    /// Node SDK policies are Rust; only startup inputs can contribute guest source.
    /// Keep this entry point for callers combining bootstrap and application manifests.
    /// The bootstrap with the builtins the graph names loaded, as the image's snapshot
    /// keeps them (`DependencyGraph::snapshot_builtins`).
    pub fn capture_for_graph(graph: &DependencyGraph) -> Result<Self, String> {
        Self::capture(crate::host::startup_snapshot::SnapshotKind::Bootstrap, graph.snapshot_builtins())
    }

    pub fn extend(&mut self, other: Self) -> Result<(), String> {
        self.assets.extend(other.assets)?;
        self.units.extend(other.units);
        Ok(())
    }

    fn capture(stage: crate::host::startup_snapshot::SnapshotKind, builtins: Vec<String>) -> Result<Self, String> {
        let inputs = Rc::new(RefCell::new(Self::default()));
        let captured = inputs.clone();
        let mut host = NodeHost::new(vec!["quench".into()]).with_exec_argv(Vec::new())?
            .with_native_programs(move |name, parts| {
                captured.borrow_mut().insert(name, parts)?;
                Ok(None)
            });
        host.building_snapshot = Some(stage);
        host.snapshot_builtins = builtins;
        let assets = Rc::new(RefCell::new(ModuleAssets::default()));
        host.module_assets = ModuleAssetState::Capture(assets.clone());
        let mut runtime = Runtime::new(host);
        let entry = Engine::specialize("", NATIVE_ENTRY_NAME).map_err(|errors| format!("{errors:?}"))?;
        runtime.execute_deferred_jobs(&entry).map_err(|error| {
            match error.source_location() {
                Some((file, line, column, _)) => format!("{file}:{line}:{column}: {error}"),
                None => error.to_string(),
            }
        })?;
        drop(runtime);
        let mut manifest = Rc::try_unwrap(inputs).map_err(|_| "bootstrap source collector is still borrowed".to_owned())?.into_inner();
        manifest.assets = Rc::try_unwrap(assets).map_err(|_| "module asset collector is still borrowed".to_owned())?.into_inner();
        Ok(manifest)
    }

    /// Compile a closed program whose whole source is the entry (`Variant::Core`): the entry
    /// program is the application itself, not the Node loader's empty bootstrap hook.
    pub fn compile_closed(self, source: &str, kind: quench_runtime::SourceKind) -> Result<CompiledSources, String> {
        let mut compiler = NativeJsCompiler::new(true);
        let entry = match kind {
            quench_runtime::SourceKind::Module => compiler.compile_module(source, NATIVE_ENTRY_NAME),
            quench_runtime::SourceKind::Script | quench_runtime::SourceKind::Eval => compiler.compile_script(source, NATIVE_ENTRY_NAME, Default::default()),
        }.map_err(|errors| format!("{errors:?}"))?;
        Ok(CompiledSources { entry: compiler.finish(entry)?, units: Vec::new(), syntax_errors: Vec::new(), assets: self.assets })
    }

    /// Compile every recorded body before execution; no application is trained
    /// or executed to create these native compilation records.
    pub fn compile(self) -> Result<CompiledSources, String> {
        let mut compiler = NativeJsCompiler::new(true);
        let entry = match self.assets.entry.as_ref().and_then(|path| self.assets.files.get(path).map(|file| (path, file.kind))) {
            Some((path, quench_runtime::SourceKind::Module)) => {
                let url = url::Url::from_file_path(path).map_err(|_| "invalid ES module entry path")?;
                let source = format!("import {};", serde_json::to_string(url.as_str()).map_err(|error| error.to_string())?);
                compiler.compile_module(&source, NATIVE_ENTRY_NAME)
            },
            _ => compiler.compile_script("", NATIVE_ENTRY_NAME, Default::default()),
        }.map_err(|errors| format!("{errors:?}"))?;
        let mut units = Vec::new();
        let mut syntax_errors = Vec::new();
        for key in self.units {
            let compiled = if key.is_function_constructor() {
                compiler.compile_function_source(&key)
            } else {
                compiler.compile_source(&key).map(NativeSourceCompilation::Program)
            }.map_err(|error| format!("{}: {error}", key.name()))?;
            match compiled {
                NativeSourceCompilation::Program(program) => units.push(CompiledSource { key, program }),
                NativeSourceCompilation::SyntaxError(message) => syntax_errors.push(DeferredSourceSyntaxError { key, message }),
            }
        }
        Ok(CompiledSources { entry: compiler.finish(entry)?, units, syntax_errors, assets: self.assets })
    }
}

#[cfg(test)]
mod tests {
    use super::*;



    #[test]
    fn local_names_and_typescript_assertions_do_not_create_host_global_demands() {
        let directory = std::env::temp_dir().join(format!("quench-shadowed-global-exports-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let entry = directory.join("entry.cts");
        fs::write(&entry, "function read(performance: unknown, console: unknown, Blob: unknown) { return [performance as Performance, console as Console, Blob as typeof globalThis.Blob]; }").unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        assert!(graph.host_globals.is_empty(), "{:?}", graph.host_globals);
        assert!(!graph.builtins.contains("node:console"));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn type_queries_and_asserted_local_global_objects_do_not_request_sdk_sources() {
        let directory = std::env::temp_dir().join(format!("quench-type-only-sdk-demands-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let entry = directory.join("entry.cts");
        fs::write(&entry, r#"
            function typed(clock: typeof performance, encoder: typeof TextEncoder, loader: typeof require): typeof EventTarget { throw 1; }
            function local(globalThis: unknown) { return (globalThis as typeof globalThis).TextEncoder; }
        "#).unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        assert!(graph.host_globals.is_empty(), "{:?}", graph.host_globals);
        assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn graph_tracks_global_eval_references_without_trusting_types_or_shadowed_names() {
        let directory = std::env::temp_dir().join(format!("quench-eval-demands-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let entry = directory.join("entry.cts");
        let dependency = directory.join("dependency.cjs");
        fs::write(&entry, "require('./dependency.cjs'); function local(eval: (x: string) => string) { return eval('x'); } type T = typeof eval;").unwrap();
        fs::write(&dependency, "const indirect = eval; globalThis.eval; globalThis['eval'];").unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
        assert_eq!(graph.dynamic_eval_sites.len(), 3);
        let dependency = dependency.canonicalize().unwrap();
        assert!(graph.dynamic_eval_sites.iter().all(|(path, _)| path == &dependency), "{:?}", graph.dynamic_eval_sites);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn graph_uses_constructor_flow_for_dynamic_source_demands() {
        let directory = std::env::temp_dir().join(format!("quench-constructor-demands-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let entry = directory.join("entry.cts");
        fs::write(&entry, r#"
            const source = process.argv[2] as unknown as number;
            const ordinary = Function;
            const Async = (async function() {}).constructor;
            ordinary(source); new Function(source); Async(source);
            Function('return 42');
        "#).unwrap();
        let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
        assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
        assert_eq!(graph.dynamic_function_sites.len(), 3, "{:?}", graph.dynamic_function_sites);
        assert!(graph.dynamic_eval_sites.is_empty());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn graph_requires_dynamic_source_for_reflective_forms_only() {
        let directory = std::env::temp_dir().join(format!("quench-reflective-demands-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        for (source, requires) in [
            ("eval('1');", true), ("new Function('return 1');", true), ("const F = value.constructor; F(text);", true),
            // A computed read alone is an index (`SourceDemands::computed`), not a name the program proves dynamic;
            // only where it is called does it reach a constructor by a name the flow cannot follow.
            ("const key = text(); value[key];", false), ("const key = text(); value[key](text);", true),
            ("const o = { a: 1 }; o.a + o['b'] + [1][0];", false),
        ] {
            let entry = directory.join("entry.cjs");
            fs::write(&entry, source).unwrap();
            let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
            assert_eq!(graph.requires_dynamic_source, requires, "{source}");
        }
        fs::remove_dir_all(directory).unwrap();
    }

}
