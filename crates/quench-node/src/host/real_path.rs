//! Module paths' real paths, as Node's loaders keep them for the process
//! (`lib/internal/modules/helpers.js` and `lib/internal/modules/esm/resolve.js` `realpathCache`):
//! a map of known-real paths that each resolution extends, so a module's path reads only the
//! components no earlier module shared. Canonicalizing every module path read each component's
//! link again (the typescript-eslint npm scenario: 8,723 `readlink` calls for 817 modules).
//! Only module loading resolves through it; `fs.realpathSync` reads the filesystem.
use std::collections::HashMap;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The symlinks one resolution follows before it leaves the path to the host's resolver, which
/// reports the loop.
const MAX_FOLLOWED_LINKS: u32 = 40;

thread_local! {
    /// Each resolved path component, keyed by its path, mapped to its real path.
    static KNOWN_REAL: std::cell::RefCell<HashMap<PathBuf, PathBuf>> = Default::default();
}

/// `path`'s real path through the known-real paths. A path they cannot resolve, a relative one
/// or one that does not exist, is canonicalized by the host, which reports its error.
pub(crate) fn real_path(path: &Path) -> io::Result<PathBuf> {
    let known = path.is_absolute()
        .then(|| KNOWN_REAL.with(|known| resolve(path, &mut known.borrow_mut(), &mut 0)))
        .flatten();
    match known {
        Some(real) => Ok(real),
        None => std::fs::canonicalize(path),
    }
}

fn resolve(path: &Path, known: &mut HashMap<PathBuf, PathBuf>, followed: &mut u32) -> Option<PathBuf> {
    if let Some(real) = known.get(path) {
        return Some(real.clone());
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir => current.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(name) => {
                let next = current.join(name);
                if let Some(real) = known.get(&next) {
                    current = real.clone();
                    continue;
                }
                let real = if std::fs::symlink_metadata(&next).ok()?.file_type().is_symlink() {
                    *followed += 1;
                    if *followed > MAX_FOLLOWED_LINKS {
                        return None;
                    }
                    let target = std::fs::read_link(&next).ok()?;
                    resolve(&current.join(target), known, followed)?
                } else {
                    next.clone()
                };
                known.insert(next, real.clone());
                current = real;
            }
            Component::Prefix(_) => return None,
        }
    }
    current.is_absolute().then_some(current)
}

#[cfg(test)]
mod tests {
    use super::real_path;

    #[test]
    fn known_real_paths_resolve_as_the_host_does() {
        let root = std::env::temp_dir().join(format!("quench-real-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("real/pkg/lib")).unwrap();
        std::fs::write(root.join("real/pkg/lib/index.js"), "").unwrap();
        std::fs::create_dir_all(root.join("app/node_modules")).unwrap();
        std::os::unix::fs::symlink("../../real/pkg", root.join("app/node_modules/pkg")).unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("alias")).unwrap();
        std::os::unix::fs::symlink("loop-b", root.join("loop-a")).unwrap();
        std::os::unix::fs::symlink("loop-a", root.join("loop-b")).unwrap();
        for path in [
            "app/node_modules/pkg/lib/index.js",
            "app/node_modules/pkg/lib/../lib/./index.js",
            "alias/pkg/lib/index.js",
            "app/node_modules/pkg/lib/index.js",
            "app/node_modules/pkg/missing.js",
            "app/node_modules/pkg/lib/index.js/below",
            "loop-a",
        ] {
            let path = root.join(path);
            let ours = real_path(&path).map_err(|error| error.kind());
            let host = std::fs::canonicalize(&path).map_err(|error| error.kind());
            assert_eq!(ours, host, "{path:?}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
