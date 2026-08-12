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

    pub fn units(&self) -> &[ModuleUnit] {
        &self.units
    }

    pub fn resolve(&self, from: ModuleId, specifier: &str) -> Option<ModuleId> {
        let base = self.units.get(from.0 as usize)?.path.parent()?;
        let path = normalize_module_path(&base.join(specifier));
        self.paths.get(&path).copied()
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
        let entry = graph.add_entry(PathBuf::from("test/entry.js"), "".to_string());
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
        assert_eq!(graph.units().len(), 2);
    }
}
