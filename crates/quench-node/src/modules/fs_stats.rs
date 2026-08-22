//! `fs.Stats` and `fs.Dirent` values: real metadata plus the
//! `isFile()`/`isDirectory()`/... predicate family, implemented as
//! host capabilities that read a hidden mode slot on the receiver.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

const MODE_KEY: &str = "\0quench:fs:mode";

const S_IFMT: u32 = 0o170000;
const S_IFSOCK: u32 = 0o140000;
const S_IFLNK: u32 = 0o120000;
const S_IFREG: u32 = 0o100000;
const S_IFBLK: u32 = 0o60000;
const S_IFDIR: u32 = 0o40000;
const S_IFCHR: u32 = 0o20000;
const S_IFIFO: u32 = 0o10000;

fn predicates() -> Vec<(&'static str, u16)> {
    use crate::registry::*;
    vec![
        ("isFile", SPEC_FS_STAT_ISFILE.cap),
        ("isDirectory", SPEC_FS_STAT_ISDIR.cap),
        ("isSymbolicLink", SPEC_FS_STAT_ISSYMLINK.cap),
        ("isBlockDevice", SPEC_FS_STAT_ISBLOCK.cap),
        ("isCharacterDevice", SPEC_FS_STAT_ISCHAR.cap),
        ("isFIFO", SPEC_FS_STAT_ISFIFO.cap),
        ("isSocket", SPEC_FS_STAT_ISSOCKET.cap),
    ]
}

fn with_predicates(mode: u32, mut entries: Vec<(String, Value)>) -> Value {
    for (name, cap) in predicates() {
        entries.push((
            name.to_string(),
            quench_runtime::host_api::custom_function(quench_runtime::ops::RealmId::ROOT, cap),
        ));
    }
    entries.push((MODE_KEY.to_string(), Value::Number(mode as f64)));
    host_api::object(entries)
}

/// A `Dirent` for `readdir` with `withFileTypes`.
pub fn dirent(name: &str, mode: u32) -> Value {
    with_predicates(
        mode,
        vec![("name".to_string(), Value::String(name.to_string()))],
    )
}

/// Build a `Stats` from constructor arguments (deprecated `fs.Stats`).
/// Numeric fields are passed through; the four Date fields are given as
/// epoch-millisecond numbers, matching Node's Stats constructor.
pub fn stats_from_values(
    dev: f64,
    mode: f64,
    nlink: f64,
    uid: f64,
    gid: f64,
    rdev: f64,
    blksize: f64,
    ino: f64,
    size: f64,
    blocks: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
) -> Value {
    with_predicates(
        mode as u32,
        stats_entries(
            dev,
            mode,
            nlink,
            uid,
            gid,
            rdev,
            blksize,
            ino,
            size,
            blocks,
            atime_ms,
            mtime_ms,
            ctime_ms,
            birthtime_ms,
        ),
    )
}

fn stats_entries(
    dev: f64,
    mode: f64,
    nlink: f64,
    uid: f64,
    gid: f64,
    rdev: f64,
    blksize: f64,
    ino: f64,
    size: f64,
    blocks: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
) -> Vec<(String, Value)> {
    let mut entries = stats_numeric_entries(
        dev,
        mode,
        nlink,
        uid,
        gid,
        rdev,
        blksize,
        ino,
        size,
        blocks,
        atime_ms,
        mtime_ms,
        ctime_ms,
        birthtime_ms,
    );
    entries.extend(stats_date_entries(
        atime_ms,
        mtime_ms,
        ctime_ms,
        birthtime_ms,
    ));
    entries
}

fn stats_numeric_entries(
    dev: f64,
    mode: f64,
    nlink: f64,
    uid: f64,
    gid: f64,
    rdev: f64,
    blksize: f64,
    ino: f64,
    size: f64,
    blocks: f64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
) -> Vec<(String, Value)> {
    vec![
        ("dev".to_string(), Value::Number(dev)),
        ("mode".to_string(), Value::Number(mode)),
        ("nlink".to_string(), Value::Number(nlink)),
        ("uid".to_string(), Value::Number(uid)),
        ("gid".to_string(), Value::Number(gid)),
        ("rdev".to_string(), Value::Number(rdev)),
        ("blksize".to_string(), Value::Number(blksize)),
        ("ino".to_string(), Value::Number(ino)),
        ("size".to_string(), Value::Number(size)),
        ("blocks".to_string(), Value::Number(blocks)),
        ("atimeMs".to_string(), Value::Number(atime_ms)),
        ("mtimeMs".to_string(), Value::Number(mtime_ms)),
        ("ctimeMs".to_string(), Value::Number(ctime_ms)),
        ("birthtimeMs".to_string(), Value::Number(birthtime_ms)),
    ]
}

fn stats_date_entries(
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
) -> [(String, Value); 4] {
    [
        (
            "atime".to_string(),
            quench_runtime::date::instance(atime_ms),
        ),
        (
            "mtime".to_string(),
            quench_runtime::date::instance(mtime_ms),
        ),
        (
            "ctime".to_string(),
            quench_runtime::date::instance(ctime_ms),
        ),
        (
            "birthtime".to_string(),
            quench_runtime::date::instance(birthtime_ms),
        ),
    ]
}

/// A `Stats` built from real filesystem metadata.
pub fn stats(meta: &std::fs::Metadata) -> Value {
    #[cfg(unix)]
    {
        stats_unix(meta)
    }
    #[cfg(not(unix))]
    {
        with_predicates(
            mode_fallback(meta),
            vec![
                ("size".to_string(), Value::Number(meta.len() as f64)),
                ("mtime".to_string(), quench_runtime::date::instance(0.0)),
                ("atime".to_string(), quench_runtime::date::instance(0.0)),
                ("ctime".to_string(), quench_runtime::date::instance(0.0)),
                ("birthtime".to_string(), quench_runtime::date::instance(0.0)),
            ],
        )
    }
}

#[cfg(unix)]
fn stats_unix(meta: &std::fs::Metadata) -> Value {
    use std::os::unix::fs::MetadataExt;
    with_predicates(meta.mode(), stats_unix_entries(meta))
}

#[cfg(unix)]
fn stats_unix_entries(meta: &std::fs::Metadata) -> Vec<(String, Value)> {
    use std::os::unix::fs::MetadataExt;
    let atime_ms = ms(meta.atime(), meta.atime_nsec());
    let mtime_ms = ms(meta.mtime(), meta.mtime_nsec());
    let ctime_ms = ms(meta.ctime(), meta.ctime_nsec());
    let birthtime_ms = created_ms(meta);
    let mut entries = stats_unix_numeric_entries(
        meta.dev(),
        meta.ino(),
        meta.mode(),
        meta.nlink(),
        meta.uid(),
        meta.gid(),
        meta.rdev(),
        meta.size(),
        meta.blksize(),
        meta.blocks(),
        atime_ms,
        mtime_ms,
        ctime_ms,
        birthtime_ms,
    );
    entries.extend(stats_date_entries(
        atime_ms,
        mtime_ms,
        ctime_ms,
        birthtime_ms,
    ));
    entries
}

#[cfg(unix)]
fn stats_unix_numeric_entries(
    dev: u64,
    ino: u64,
    mode: u32,
    nlink: u64,
    uid: u32,
    gid: u32,
    rdev: u64,
    size: u64,
    blksize: u64,
    blocks: u64,
    atime_ms: f64,
    mtime_ms: f64,
    ctime_ms: f64,
    birthtime_ms: f64,
) -> Vec<(String, Value)> {
    vec![
        ("dev".to_string(), Value::Number(dev as f64)),
        ("ino".to_string(), Value::Number(ino as f64)),
        ("mode".to_string(), Value::Number(mode as f64)),
        ("nlink".to_string(), Value::Number(nlink as f64)),
        ("uid".to_string(), Value::Number(uid as f64)),
        ("gid".to_string(), Value::Number(gid as f64)),
        ("rdev".to_string(), Value::Number(rdev as f64)),
        ("size".to_string(), Value::Number(size as f64)),
        ("blksize".to_string(), Value::Number(blksize as f64)),
        ("blocks".to_string(), Value::Number(blocks as f64)),
        ("atimeMs".to_string(), Value::Number(atime_ms)),
        ("mtimeMs".to_string(), Value::Number(mtime_ms)),
        ("ctimeMs".to_string(), Value::Number(ctime_ms)),
        ("birthtimeMs".to_string(), Value::Number(birthtime_ms)),
    ]
}

#[cfg(unix)]
fn ms(sec: i64, nsec: i64) -> f64 {
    sec as f64 * 1000.0 + nsec as f64 / 1_000_000.0
}

#[cfg(unix)]
fn created_ms(meta: &std::fs::Metadata) -> f64 {
    meta.created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as f64 * 1000.0 + d.subsec_nanos() as f64 / 1_000_000.0)
        .unwrap_or(0.0)
}

/// Mode bits from a `std::fs::FileType` (dirents and non-unix stat).
pub fn mode_of(file_type: &std::fs::FileType) -> u32 {
    if file_type.is_dir() {
        S_IFDIR
    } else if file_type.is_symlink() {
        S_IFLNK
    } else {
        S_IFREG
    }
}

#[cfg(not(unix))]
fn mode_fallback(meta: &std::fs::Metadata) -> u32 {
    mode_of(&meta.file_type())
}

/// Handler for the predicate family: reads the receiver's mode slot.
pub fn predicate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    kind: u32,
) -> Result<Value, VmError> {
    let mode = receiver
        .map(|r| quench_runtime::vm::get_property(r, MODE_KEY))
        .and_then(|v| match v {
            Value::Number(n) => Some(n as u32),
            _ => None,
        })
        .unwrap_or(0);
    Ok(Value::Boolean(mode & S_IFMT == kind))
}

macro_rules! predicate_op {
    ($func:ident, $kind:expr) => {
        pub fn $func(
            state: &Rc<RefCell<HostState>>,
            receiver: Option<&Value>,
            _args: &[Value],
        ) -> Result<Value, VmError> {
            predicate(state, receiver, $kind)
        }
    };
}

predicate_op!(is_file, S_IFREG);
predicate_op!(is_dir, S_IFDIR);
predicate_op!(is_symlink, S_IFLNK);
predicate_op!(is_block, S_IFBLK);
predicate_op!(is_char, S_IFCHR);
predicate_op!(is_fifo, S_IFIFO);
predicate_op!(is_socket, S_IFSOCK);
