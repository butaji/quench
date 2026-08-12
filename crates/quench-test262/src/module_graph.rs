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
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ModuleGraph {
    entry: Option<ModuleId>,
    units: Vec<ModuleUnit>,
    paths: HashMap<PathBuf, ModuleId>,
    edges: HashMap<ModuleId, Vec<ModuleId>>,
}

impl ModuleGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_entry(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, true)
    }

    pub fn add_dependency(&mut self, path: PathBuf, source: String) -> ModuleId {
        self.add_unit(path, source, false)
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

    pub fn resolve(&self, from: ModuleId, specifier: &str) -> Option<ModuleId> {
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
        let source = self
            .unit(from)
            .ok_or_else(|| "module unit is unknown".to_string())?
            .source
            .clone();
        let metadata = quench_runtime::reduce::inspect_module_source(&source)
            .map_err(|errors| errors.join("; "))?;
        self.link_specifiers(from, metadata.import_specifiers.iter().map(String::as_str))
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

    fn add_unit(&mut self, path: PathBuf, source: String, entry: bool) -> ModuleId {
        let path = normalize_module_path(&path);
        if let Some(id) = self.paths.get(&path).copied() {
            return id;
        }
        let id = ModuleId(self.units.len() as u32);
        self.paths.insert(path.clone(), id);
        self.units.push(ModuleUnit { id, path, source });
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
        graph.link_unit_imports(entry).expect("known edge");
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
