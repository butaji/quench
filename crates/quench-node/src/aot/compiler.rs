use super::{DependencyGraph, ModuleAssets, ModuleAssetState, PreparedModuleSource};
use crate::NodeHost;
use quench_runtime::{Engine, NativeJsCompiler, NativeSourceKey, NativeSourceCompilation, ResidualProgram, Runtime, emit_native_module, lower_native_js_module};
use std::{cell::RefCell, collections::BTreeSet, fs, io::Write, path::{Path, PathBuf}, rc::Rc};

mod source_pool;
use source_pool::SourcePool;

pub(super) const NATIVE_ENTRY_NAME: &str = "<quench:aot-entry>";

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
pub struct NativeImageInputs {
    directory: PathBuf,
    units: usize,
}

impl NativeImageInputs {
    pub fn catalog(&self) -> PathBuf { self.directory.join("catalog.rs") }
    pub fn required_exports(&self) -> PathBuf { self.directory.join("required_exports.txt") }
    pub fn objects(&self) -> impl Iterator<Item = PathBuf> + '_ {
        std::iter::once(self.directory.join("entry.o"))
            .chain((0..self.units).map(|index| self.directory.join(format!("unit_{index}.o"))))
    }
}

impl CompiledSources {
    pub fn emit(self, directory: &Path) -> Result<NativeImageInputs, String> {
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
        let namespace = quench_runtime::NativeProgramNamespace::from_entry(&self.entry)?;
        let mut write = |program: &ResidualProgram, name: &str, namespace: Option<&quench_runtime::NativeProgramNamespace>| -> Result<(PathBuf, String), String> {
            program.freeze_native_diagnostics();
            let module = lower_native_js_module(program, &format!("quench_{name}"))?;
            let symbol = module.entry_table_symbol().to_owned();
            for function in &module.functions {
                if let quench_runtime::NativeFunction::Import { name, .. } = function {
                    required_exports.insert(name.clone());
                }
            }
            let object = emit_native_module(name, module.functions, module.data)?;
            fs::write(directory.join(format!("{name}.o")), object).map_err(|error| error.to_string())?;
            let metadata = directory.join(format!("{name}.bin"));
            match namespace {
                Some(namespace) => fs::write(&metadata, namespace.encode_with_external_source(program)?).map_err(|error| error.to_string())?,
                None => program.write_binary(&metadata)?,
            }
            Ok((metadata, symbol))
        };
        let (entry, entry_symbol) = write(&self.entry, "entry", None)?;
        let mut catalog = fs::File::create(directory.join("catalog.rs")).map_err(|error| error.to_string())?;
        writeln!(catalog, "quench_node::linked_source_pool!(quench_native_source_text, {:?});", pool_path.to_str().ok_or("source pool path is not UTF-8")?)
            .map_err(|error| error.to_string())?;
        writeln!(catalog, "const _: () = assert!(quench_runtime::ResidualProgram::RUNTIME_ABI_FINGERPRINT == {:#x}, \"native metadata ABI mismatch\");", ResidualProgram::RUNTIME_ABI_FINGERPRINT)
            .map_err(|error| error.to_string())?;
        writeln!(catalog, "quench_node::linked_native_sources! {{\nentry bind_entry: entry_table => ({entry_symbol:?}, {:?});\nresolver resolve {{", entry.to_str().ok_or("native image path is not UTF-8")?)
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
        let node_root = self.assets.node_root.as_deref().map(|path| path.to_str().ok_or("module root is not UTF-8")).transpose()?;
        let entry_path = self.assets.entry.as_deref().map(|path| path.to_str().ok_or("module entry is not UTF-8")).transpose()?;
        writeln!(catalog, "quench_node::linked_module_assets! {{ values embedded_modules; root {node_root:?}; entry {entry_path:?}; files {{")
            .map_err(|error| error.to_string())?;
        for (index, (path, file)) in self.assets.files.iter().enumerate() {
            let bytes = match std::str::from_utf8(&file.bytes) {
                Ok(text) => format!("{}.as_bytes()", pool.reference(text)),
                Err(_) => {
                    let bytes_path = directory.join(format!("asset_{index}.bin"));
                    fs::write(&bytes_path, file.bytes.as_ref()).map_err(|error| error.to_string())?;
                    format!("include_bytes!({:?})", bytes_path.to_str().ok_or("native image path is not UTF-8")?)
                }
            };
            let prepared = match &file.prepared {
                Some(PreparedModuleSource::Original) => "Some(quench_node::aot::PreparedModuleSource::Original)".to_owned(),
                Some(PreparedModuleSource::Projected(source)) => {
                    format!("Some(quench_node::aot::PreparedModuleSource::Projected({}))", pool.reference(source))
                },
                None => "None".to_owned(),
            };
            writeln!(catalog, "{:?} => (bytes {bytes}, {:?}, {prepared}),", path.to_str().ok_or("module asset path is not UTF-8")?, file.kind)
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} resolutions {{").map_err(|error| error.to_string())?;
        for ((parent, specifier), target) in &self.assets.resolutions {
            writeln!(catalog, "({:?}, {specifier:?}) => {:?},", parent.to_str().ok_or("module parent is not UTF-8")?, target.to_str().ok_or("module target is not UTF-8")?)
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} views {{").map_err(|error| error.to_string())?;
        for ((path, key), source) in &self.assets.views {
            writeln!(catalog, "({:?}, {key:?}) => source {},", path.to_str().ok_or("SDK view path is not UTF-8")?, pool.reference(source)).map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} imports {{").map_err(|error| error.to_string())?;
        for ((referrer, specifier), target) in &self.assets.imports {
            writeln!(catalog, "({referrer:?}, {specifier:?}) => {target:?},").map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} modules {{").map_err(|error| error.to_string())?;
        for (name, (source, typescript)) in &self.assets.modules {
            writeln!(catalog, "{name:?} => source {}, typescript {typescript},", pool.reference(source)).map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} }}").map_err(|error| error.to_string())?;
        fs::write(directory.join("required_exports.txt"), required_exports.iter().map(String::as_str).collect::<Vec<_>>().join("\n") + "\n")
            .map_err(|error| error.to_string())?;
        let exports = required_exports.iter().map(|name| format!("{name:?}")).collect::<Vec<_>>().join(",");
        writeln!(catalog, "quench_runtime::export_native_js_operations!(quench_node::NodeHost, semantic_exports, [{exports}]);")
            .map_err(|error| error.to_string())?;
        Ok(NativeImageInputs { directory, units: self.units.len() })
    }
}

#[derive(Default)]
pub(crate) struct HostSourceDemands {
    builtins: BTreeSet<String>,
    globals: BTreeSet<String>,
    esm_builtins: BTreeSet<String>,
}

enum CaptureStage<'a> {
    Snapshot(crate::host::startup_snapshot::SnapshotKind),
    Modules(&'a DependencyGraph),
}

/// Materialize only host-defined SDK sources in a fresh compiler VM. This point
/// precedes IPC, user preloads and application entry; no application is trained.
pub(crate) fn stage_host_sources(
    context: &mut quench_runtime::NativeContext<'_, NodeHost>, demands: HostSourceDemands,
) -> Result<(), quench_runtime::RootedError> {
    // The default warning listener is installed after the staging boundary and
    // can run for host-generated warnings without an application import.
    crate::modules::process_warning_shared_vm::prepare(context)?;
    let global = context.global_root()?;
    let process = context.string_rooted("process");
    let process = context.get_property_rooted(global, process)?;
    let env = context.string_rooted("env");
    context.get_property_rooted(process, env)?;
    let builtins: Vec<_> = demands.builtins.iter().map(String::as_str).collect();
    crate::host::startup_snapshot::preload_named(context, &builtins)?;
    for name in demands.esm_builtins {
        context.scoped(|context| crate::host::shared_vm::commonjs::stage_builtin_exports(context, &name))?;
    }
    // These imported modules expose SDK-backed lazy policies. Stage the
    // policies without invoking their exported operations or capturing state.
    for name in &builtins {
        context.scoped(|context| {
            let (module, member) = match name.strip_prefix("node:").unwrap_or(name) {
                "console" => return crate::modules::console_shared_vm::stage_formatting(context),
                // Public buffer exports include lazy Blob/File constructors; callers
                // can observe them by copying the module's enumerable properties.
                "buffer" => return crate::host::shared_vm::commonjs::stage_builtin_exports(context, "node:buffer"),
                // createRequire tests URL-like inputs through the SDK URL policy,
                // including the static CommonJS facade used by ESM imports.
                "module" => return crate::host::shared_vm::commonjs::internal_url_module(context).map(drop),
                "fs" | "fs/promises" => {
                    let fs = crate::host::shared_vm::commonjs::fs_module(context)?;
                    let key = context.string_rooted("promises");
                    (context.get_property_rooted(fs, key)?, "readlink")
                }
                "util" => (crate::modules::util_shared_vm::module(context)?, "aborted"),
                // Domain's capture callback preserves the process report flag.
                "domain" => (process, "report"),
                _ => return Ok(()),
            };
            let key = context.string_rooted(member);
            context.get_property_rooted(module, key).map(drop)
        })?;
    }
    for name in demands.globals {
        let key = context.string_rooted(&name);
        context.get_property_rooted(global, key)?;
    }
    Ok(())
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
    pub fn capture_bootstrap(library_root: &Path) -> Result<Self, String> {
        Self::capture(library_root, CaptureStage::Snapshot(crate::host::startup_snapshot::SnapshotKind::Bootstrap))
    }

    /// The existing Node snapshot preload set, staged before any application.
    pub fn capture_preloaded(library_root: &Path) -> Result<Self, String> {
        Self::capture(library_root, CaptureStage::Snapshot(crate::host::startup_snapshot::SnapshotKind::Preloaded))
    }

    pub fn capture_for_graph(library_root: &Path, graph: &DependencyGraph) -> Result<Self, String> {
        Self::capture(library_root, CaptureStage::Modules(graph))
    }

    pub fn extend(&mut self, other: Self) -> Result<(), String> {
        self.assets.extend(other.assets)?;
        self.units.extend(other.units);
        Ok(())
    }

    fn capture(library_root: &Path, stage: CaptureStage<'_>) -> Result<Self, String> {
        let inputs = Rc::new(RefCell::new(Self::default()));
        let captured = inputs.clone();
        let mut host = NodeHost::new(vec!["quench".into()]).with_exec_argv(Vec::new())?
            .with_native_programs(move |name, parts| {
                captured.borrow_mut().insert(name, parts)?;
                Ok(None)
            });
        host.node_library_root = library_root.canonicalize().map_err(|error| error.to_string())?;
        match stage {
            CaptureStage::Snapshot(kind) => host.building_snapshot = Some(kind),
            CaptureStage::Modules(graph) => host.native_source_staging = Some(HostSourceDemands {
                builtins: graph.builtins.union(&graph.esm_builtins).cloned().collect(),
                globals: graph.host_globals.clone(), esm_builtins: graph.esm_builtins.clone(),
            }),
        }
        let assets = Rc::new(RefCell::new(ModuleAssets { node_root: Some(host.node_library_root.clone()), ..Default::default() }));
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
    fn host_staging_uses_builtin_loaders_without_a_secondary_cache() {
        std::thread::Builder::new().stack_size(quench_runtime::WORKER_STACK_SIZE).spawn(|| {
            let directory = std::env::temp_dir().join(format!("quench-staged-builtin-loaders-{}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            let entry = directory.join("entry.mjs");
            fs::write(&entry, r#"
                import 'node:process';
                import 'node:path';
                import 'node:path/posix';
                import 'node:path/win32';
                import 'node:assert';
                import 'node:assert/strict';
                import 'node:async_hooks';
                import 'node:diagnostics_channel';
                throw new Error('application entry must not execute during staging');
            "#).unwrap();
            let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
            assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node");
            SourceManifest::capture_for_graph(&root, &graph).unwrap();
            fs::remove_dir_all(directory).unwrap();
        }).unwrap().join().unwrap();
    }

    #[test]
    fn commonjs_import_facades_stage_create_require_url_policy() {
        std::thread::Builder::new().stack_size(quench_runtime::WORKER_STACK_SIZE).spawn(|| {
            let directory = std::env::temp_dir().join(format!("quench-staged-create-require-{}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            let entry = directory.join("entry.mjs");
            fs::write(&entry, "import value from './dependency.cjs'; throw new Error('application must not execute during staging');").unwrap();
            fs::write(directory.join("dependency.cjs"), "module.exports = 42;").unwrap();
            let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
            assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
            assert!(graph.esm_builtins.contains("node:module"));
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node");
            let staged = SourceManifest::capture_for_graph(&root, &graph).unwrap();
            assert!(staged.assets().files.keys().any(|path|path.ends_with("lib/internal/url.js")));
            fs::remove_dir_all(directory).unwrap();
        }).unwrap().join().unwrap();
    }
    #[test]
    fn host_staging_captures_performance_and_lazy_buffer_exports() {
        std::thread::Builder::new().stack_size(quench_runtime::WORKER_STACK_SIZE).spawn(|| {
            let directory = std::env::temp_dir().join(format!("quench-staged-global-exports-{}", std::process::id()));
            fs::create_dir(&directory).unwrap();
            let entry = directory.join("entry.cts");
            fs::write(&entry, "require('node:buffer'); (performance as unknown as {now(): number}).now(); throw new Error('entry must not execute during staging');").unwrap();
            let graph = SourceManifest::default().add_module_graph(&entry).unwrap();
            assert!(graph.requirements.is_empty(), "{:?}", graph.requirements);
            assert!(graph.host_globals.contains("performance"));
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/node");
            let staged = SourceManifest::capture_for_graph(&root, &graph).unwrap();
            assert!(staged.units().any(|key| key.name() == "node:perf_hooks.js"));
            assert!(staged.assets().files.keys().any(|path|path.ends_with("lib/internal/blob.js")));
            fs::remove_dir_all(directory).unwrap();
        }).unwrap().join().unwrap();
    }

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

}
