//! Frozen module inputs. These are loader assets, not a replacement filesystem.
use quench_runtime::SourceKind;
#[cfg(feature = "aot")]
use std::cell::RefCell;
use std::{collections::BTreeMap, path::{Path, PathBuf}, rc::Rc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PreparedModuleSource<T> {
    Original,
    Projected(T),
}

#[derive(Clone)]
pub struct ModuleFile {
    pub bytes: Rc<[u8]>,
    pub kind: SourceKind,
    /// Host-policy projection frozen during source staging, when needed.
    pub prepared: Option<PreparedModuleSource<Rc<str>>>,
}

#[derive(Default)]
pub struct ModuleAssets {
    pub files: BTreeMap<PathBuf, ModuleFile>,
    pub resolutions: BTreeMap<(PathBuf, String), PathBuf>,
    pub node_root: Option<PathBuf>,
    pub entry: Option<PathBuf>,
}

#[cfg(feature = "aot")]
impl ModuleAssets {
    pub(super) fn extend(&mut self, other: Self) -> Result<(), String> {
        for (path, file) in &other.files {
            if self.files.get(path).is_some_and(|old| old.bytes != file.bytes || old.kind != file.kind || matches!((&old.prepared, &file.prepared), (Some(a), Some(b)) if a != b)) {
                return Err(format!("conflicting module input: {}", path.display()));
            }
        }
        for (edge, target) in &other.resolutions {
            if self.resolutions.get(edge).is_some_and(|old| old != target) {
                return Err(format!("conflicting module resolution: {} from {}", edge.1, edge.0.display()));
            }
        }
        if self.node_root.is_some() && other.node_root.is_some() && self.node_root != other.node_root {
            return Err("conflicting Node SDK roots".into());
        }
        if self.entry.is_some() && other.entry.is_some() && self.entry != other.entry {
            return Err("conflicting application entries".into());
        }
        for (path, mut file) in other.files {
            if file.prepared.is_none() {
                file.prepared = self.files.get(&path).and_then(|old| old.prepared.clone());
            }
            self.files.insert(path, file);
        }
        self.resolutions.extend(other.resolutions);
        if other.node_root.is_some() { self.node_root = other.node_root; }
        if other.entry.is_some() { self.entry = other.entry; }
        Ok(())
    }
}

pub struct EmbeddedModuleFile {
    pub path: &'static str,
    pub bytes: &'static [u8],
    pub kind: SourceKind,
    pub prepared: Option<PreparedModuleSource<&'static str>>,
}

pub struct EmbeddedModuleResolution {
    pub parent: &'static str,
    pub specifier: &'static str,
    pub target: &'static str,
}

/// Compiler-generated, sorted original module bytes and resolved edges.
pub struct EmbeddedModules {
    pub node_root: Option<&'static str>,
    pub entry: Option<&'static str>,
    pub files: &'static [EmbeddedModuleFile],
    pub resolutions: &'static [EmbeddedModuleResolution],
}

impl EmbeddedModules {
    pub(crate) fn file(&self, path: &Path) -> Option<&EmbeddedModuleFile> {
        self.files.binary_search_by(|file| Path::new(file.path).cmp(path)).ok().map(|index| &self.files[index])
    }

    pub(crate) fn resolve(&self, parent: &Path, specifier: &str) -> Option<&'static Path> {
        self.resolutions.binary_search_by(|edge| Path::new(edge.parent).cmp(parent).then_with(|| edge.specifier.cmp(specifier)))
            .ok().map(|index| Path::new(self.resolutions[index].target))
    }
}

#[derive(Default)]
pub(crate) enum ModuleAssetState {
    #[default]
    Filesystem,
    #[cfg(feature = "aot")]
    Capture(Rc<RefCell<ModuleAssets>>),
    Embedded(&'static EmbeddedModules),
}

impl crate::NodeHost {
    /// Use frozen compiler inputs for bundled modules; ordinary data-file APIs
    /// retain their existing filesystem behavior.
    pub fn with_embedded_modules(mut self, modules: &'static EmbeddedModules) -> Self {
        if let Some(root) = modules.node_root { self.node_library_root = root.into(); }
        if let Some(entry) = modules.entry { self = self.with_commonjs_entry(entry.into()); }
        self.module_assets = ModuleAssetState::Embedded(modules);
        self
    }

    pub(crate) fn embedded_module(&self, path: &Path) -> Option<&'static EmbeddedModuleFile> {
        match self.module_assets {
            ModuleAssetState::Embedded(modules) => modules.file(path),
            _ => None,
        }
    }

    pub(crate) fn module_path(&self, path: &Path) -> std::io::Result<PathBuf> {
        if self.embedded_module(path).is_some() { return Ok(path.to_owned()); }
        std::fs::canonicalize(path)
    }

    pub(crate) fn module_bytes(&mut self, path: &Path, _kind: SourceKind) -> std::io::Result<Vec<u8>> {
        if let Some(file) = self.embedded_module(path) { return Ok(file.bytes.to_vec()); }
        let bytes = std::fs::read(path).or_else(|error| {
            #[cfg(feature = "aot")]
            if matches!(self.module_assets, ModuleAssetState::Capture(_)) {
                return crate::node_sources::source(path, &self.node_library_root)
                    .map(|source| source.as_bytes().to_vec()).ok_or(error);
            }
            Err(error)
        })?;
        #[cfg(feature = "aot")]
        self.record_module_bytes(path, _kind, &bytes)?;
        Ok(bytes)
    }

    #[cfg(feature = "aot")]
    pub(crate) fn record_module_bytes(&mut self, path: &Path, kind: SourceKind, bytes: &[u8]) -> std::io::Result<()> {
        if let ModuleAssetState::Capture(assets) = &self.module_assets {
            let mut assets = assets.borrow_mut();
            match assets.files.entry(path.to_owned()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(ModuleFile { bytes: Rc::from(bytes), kind, prepared: None });
                },
                std::collections::btree_map::Entry::Occupied(entry) => {
                    if entry.get().bytes.as_ref() != bytes || entry.get().kind != kind {
                        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("module changed during source capture: {}", path.display())));
                    }
                },
            }
        }
        Ok(())
    }

    #[cfg(feature = "aot")]
    pub(crate) fn record_prepared_module(&mut self, path: &Path, text: &str) -> Result<(), String> {
        if let ModuleAssetState::Capture(assets) = &self.module_assets {
            let mut assets = assets.borrow_mut();
            let file = assets.files.get_mut(path).ok_or("prepared module has no captured original input")?;
            let prepared = if file.bytes.as_ref() == text.as_bytes() { PreparedModuleSource::Original }
                else { PreparedModuleSource::Projected(Rc::from(text)) };
            if file.prepared.as_ref().is_some_and(|previous| previous != &prepared) {
                return Err(format!("module preparation changed during source capture: {}", path.display()));
            }
            file.prepared = Some(prepared);
        }
        Ok(())
    }

    pub(crate) fn embedded_resolution(&self, parent: &Path, specifier: &str) -> Option<PathBuf> {
        match self.module_assets {
            ModuleAssetState::Embedded(modules) => modules.resolve(parent, specifier).map(Path::to_owned),
            _ => None,
        }
    }

    #[cfg(feature = "aot")]
    pub(crate) fn record_module_resolution(&mut self, parent: &Path, specifier: &str, target: &Path) -> Result<(), String> {
        if let ModuleAssetState::Capture(assets) = &self.module_assets {
            let mut assets = assets.borrow_mut();
            let edge = (parent.to_owned(), specifier.to_owned());
            if assets.resolutions.get(&edge).is_some_and(|previous| previous != target) {
                return Err(format!("module resolution changed during source capture: {specifier} from {}", parent.display()));
            }
            assets.resolutions.insert(edge, target.to_owned());
        }
        Ok(())
    }
}
