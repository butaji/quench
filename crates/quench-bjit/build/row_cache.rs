//! The row cache: each generated row's finished table entry under its key (`ir_hash`), and the row keys an IR job
//! read from its shard. An entry is two flags and the row's table line; a key names the entry, so an entry is never
//! rewritten, only removed once no row of the build has its key.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

fn entry_path(dir: &Path, key: u128) -> PathBuf {
    dir.join(format!("{key:032x}.row"))
}

/// The entry under `key`: whether its row keeps a stack frame, whether its hot path is unchained, and its table line.
pub fn read_entry(dir: &Path, key: u128) -> Option<(bool, bool, String)> {
    let text = fs::read_to_string(entry_path(dir, key)).ok()?;
    let (flags, line) = text.split_once('\n')?;
    let (framed, unchained) = flags.split_once(' ')?;
    Some((framed == "1", unchained == "1", line.to_string()))
}

pub fn write_entry(dir: &Path, key: u128, framed: bool, unchained: bool, line: &str) {
    fs::write(entry_path(dir, key), format!("{} {}\n{line}", u8::from(framed), u8::from(unchained))).unwrap();
}

/// Removes every entry whose key is not in `keep`.
pub fn prune(dir: &Path, keep: &HashSet<u128>) {
    let Ok(listing) = fs::read_dir(dir) else { return };
    for file in listing.flatten() {
        let key = file.file_name().to_str().and_then(|name| name.strip_suffix(".row")).and_then(|hex| u128::from_str_radix(hex, 16).ok());
        if key.is_some_and(|key| !keep.contains(&key)) {
            let _ = fs::remove_file(file.path());
        }
    }
}

/// The rows of an IR job's shard with their keys, one `<key hex> <name>` line each.
pub fn write_keys(path: &Path, keys: &[(String, u128)]) {
    let text: String = keys.iter().map(|(name, key)| format!("{key:032x} {name}\n")).collect();
    fs::write(path, text).unwrap();
}

pub fn read_keys(path: &Path) -> Vec<(String, u128)> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    text.lines()
        .map(|line| {
            let (key, name) = line.split_once(' ').expect("a key line");
            (name.to_string(), u128::from_str_radix(key, 16).expect("a hex key"))
        })
        .collect()
}
