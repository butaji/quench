use quench_runtime::native_link::{NativeImageInputs, CATALOG_FILENAME, REQUIRED_EXPORTS_FILENAME};
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
        let mut objects = Vec::new();
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
        let (entry, entry_symbol) = write(&self.entry, "entry", None)?;
        let mut catalog = fs::File::create(directory.join(CATALOG_FILENAME)).map_err(|error| error.to_string())?;
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
        let entry_path = self.assets.entry.as_deref().map(|path| path.to_str().ok_or("module entry is not UTF-8")).transpose()?;
        writeln!(catalog, "quench_node::linked_module_assets! {{ values embedded_modules; entry {entry_path:?}; files {{")
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
        fs::write(directory.join(REQUIRED_EXPORTS_FILENAME), required_exports.iter().map(String::as_str).collect::<Vec<_>>().join("\n") + "\n")
            .map_err(|error| error.to_string())?;
        let exports = required_exports.iter().map(|name| format!("{name:?}")).collect::<Vec<_>>().join(",");
        writeln!(catalog, "quench_runtime::export_native_js_operations!(quench_node::NodeHost, semantic_exports, [{exports}]);")
            .map_err(|error| error.to_string())?;
        let host_exports = crate::modules::napi::exported_symbol_names().map(str::to_owned)
            .chain(std::iter::once(crate::host::resident_pages::COMPILED_ENTRY_SYMBOL.to_owned())).collect();
        Ok(NativeImageInputs::new(directory, objects, host_exports))
    }
}

