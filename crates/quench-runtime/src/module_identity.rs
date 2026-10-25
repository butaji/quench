use std::{
    borrow::Cow,
    path::{Component, Path, PathBuf},
};

/// Filesystem components and canonical host URL names have different equality
/// rules: a URL query or fragment is opaque, even when it contains /../.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ModuleIdentity {
    File(PathBuf),
    Url(String),
}

impl ModuleIdentity {
    pub(crate) fn text(&self) -> Cow<'_, str> {
        match self {
            Self::File(path) => path.to_string_lossy(),
            Self::Url(url) => Cow::Borrowed(url),
        }
    }
}

fn is_url_name(name: &str) -> bool {
    let Some((scheme, _)) = name.split_once("://") else {
        return false;
    };
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

/// Whether `name` already spells its normalized file identity: on Unix, a name with no empty,
/// `.` or `..` component and no trailing separator, from which `normalize` rebuilds the same
/// text.
fn is_normal_file_name(name: &str) -> bool {
    if !cfg!(unix) {
        return false;
    }
    match name.strip_prefix('/') {
        Some("") => true,
        rest => rest
            .unwrap_or(name)
            .split('/')
            .all(|part| !matches!(part, "" | "." | "..")),
    }
}

/// The text of `name`'s module identity (`normalize(name).text()`), borrowed when `name`
/// already spells it.
pub(crate) fn normalized_text(name: &str) -> Cow<'_, str> {
    if is_url_name(name) || is_normal_file_name(name) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(normalize(Path::new(name)).text().into_owned())
    }
}

pub(crate) fn normalize(path: &Path) -> ModuleIdentity {
    if let Some(name) = path.to_str() {
        if is_url_name(name) {
            return ModuleIdentity::Url(name.to_owned());
        }
        if is_normal_file_name(name) {
            return ModuleIdentity::File(PathBuf::from(name));
        }
    }
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
    ModuleIdentity::File(PathBuf::from_iter(parts))
}

pub(crate) fn same_name(left: &str, right: &str) -> bool {
    normalize(Path::new(left)) == normalize(Path::new(right))
}

pub(crate) fn resolves_to(module_name: &str, specifier: &str) -> bool {
    if is_url_name(module_name) {
        return false;
    }
    let module_path = Path::new(module_name);
    module_path
        .parent()
        .is_some_and(|parent| normalize(&parent.join(specifier)) == normalize(module_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A name `normalized_text` borrows is the one `normalize` rebuilds.
    #[test]
    fn normalized_names_are_their_identity() {
        for name in [
            "/", "/tmp/a.mjs", "a/b.js", "/tmp/./a.mjs", "/tmp/sub/../a.mjs", "/tmp//a.mjs",
            "/tmp/a/", "./a.js", "../a.js", "", "a", "/..", "file:///tmp/a.mjs?x=/../one",
            "node:fs",
        ] {
            assert_eq!(normalized_text(name), normalize(Path::new(name)).text(), "{name:?}");
        }
    }

    #[test]
    fn url_queries_are_opaque_module_identity() {
        let a = normalize(Path::new("file:///tmp/a.mjs?x=/../one"));
        let b = normalize(Path::new("file:///tmp/one"));
        assert_ne!(a, b);
        assert_ne!(a, normalize(Path::new("file:///tmp/a.mjs?x=/one")));
        assert_eq!(a.text(), "file:///tmp/a.mjs?x=/../one");
        assert_eq!(
            normalize(Path::new("/tmp/sub/../a.mjs")),
            normalize(Path::new("/tmp/a.mjs"))
        );
    }
}
