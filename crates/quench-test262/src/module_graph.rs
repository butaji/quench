use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleUnit {
    pub id: ModuleId,
    pub path: PathBuf,
    pub source: String,
    pub kind: ModuleKind,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleKind {
    JavaScript,
    Json,
    Text,
    Bytes,
}

fn dynamic_kind(specifier: &str) -> ModuleKind {
    if specifier.ends_with(".json") {
        ModuleKind::Json
    } else if specifier.ends_with(".txt") {
        ModuleKind::Text
    } else if specifier.ends_with(".bin") {
        ModuleKind::Bytes
    } else {
        ModuleKind::JavaScript
    }
}

fn import_attribute_kind(value: &str) -> Option<ModuleKind> {
    match value {
        "type=json" => Some(ModuleKind::Json),
        "type=text" => Some(ModuleKind::Text),
        "type=bytes" => Some(ModuleKind::Bytes),
        _ => None,
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ModuleGraph {
    entry: Option<ModuleId>,
    units: Vec<ModuleUnit>,
    paths: HashMap<(PathBuf, ModuleKind), ModuleId>,
    edges: HashMap<ModuleId, Vec<ModuleId>>,
    deferred_edges: std::collections::HashSet<(ModuleId, ModuleId)>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_entry(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, ModuleKind::JavaScript, true)
    }

    pub fn add_dependency(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, ModuleKind::JavaScript, false)
    }

    pub fn add_json_dependency(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, ModuleKind::Json, false)
    }

    pub fn add_text_dependency(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, ModuleKind::Text, false)
    }

    pub fn add_bytes_dependency(&mut self, path: PathBuf, bytes: Vec<u8>) -> ModuleId {
        let id = self.add_unit(path, String::new(), ModuleKind::Bytes, false);
        self.units[id.0 as usize].bytes = Some(bytes);
        id
    }

    pub fn entry(&self) -> Option<ModuleId> {
        self.entry
    }

    pub fn entry_unit(&self) -> Option<&ModuleUnit> {
        self.entry.and_then(|id| self.unit(id))
    }

    pub fn unit(&self, id: ModuleId) -> Option<&ModuleUnit> {
        self.units.get(id.0 as usize).filter(|unit| unit.id == id)
    }

    pub fn dependencies(&self, id: ModuleId) -> Vec<ModuleId> {
        self.edges.get(&id).into_iter().flatten().copied().collect()
    }

    pub fn is_deferred_edge(&self, from: ModuleId, to: ModuleId) -> bool {
        self.deferred_edges.contains(&(from, to))
    }

    pub fn has_async_dependency(&self, id: ModuleId) -> Result<bool, String> {
        self.gather_async_dependencies(id, &mut Vec::new(), &mut Vec::new())
    }

    pub fn units(&self) -> &[ModuleUnit] {
        &self.units
    }

    pub fn resolve(&self, from: ModuleId, specifier: &str) -> Option<ModuleId> {
        self.resolve_kind(from, specifier, ModuleKind::JavaScript)
    }

    pub fn resolve_kind(
        &self,
        from: ModuleId,
        specifier: &str,
        kind: ModuleKind,
    ) -> Option<ModuleId> {
        if specifier == "<module source>" {
            return Some(from);
        }
        let base = self.units.get(from.0 as usize)?.path.parent()?;
        let path = normalize_module_path(&base.join(specifier));
        self.paths.get(&(path, kind)).copied()
    }

    /// Record a statically resolved import edge. The graph owns resolution;
    /// execution remains the host's concern.
    pub fn link(&mut self, from: ModuleId, to: ModuleId) -> Result<(), String> {
        if self.unit(from).is_none() || self.unit(to).is_none() {
            return Err("module edge references an unknown unit".to_string());
        }
        let dependencies = self.edges.entry(from).or_default();
        if !dependencies.contains(&to) {
            dependencies.push(to);
        }
        Ok(())
    }

    /// Resolve and record all static imports discovered by the runtime's OXC
    /// metadata pass. Resolution remains graph-owned; execution is separate.
    pub fn link_specifiers<'a, I>(&mut self, from: ModuleId, specifiers: I) -> Result<(), String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        for specifier in specifiers {
            let target = self.resolve(from, specifier).ok_or_else(|| {
                format!("unresolved module specifier {specifier:?} from {from:?}")
            })?;
            self.link(from, target)?;
        }
        Ok(())
    }

    /// Extract and link the unit's static imports through the runtime's OXC
    /// metadata path. The graph owns resolution; the runtime owns syntax.
    pub fn link_unit_imports(&mut self, from: ModuleId) -> Result<(), String> {
        let unit = self
            .unit(from)
            .ok_or_else(|| "module unit is unknown".to_string())?;
        if unit.kind != ModuleKind::JavaScript {
            return Ok(());
        }
        let source = unit.source.clone();
        let metadata = quench_runtime::reduce::inspect_module_source(&source)
            .map_err(|errors| errors.join("; "))?;
        for specifier in &metadata.import_specifiers {
            let kind = metadata
                .import_attributes
                .iter()
                .find(|(source, _)| source == specifier)
                .and_then(|(_, value)| match value.as_str() {
                    "type=json" => Some(ModuleKind::Json),
                    "type=text" => Some(ModuleKind::Text),
                    "type=bytes" => Some(ModuleKind::Bytes),
                    _ => None,
                })
                .unwrap_or(ModuleKind::JavaScript);
            let target = self.resolve_kind(from, specifier, kind).ok_or_else(|| {
                format!("unresolved module specifier {specifier:?} from {from:?}")
            })?;
            let is_deferred = metadata
                .deferred_imports
                .iter()
                .any(|item| item == specifier);
            let has_eager_request = metadata.eager_imports.iter().any(|item| item == specifier);
            if !is_deferred || !has_eager_request {
                self.link(from, target)?;
            }
            if metadata
                .deferred_imports
                .iter()
                .any(|item| item == specifier)
                && !has_eager_request
            {
                self.deferred_edges.insert((from, target));
            }
        }
        for specifier in &metadata.eager_imports {
            if metadata
                .deferred_imports
                .iter()
                .any(|item| item == specifier)
            {
                let kind = metadata
                    .import_attributes
                    .iter()
                    .find(|(source, _)| source == specifier)
                    .and_then(|(_, value)| import_attribute_kind(value))
                    .unwrap_or(ModuleKind::JavaScript);
                let target = self.resolve_kind(from, specifier, kind).ok_or_else(|| {
                    format!("unresolved module specifier {specifier:?} from {from:?}")
                })?;
                self.link(from, target)?;
            }
        }
        for specifier in &metadata.dynamic_imports {
            let kind = dynamic_kind(specifier);
            let _ = self.ensure_dependency(from, specifier, kind);
        }
        Ok(())
    }

    fn ensure_dependency(
        &mut self,
        from: ModuleId,
        specifier: &str,
        kind: ModuleKind,
    ) -> Result<ModuleId, String> {
        if let Some(target) = self.resolve_kind(from, specifier, kind) {
            return Ok(target);
        }
        let base = self
            .unit(from)
            .and_then(|unit| unit.path.parent())
            .ok_or_else(|| "dynamic import source has no base path".to_string())?;
        let path = normalize_module_path(&base.join(specifier));
        match kind {
            ModuleKind::Json | ModuleKind::Text | ModuleKind::JavaScript => {
                let source = std::fs::read_to_string(&path)
                    .map_err(|error| format!("module {}: {error}", path.display()))?;
                if kind == ModuleKind::JavaScript {
                    Self::validate_static_source(&path, &source, &mut Vec::new())?;
                }
                Ok(match kind {
                    ModuleKind::Json => self.add_json_dependency(path, source),
                    ModuleKind::Text => self.add_text_dependency(path, source),
                    ModuleKind::JavaScript => self.add_dependency(path, source),
                    ModuleKind::Bytes => unreachable!(),
                })
            }
            ModuleKind::Bytes => {
                let bytes = std::fs::read(&path)
                    .map_err(|error| format!("module {}: {error}", path.display()))?;
                Ok(self.add_bytes_dependency(path, bytes))
            }
        }
    }

    fn validate_static_source(
        path: &Path,
        source: &str,
        seen: &mut Vec<PathBuf>,
    ) -> Result<(), String> {
        let path = normalize_module_path(path);
        if seen.contains(&path) {
            return Ok(());
        }
        seen.push(path.clone());
        let metadata = quench_runtime::reduce::inspect_module_source(source)
            .map_err(|errors| errors.join("; "))?;
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        for imported in metadata.import_specifiers {
            if imported == "<module source>" {
                continue;
            }
            let imported_path = normalize_module_path(&base.join(&imported));
            let imported_source = std::fs::read_to_string(&imported_path)
                .map_err(|_| format!("unresolved module {}", imported_path.display()))?;
            Self::validate_static_source(&imported_path, &imported_source, seen)?;
        }
        Ok(())
    }

    /// Link every currently loaded unit from its canonical static metadata.
    pub fn link_all_units(&mut self) -> Result<(), String> {
        let units = self.units.iter().map(|unit| unit.id).collect::<Vec<_>>();
        for unit in units {
            self.link_unit_imports(unit)?;
        }
        Ok(())
    }

    /// Return deterministic post-order units for an entry, preserving edge
    /// order and tolerating back-edges for legal cyclic module graphs.
    pub fn dependency_order(&self, entry: ModuleId) -> Result<Vec<ModuleId>, String> {
        if self.unit(entry).is_none() {
            return Err("module entry references an unknown unit".to_string());
        }
        let mut state = HashMap::new();
        let mut order = Vec::new();
        self.visit(entry, &mut state, &mut order)?;
        Ok(order)
    }

    fn visit(
        &self,
        id: ModuleId,
        state: &mut HashMap<ModuleId, u8>,
        order: &mut Vec<ModuleId>,
    ) -> Result<(), String> {
        if state.get(&id).copied() == Some(2) {
            return Ok(());
        }
        if state.get(&id).copied() == Some(1) {
            return Ok(());
        }
        state.insert(id, 1);
        let dependencies = self.edges.get(&id).cloned().unwrap_or_default();
        for dependency in dependencies {
            let deferred = self.deferred_edges.contains(&(id, dependency));
            if deferred {
                let mut asynchronous = Vec::new();
                self.gather_async_dependencies(dependency, &mut Vec::new(), &mut asynchronous)?;
                for asynchronous in asynchronous {
                    self.visit(asynchronous, state, order)?;
                }
                continue;
            }
            self.visit(dependency, state, order)?;
        }
        state.insert(id, 2);
        order.push(id);
        Ok(())
    }

    fn gather_async_dependencies(
        &self,
        id: ModuleId,
        seen: &mut Vec<ModuleId>,
        asynchronous: &mut Vec<ModuleId>,
    ) -> Result<bool, String> {
        if seen.contains(&id) {
            return Ok(false);
        }
        seen.push(id);
        let Some(unit) = self.unit(id) else {
            return Err("module unit is unknown".to_string());
        };
        if unit.kind != ModuleKind::JavaScript {
            return Ok(false);
        }
        if quench_runtime::reduce::inspect_module_source(&unit.source)
            .map_err(|errors| errors.join("; "))?
            .has_top_level_await
        {
            if !asynchronous.contains(&id) {
                asynchronous.push(id);
            }
            return Ok(true);
        }
        for dependency in self.edges.get(&id).into_iter().flatten() {
            self.gather_async_dependencies(*dependency, seen, asynchronous)?;
        }
        Ok(!asynchronous.is_empty())
    }

    fn add_unit(
        &mut self,
        path: PathBuf,
        source: String,
        kind: ModuleKind,
        entry: bool,
    ) -> ModuleId {
        let path = normalize_module_path(&path);
        if let Some(id) = self.paths.get(&(path.clone(), kind)).copied() {
            return id;
        }
        let id = ModuleId(self.units.len() as u32);
        self.paths.insert((path.clone(), kind), id);
        self.units.push(ModuleUnit {
            id,
            path,
            source,
            kind,
            bytes: None,
        });
        if entry {
            self.entry = Some(id);
        }
        id
    }
}

fn normalize_module_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                result.pop();
            }
            component => result.push(component.as_os_str()),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{ModuleGraph, ModuleId};
    use std::path::PathBuf;

    #[test]
    fn graph_deduplicates_units_and_resolves_relative_edges() {
        let mut graph = ModuleGraph::new();
        let entry = graph.add_entry(
            PathBuf::from("test/entry.js"),
            "import value from './lib/value.js';".to_string(),
        );
        let dependency = graph.add_dependency(PathBuf::from("test/lib/value.js"), "".to_string());
        assert_eq!(
            graph.add_dependency(
                PathBuf::from("test/lib/../lib/value.js"),
                "changed".to_string()
            ),
            dependency
        );
        assert_eq!(graph.resolve(entry, "./lib/value.js"), Some(dependency));
        assert_eq!(graph.entry(), Some(ModuleId(0)));
        assert_eq!(graph.entry_unit().map(|unit| unit.id), Some(entry));
        assert_eq!(graph.unit(dependency).map(|unit| unit.id), Some(dependency));
        assert_eq!(graph.units().len(), 2);
        graph.link_all_units().expect("known edge");
        assert_eq!(
            graph.dependency_order(entry).unwrap(),
            vec![dependency, entry]
        );
    }

    #[test]
    fn graph_allows_cycles_and_rejects_unknown_edges() {
        let mut graph = ModuleGraph::new();
        let entry = graph.add_entry(PathBuf::from("entry.js"), "".to_string());
        let dependency = graph.add_dependency(PathBuf::from("dep.js"), "".to_string());
        assert!(graph.link(entry, ModuleId(99)).is_err());
        graph.link(entry, dependency).unwrap();
        graph.link(dependency, entry).unwrap();
        assert_eq!(
            graph.dependency_order(entry).unwrap(),
            vec![dependency, entry]
        );
    }
}
