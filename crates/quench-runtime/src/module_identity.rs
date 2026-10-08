use std::path::{Component, Path, PathBuf};

pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if parts.last().is_some_and(|part| part != "..") => {
                parts.pop();
            }
            Component::ParentDir => parts.push("..".into()),
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                parts.push(component.as_os_str().to_owned());
            }
        }
    }
    PathBuf::from_iter(parts)
}

pub(crate) fn same_name(left: &str, right: &str) -> bool {
    normalize(Path::new(left)) == normalize(Path::new(right))
}

pub(crate) fn resolves_to(module_name: &str, specifier: &str) -> bool {
    let module_path = Path::new(module_name);
    module_path
        .parent()
        .is_some_and(|parent| normalize(&parent.join(specifier)) == normalize(module_path))
}
