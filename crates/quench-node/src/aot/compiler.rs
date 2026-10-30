use super::{DependencyGraph, ModuleAssets, ModuleAssetState, PreparedModuleSource};
use crate::NodeHost;
use quench_runtime::{Engine, NativeJsCompiler, NativeSourceKey, ResidualProgram, Runtime, emit_native_module, lower_native_js_module};
use std::{cell::RefCell, collections::BTreeSet, fs, io::Write, path::{Path, PathBuf}, rc::Rc};

const NATIVE_ENTRY_NAME: &str = "<quench:aot-entry>";

/// Original source keys, sorted and deduplicated by full equality. This manifest
/// is compiler input, rather than a list of sources inferred from benchmark names.
#[derive(Default)]
pub struct SourceManifest {
    units: BTreeSet<NativeSourceKey>,
    assets: ModuleAssets,
}

pub struct CompiledSource {
    pub key: NativeSourceKey,
    pub program: ResidualProgram,
}

pub struct CompiledSources {
    pub entry: ResidualProgram,
    pub units: Vec<CompiledSource>,
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
        let mut required_exports = BTreeSet::new();
        let namespace = quench_runtime::NativeProgramNamespace::from_entry(&self.entry)?;
        let mut write = |program: &ResidualProgram, name: &str, namespace: Option<&quench_runtime::NativeProgramNamespace>| -> Result<(PathBuf, String), String> {
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
                Some(namespace) => fs::write(&metadata, namespace.encode(program)?).map_err(|error| error.to_string())?,
                None => program.write_binary(&metadata)?,
            }
            Ok((metadata, symbol))
        };
        let (entry, entry_symbol) = write(&self.entry, "entry", None)?;
        let mut catalog = fs::File::create(directory.join("catalog.rs")).map_err(|error| error.to_string())?;
        writeln!(catalog, "quench_node::linked_native_sources! {{\nentry bind_entry: entry_table => ({entry_symbol:?}, {:?});\nresolver resolve {{", entry.to_str().ok_or("native image path is not UTF-8")?)
            .map_err(|error| error.to_string())?;
        for (index, unit) in self.units.iter().enumerate() {
            let (metadata, symbol) = write(&unit.program, &format!("unit_{index}"), Some(&namespace))?;
            let parts: Vec<_> = unit.key.parts().collect();
            writeln!(catalog, "source_{index}_table => ({symbol:?}, {:?}, {:?}, {:?}),", metadata.to_str().ok_or("native image path is not UTF-8")?, unit.key.name(), parts)
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}}\n}}").map_err(|error| error.to_string())?;
        let node_root = self.assets.node_root.as_deref().map(|path| path.to_str().ok_or("module root is not UTF-8")).transpose()?;
        let entry_path = self.assets.entry.as_deref().map(|path| path.to_str().ok_or("module entry is not UTF-8")).transpose()?;
        writeln!(catalog, "quench_node::linked_module_assets! {{ embedded_modules; root {node_root:?}; entry {entry_path:?}; files {{")
            .map_err(|error| error.to_string())?;
        for (index, (path, file)) in self.assets.files.iter().enumerate() {
            let bytes_path = directory.join(format!("asset_{index}.bin"));
            fs::write(&bytes_path, file.bytes.as_ref()).map_err(|error| error.to_string())?;
            let prepared = match &file.prepared {
                Some(PreparedModuleSource::Original) => "Some(quench_node::aot::PreparedModuleSource::Original)".to_owned(),
                Some(PreparedModuleSource::Projected(source)) => {
                    let path = directory.join(format!("asset_{index}.prepared"));
                    fs::write(&path, source.as_bytes()).map_err(|error| error.to_string())?;
                    format!("Some(quench_node::aot::PreparedModuleSource::Projected(include_str!({:?})))", path.to_str().ok_or("native image path is not UTF-8")?)
                },
                None => "None".to_owned(),
            };
            writeln!(catalog, "{:?} => ({:?}, {:?}, {prepared}),", path.to_str().ok_or("module asset path is not UTF-8")?, bytes_path.to_str().ok_or("native image path is not UTF-8")?, file.kind)
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} resolutions {{").map_err(|error| error.to_string())?;
        for ((parent, specifier), target) in &self.assets.resolutions {
            writeln!(catalog, "({:?}, {specifier:?}) => {:?},", parent.to_str().ok_or("module parent is not UTF-8")?, target.to_str().ok_or("module target is not UTF-8")?)
                .map_err(|error| error.to_string())?;
        }
        writeln!(catalog, "}} views {{").map_err(|error| error.to_string())?;
        for (index, ((path, key), source)) in self.assets.views.iter().enumerate() {
            let view_path = directory.join(format!("view_{index}.js"));
            fs::write(&view_path, source.as_bytes()).map_err(|error| error.to_string())?;
            writeln!(catalog, "({:?}, {key:?}) => {:?},", path.to_str().ok_or("SDK view path is not UTF-8")?, view_path.to_str().ok_or("native image path is not UTF-8")?).map_err(|error| error.to_string())?;
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
    // These imported modules expose SDK-backed lazy policies. Stage the
    // policies without invoking their exported operations or capturing state.
    for name in &builtins {
        context.scoped(|context| {
            let (module, member) = match name.strip_prefix("node:").unwrap_or(name) {
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

    pub fn add_commonjs_graph(&mut self, entry: &Path) -> Result<DependencyGraph, String> {
        let graph = super::graph::discover(self, entry)?;
        self.assets.entry = Some(entry.canonicalize().map_err(|error| error.to_string())?);
        self.assets.files.extend(graph.files.iter().map(|(path, file)| (path.clone(), file.clone())));
        self.assets.resolutions.extend(graph.resolutions.iter().map(|(edge, target)| (edge.clone(), target.clone())));
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
                builtins: graph.builtins.clone(), globals: graph.host_globals.clone(),
            }),
        }
        let assets = Rc::new(RefCell::new(ModuleAssets { node_root: Some(host.node_library_root.clone()), ..Default::default() }));
        host.module_assets = ModuleAssetState::Capture(assets.clone());
        let mut runtime = Runtime::new(host);
        let entry = Engine::specialize("", NATIVE_ENTRY_NAME).map_err(|errors| format!("{errors:?}"))?;
        runtime.execute_deferred_jobs(&entry).map_err(|error| error.to_string())?;
        drop(runtime);
        let mut manifest = Rc::try_unwrap(inputs).map_err(|_| "bootstrap source collector is still borrowed".to_owned())?.into_inner();
        manifest.assets = Rc::try_unwrap(assets).map_err(|_| "module asset collector is still borrowed".to_owned())?.into_inner();
        Ok(manifest)
    }

    /// Compile every recorded body before execution; no application is trained
    /// or executed to create these native compilation records.
    pub fn compile(self) -> Result<CompiledSources, String> {
        let mut compiler = NativeJsCompiler::new(true);
        let entry = compiler.compile_script("", NATIVE_ENTRY_NAME, Default::default())
            .map_err(|errors| format!("{errors:?}"))?;
        let units = self.units.into_iter().map(|key| {
            let program = compiler.compile_source(&key).map_err(|error| format!("{}: {error}", key.name()))?;
            Ok(CompiledSource { key, program })
        }).collect::<Result<Vec<_>, String>>()?;
        Ok(CompiledSources { entry: compiler.finish(entry)?, units, assets: self.assets })
    }
}
