//! The stencil sources as of the last build that finished: every watched file's content, kept in `OUT_DIR`, so the
//! next build can tell which functions an edit changed (`edits`) and which key each shard object had under the
//! sources it was built from (`cache::sources_key_of`).

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

/// The saved sources by path.
#[derive(Default)]
pub struct Snapshot {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

fn dir(out: &Path) -> PathBuf {
    out.join("stencil_snapshot")
}

impl Snapshot {
    /// The snapshot a finished build saved, or an empty one.
    pub fn load(out: &Path) -> Self {
        let dir = dir(out);
        let Ok(index) = fs::read_to_string(dir.join("index")) else { return Self::default() };
        let mut files = BTreeMap::new();
        for (at, path) in index.lines().enumerate() {
            let Ok(content) = fs::read(dir.join(at.to_string())) else { return Self::default() };
            files.insert(PathBuf::from(path), content);
        }
        Self { files }
    }

    /// Saves the current content of `paths` as the snapshot, replacing the last.
    pub fn save(out: &Path, paths: &BTreeSet<PathBuf>) {
        let dir = dir(out);
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut index = String::new();
        for (at, path) in paths.iter().filter(|path| path.is_file()).enumerate() {
            fs::copy(path, dir.join(at.to_string())).unwrap();
            index += &format!("{}\n", path.display());
        }
        // The index last: a snapshot without its index is no snapshot.
        fs::write(dir.join("index"), index).unwrap();
    }

    pub fn get(&self, path: &Path) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Whether the snapshot holds every one of `paths` (as files).
    pub fn covers(&self, paths: &BTreeSet<PathBuf>) -> bool {
        paths.iter().filter(|path| path.is_file()).all(|path| self.files.contains_key(path))
    }

    /// The functions an edit of `paths` from the snapshot to their current content changed (`edits`): `None` where
    /// a file is new, gone or not valid UTF-8, or an edit is structural.
    ///
    /// A function that is its file's own item is named `module::name` for each module the crate mounts the file as
    /// (`module_names`), which a shard's reach lists as the last two segments of the symbol paths it holds
    /// (`reach::mono_items`); any other function (a method, one inside a `mod` or a macro) by its bare name.
    pub fn changed_functions(&self, paths: &BTreeSet<PathBuf>) -> Option<BTreeSet<String>> {
        let mut names = BTreeSet::new();
        let mut modules = None;
        for path in paths.iter().filter(|path| path.is_file()) {
            let now = fs::read(path).ok()?;
            let was = self.files.get(path)?;
            if *was != now {
                let (was, now) = (std::str::from_utf8(was).ok()?, std::str::from_utf8(&now).ok()?);
                for (name, free) in crate::edits::changed_items(was, now)? {
                    if !free {
                        names.insert(name);
                        continue;
                    }
                    let mounts = modules.get_or_insert_with(|| module_names(paths));
                    for module in mounts.get(&canonical(path)).into_iter().flatten() {
                        names.insert(format!("{module}::{name}"));
                    }
                    // The file's own stem: the module a plain `mod stem;` names, or the crate root's path for
                    // `lib.rs`, `main.rs` and `mod.rs`, whose items carry no such segment.
                    match path.file_stem().and_then(|stem| stem.to_str()) {
                        Some("lib" | "main" | "mod") | None => { names.insert(name); }
                        Some(stem) => { names.insert(format!("{stem}::{name}")); }
                    }
                }
            }
        }
        Some(names)
    }
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The modules each file is mounted as by a `#[path = "..."]` attribute on a `mod NAME;` in one of `paths`, by the
/// file's canonical path. A path attribute is resolved against the directory of the file that holds it.
fn module_names(paths: &BTreeSet<PathBuf>) -> BTreeMap<PathBuf, BTreeSet<String>> {
    let mut mounts: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    for path in paths {
        let Ok(text) = fs::read_to_string(path) else { continue };
        let base = path.parent().unwrap_or(Path::new("."));
        let mut rest = text.as_str();
        while let Some(at) = rest.find("#[path") {
            rest = &rest[at + 6..];
            let Some(open) = rest.find('"') else { break };
            let Some(close) = rest[open + 1..].find('"') else { break };
            let target = &rest[open + 1..open + 1 + close];
            rest = &rest[open + 1 + close..];
            // The `mod NAME` the attribute (and any beside it) applies to.
            let Some(at) = rest.find("mod ") else { break };
            let name: String = rest[at + 4..].trim_start().chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                mounts.entry(canonical(&base.join(target))).or_default().insert(name);
            }
        }
    }
    mounts
}
