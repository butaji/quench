//! Immutable, pinned Node library input for the shared JavaScript interpreter.
use std::path::{Component, Path, PathBuf};
include!(concat!(env!("OUT_DIR"), "/node_sources.rs"));
include!("node_source_dependencies.rs");

pub(crate) fn resolve(path: &Path, root: &Path) -> Option<(PathBuf, &'static str)> {
    let relative = path.strip_prefix(root).ok()?;
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => normalized.push(name),
            Component::CurDir => (),
            Component::ParentDir if normalized.pop() => (),
            _ => return None,
        }
    }
    let relative = normalized.to_str()?.replace('\\', "/");
    let index = SOURCES
        .binary_search_by_key(&relative.as_str(), |(name, _)| *name)
        .ok()?;
    Some((root.join(SOURCES[index].0), SOURCES[index].1))
}

pub(crate) fn source(path: &Path, root: &Path) -> Option<&'static str> {
    resolve(path, root).map(|(_, source)| source)
}
