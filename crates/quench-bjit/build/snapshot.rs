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
    pub fn changed_functions(&self, paths: &BTreeSet<PathBuf>) -> Option<BTreeSet<String>> {
        let mut names = BTreeSet::new();
        for path in paths.iter().filter(|path| path.is_file()) {
            let now = fs::read(path).ok()?;
            let was = self.files.get(path)?;
            if *was != now {
                let (was, now) = (std::str::from_utf8(was).ok()?, std::str::from_utf8(&now).ok()?);
                names.extend(crate::edits::changed_functions(was, now)?);
            }
        }
        Some(names)
    }
}
