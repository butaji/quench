//! The keys that let a build-script rerun skip a shard's rustc: a shard object is reused when the key written beside
//! it (`rows_<cpu>_<shard>.key`) equals the key of the inputs it would be built from now. The key covers the
//! toolchain (rustc's path and its `-vV`), the target, every argument of the shard's command (cpu, cfgs, the flags
//! file's flags), the shard's instance file, the call-rows file, and the contents of every watched source. Cargo
//! reruns the script on any watched change, a merge or a branch switch; the shards whose inputs are byte for byte
//! the same are not compiled again, and their rows' checks still run on every read.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// FNV-1a over 128 bits: a stable hash (the standard library's is not promised across releases), wide enough that
/// two different inputs never share a key in practice.
#[derive(Clone, Copy)]
struct Fnv(u128);

impl Fnv {
    const OFFSET: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;

    fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 ^ u128::from(*byte)).wrapping_mul(Self::PRIME);
        }
    }

    /// `bytes` with its length first, so the concatenation of two fields is not ambiguous.
    fn field(&mut self, bytes: &[u8]) {
        self.bytes(&(bytes.len() as u64).to_le_bytes());
        self.bytes(bytes);
    }
}

/// The key of everything every shard shares: the toolchain and the contents of the watched sources (`watched`, the
/// `#[path]` graph, files and directories; a directory's files are themselves in it) and `extra`, relative to
/// `crate_dir`.
pub fn sources_key(rustc: &Path, crate_dir: &Path, watched: &BTreeSet<PathBuf>, extra: &[&str]) -> u128 {
    sources_key_of(rustc, crate_dir, watched, extra, |file| fs::read(file).unwrap_or_else(|e| panic!("read {}: {e}", file.display())))
}

/// `sources_key` with each file's content from `read` (a snapshot's, where the files are what they were), for the
/// files that exist now.
pub fn sources_key_of(rustc: &Path, crate_dir: &Path, watched: &BTreeSet<PathBuf>, extra: &[&str], read: impl Fn(&Path) -> Vec<u8>) -> u128 {
    let mut hash = Fnv::new();
    hash.field(rustc.as_os_str().as_encoded_bytes());
    let version = Command::new(rustc).arg("-vV").output().map(|output| output.stdout).unwrap_or_default();
    hash.field(&version);
    hash.field(std::env::var("TARGET").unwrap_or_default().as_bytes());
    let files: BTreeSet<PathBuf> = watched.iter().cloned().chain(extra.iter().map(|path| crate_dir.join(path))).filter(|path| path.is_file()).collect();
    for file in files {
        hash.field(file.strip_prefix(crate_dir).unwrap_or(&file).as_os_str().as_encoded_bytes());
        hash.field(&read(&file));
    }
    hash.0
}

/// The key of one shard's object: the shared `sources` key, the shard command's arguments, and the contents of the
/// generated files it includes, as hex.
pub fn shard_key(sources: u128, args: &[String], inputs: &[&Path]) -> String {
    let mut hash = Fnv(sources);
    for arg in args {
        hash.field(arg.as_bytes());
    }
    for input in inputs {
        hash.field(&fs::read(input).unwrap_or_else(|e| panic!("read {}: {e}", input.display())));
    }
    format!("{:032x}\n", hash.0)
}
