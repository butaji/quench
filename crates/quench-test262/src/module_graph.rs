use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleUnit {
    pub id: ModuleId,
    pub path: PathBuf,
    pub source: String,
    pub bytes: Vec<u8>,
    pub kind: ModuleKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    JavaScript,
    Json,
    Text,
    Bytes,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ModuleGraph {
    entry: Option<ModuleId>,
    units: Vec<ModuleUnit>,
    paths: HashMap<PathBuf, ModuleId>,
    edges: HashMap<ModuleId, Vec<ModuleId>>,
    deferred_modules: HashSet<ModuleId>,
    dynamic_targets: HashSet<ModuleId>,
    resolution_errors: HashMap<ModuleId, String>,
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
}

impl ModuleGraph {
    pub fn has_resolution_error(&self, id: ModuleId) -> bool {
        self.resolution_errors.contains_key(&id)
    }

    pub fn mark_dynamic_target(&mut self, id: ModuleId) {
        self.dynamic_targets.insert(id);
    }

    pub fn is_dynamic_target(&self, id: ModuleId) -> bool {
        self.dynamic_targets.contains(&id)
    }

    pub fn mark_deferred_modules(&mut self) {
        let ids = self.units.iter().map(|unit| unit.id).collect::<Vec<_>>();
        for from in ids {
            let Some(metadata) = self.unit(from).and_then(|unit| {
                (unit.kind == ModuleKind::JavaScript)
                    .then(|| quench_runtime::reduce::inspect_module_source(&unit.source).ok())
                    .flatten()
            }) else {
                continue;
            };
            for request in metadata.requests.iter().filter(|request| request.deferred) {
                if let Some(target) = self.resolve(from, &request.source) {
                    self.deferred_modules.insert(target);
                }
            }
        }
    }

    pub fn add_json_dependency(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, ModuleKind::Json, false)
    }

    pub fn add_text_dependency(&mut self, path: PathBuf, source: String) -> ModuleId {
        let path = normalize_module_path(&path);
        if self
            .paths
            .get(&path)
            .and_then(|id| self.unit(*id))
            .is_some_and(|unit| unit.kind == ModuleKind::JavaScript)
        {
            let id = ModuleId(self.units.len() as u32);
            self.paths.insert(path.clone(), id);
            self.units.push(ModuleUnit {
                id,
                path,
                source,
                bytes: Vec::new(),
                kind: ModuleKind::Text,
            });
            return id;
        }
        self.add_unit(path, source, ModuleKind::Text, false)
    }

    pub fn add_bytes_dependency(&mut self, path: PathBuf, bytes: Vec<u8>) -> ModuleId {
        self.add_unit_with_bytes(path, String::new(), bytes, ModuleKind::Bytes, false)
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

    pub fn units(&self) -> &[ModuleUnit] {
        &self.units
    }

    pub fn dependencies(&self, from: ModuleId) -> &[ModuleId] {
        self.edges.get(&from).map_or(&[], Vec::as_slice)
    }

    pub fn resolve(&self, from: ModuleId, specifier: &str) -> Option<ModuleId> {
        if specifier == "<module source>" {
            return self.paths.get(Path::new(specifier)).copied();
        }
        let base = self.units.get(from.0 as usize)?.path.parent()?;
        let path = normalize_module_path(&base.join(specifier));
        self.paths.get(&path).copied()
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

    pub fn link_specifiers<'a, I>(&mut self, from: ModuleId, specifiers: I) -> Result<(), String>
    where
        I: IntoIterator<Item = &'a str>,
    {
        for specifier in specifiers {
            let Some(target) = self.resolve(from, specifier) else {
                let from_path = self.unit(from).map_or_else(
                    || format!("<unknown:{:?}>", from),
                    |unit| unit.path.display().to_string(),
                );
                return Err(format!(
                    "unresolved static module specifier `{specifier}` from `{from_path}`"
                ));
            };
            self.link(from, target)?;
        }
        Ok(())
    }

    pub fn link_unit_imports(&mut self, from: ModuleId) -> Result<(), String> {
        let (kind, source, path) = {
            let unit = self
                .unit(from)
                .ok_or_else(|| "module unit is unknown".to_string())?;
            (
                unit.kind,
                unit.source.clone(),
                unit.path.display().to_string(),
            )
        };
        if matches!(
            kind,
            ModuleKind::Json | ModuleKind::Bytes | ModuleKind::Text
        ) {
            return Ok(());
        }
        let metadata = quench_runtime::reduce::inspect_module_source(&source)
            .map_err(|errors| errors.join("; "))?;
        for request in &metadata.requests {
            if let Some(target) = self.resolve(from, &request.source) {
                self.link(from, target)?;
            } else if self.deferred_modules.contains(&from) {
                self.resolution_errors.insert(from, request.source.clone());
            } else {
                return Err(format!(
                    "unresolved static module specifier `{}` from `{}`",
                    request.source, path
                ));
            }
        }
        Ok(())
    }

    pub fn link_all_units(&mut self) -> Result<(), String> {
        self.mark_deferred_modules();
        let units = self.units.iter().map(|unit| unit.id).collect::<Vec<_>>();
        for unit in units {
            self.link_unit_imports(unit)?;
        }
        Ok(())
    }
    pub fn has_deferred_resolution_error(&self, id: ModuleId) -> bool {
        let mut seen = HashSet::new();
        self.has_deferred_resolution_error_inner(id, &mut seen)
    }

    fn has_deferred_resolution_error_inner(
        &self,
        id: ModuleId,
        seen: &mut HashSet<ModuleId>,
    ) -> bool {
        seen.insert(id)
            && (self.resolution_errors.contains_key(&id)
                || self
                    .dependencies(id)
                    .iter()
                    .any(|dep| self.has_deferred_resolution_error_inner(*dep, seen)))
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
        for dependency in self.edges.get(&id).into_iter().flatten() {
            self.visit(*dependency, state, order)?;
        }
        state.insert(id, 2);
        order.push(id);
        Ok(())
    }

    fn add_unit(
        &mut self,
        path: PathBuf,
        source: String,
        kind: ModuleKind,
        entry: bool,
    ) -> ModuleId {
        let path = normalize_module_path(&path);
        if let Some(id) = self.paths.get(&path).copied() {
            return id;
        }
        let id = ModuleId(self.units.len() as u32);
        self.paths.insert(path.clone(), id);
        self.units.push(ModuleUnit {
            id,
            path,
            source,
            bytes: Vec::new(),
            kind,
        });
        if entry {
            self.entry = Some(id);
        }
        id
    }

    fn add_unit_with_bytes(
        &mut self,
        path: PathBuf,
        source: String,
        bytes: Vec<u8>,
        kind: ModuleKind,
        entry: bool,
    ) -> ModuleId {
        let path = normalize_module_path(&path);
        if let Some(id) = self.paths.get(&path).copied() {
            return id;
        }
        let id = ModuleId(self.units.len() as u32);
        self.paths.insert(path.clone(), id);
        self.units.push(ModuleUnit {
            id,
            path,
            source,
            bytes,
            kind,
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
    fn graph_rejects_unresolved_static_imports() {
        let mut graph = ModuleGraph::new();
        let entry = graph.add_entry(
            PathBuf::from("test/entry.js"),
            "import './missing.js';".to_string(),
        );
        let error = graph.link_unit_imports(entry).expect_err("missing import");
        assert_eq!(
            error,
            "unresolved static module specifier `./missing.js` from `test/entry.js`"
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
