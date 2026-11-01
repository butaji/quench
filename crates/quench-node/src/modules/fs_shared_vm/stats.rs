//! `fs.Stats`, `BigIntStats` and `StatFs` (`internal/fs/utils.js`): the kernel's integer
//! fields and nanosecond times are the one record each view projects.

use super::classes::{self, Class};
use crate::modules::stream_shared_vm::{get, Context, Outcome};
use quench_runtime::{NativeArithmetic, RootId, RootedError};
use std::time::{SystemTime, UNIX_EPOCH};

/// The integer fields every stat view carries, in Node's order (`StatsBase`).
pub(crate) const BASE_FIELDS: [&str; 10] = ["dev", "mode", "nlink", "uid", "gid", "rdev", "blksize", "ino", "size", "blocks"];
/// The time fields, each with `<name>Ms` (and `<name>Ns` for BigInt) members.
pub(crate) const TIME_FIELDS: [&str; 4] = ["atime", "mtime", "ctime", "birthtime"];
const TIME_MS_FIELDS: [&str; 4] = ["atimeMs", "mtimeMs", "ctimeMs", "birthtimeMs"];
const TIME_NS_FIELDS: [&str; 4] = ["atimeNs", "mtimeNs", "ctimeNs", "birthtimeNs"];
/// `StatFs`'s fields, in Node's order.
pub(crate) const STATFS_FIELDS: [&str; 7] = ["type", "bsize", "blocks", "bfree", "bavail", "files", "ffree"];

const NANOSECONDS_PER_SECOND: i128 = 1_000_000_000;
const NANOSECONDS_PER_MILLISECOND: i128 = 1_000_000;

/// The predicates `StatsBase.prototype` and `Dirent.prototype` share, with the `S_IFMT` type
/// each tests and the `UV_DIRENT_*` type a directory entry carries.
pub(crate) const TYPE_PREDICATES: [(&str, u32, i32); 7] = [
    ("isDirectory", libc::S_IFDIR as u32, super::UV_DIRENT_DIR),
    ("isFile", libc::S_IFREG as u32, super::UV_DIRENT_FILE),
    ("isBlockDevice", libc::S_IFBLK as u32, super::UV_DIRENT_BLOCK),
    ("isCharacterDevice", libc::S_IFCHR as u32, super::UV_DIRENT_CHAR),
    ("isSymbolicLink", libc::S_IFLNK as u32, super::UV_DIRENT_LINK),
    ("isFIFO", libc::S_IFIFO as u32, super::UV_DIRENT_FIFO),
    ("isSocket", libc::S_IFSOCK as u32, super::UV_DIRENT_SOCKET),
];

/// A stat result: the kernel's integer fields and its times in nanoseconds.
#[derive(Clone, Default)]
pub(crate) struct StatRecord {
    fields: [i128; BASE_FIELDS.len()],
    times: [i128; TIME_FIELDS.len()],
}

impl StatRecord {
    /// The record of `metadata`, or the zero record Node reports for a missing file.
    pub(crate) fn of(metadata: Option<&std::fs::Metadata>) -> Self {
        let Some(metadata) = metadata else {
            return Self::default();
        };
        use std::os::unix::fs::MetadataExt;
        let time = |seconds: i64, nanoseconds: i64| i128::from(seconds) * NANOSECONDS_PER_SECOND + i128::from(nanoseconds);
        let birth = metadata.created().ok().map_or(0, exact_time_ns);
        Self {
            fields: [
                i128::from(metadata.dev()),
                i128::from(metadata.mode()),
                i128::from(metadata.nlink()),
                i128::from(metadata.uid()),
                i128::from(metadata.gid()),
                i128::from(metadata.rdev()),
                i128::from(metadata.blksize()),
                i128::from(metadata.ino()),
                i128::from(metadata.len()),
                i128::from(metadata.blocks()),
            ],
            times: [
                time(metadata.atime(), metadata.atime_nsec()),
                time(metadata.mtime(), metadata.mtime_nsec()),
                time(metadata.ctime(), metadata.ctime_nsec()),
                birth,
            ],
        }
    }
}

fn exact_time_ns(time: SystemTime) -> i128 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i128,
        Err(error) => -(error.duration().as_nanos() as i128),
    }
}

/// Node's millisecond time: `seconds * 1e3 + nanoseconds / 1e6`.
fn milliseconds(time: i128) -> f64 {
    let seconds = time.div_euclid(NANOSECONDS_PER_SECOND) as f64;
    let nanoseconds = time.rem_euclid(NANOSECONDS_PER_SECOND) as f64;
    seconds * 1e3 + nanoseconds / NANOSECONDS_PER_MILLISECOND as f64
}

fn define(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true).map(drop)
}

/// The `Stats` (or `BigIntStats`) object of `record`.
pub(crate) fn stats(c: &mut Context<'_>, record: &StatRecord, bigint: bool) -> Outcome {
    let class = if bigint { Class::BigIntStats } else { Class::Stats };
    let object = classes::instance(c, class)?;
    for (name, value) in BASE_FIELDS.into_iter().zip(record.fields) {
        let value = if bigint { c.bigint_rooted(value) } else { c.number(value as f64) };
        define(c, object, name, value)?;
    }
    for (name, time) in TIME_MS_FIELDS.into_iter().zip(record.times) {
        let value = if bigint {
            c.bigint_rooted(time.div_euclid(NANOSECONDS_PER_MILLISECOND))
        } else {
            c.number(milliseconds(time))
        };
        define(c, object, name, value)?;
    }
    if bigint {
        for (name, time) in TIME_NS_FIELDS.into_iter().zip(record.times) {
            let value = c.bigint_rooted(time);
            define(c, object, name, value)?;
        }
    }
    Ok(object)
}

/// The `StatFs` object of `statfs(2)`'s fields.
pub(crate) fn statfs(c: &mut Context<'_>, values: [i128; STATFS_FIELDS.len()], bigint: bool) -> Outcome {
    let object = classes::instance(c, Class::StatFs)?;
    for (name, value) in STATFS_FIELDS.into_iter().zip(values) {
        let value = if bigint { c.bigint_rooted(value) } else { c.number(value as f64) };
        define(c, object, name, value)?;
    }
    Ok(object)
}

/// Assign `values` to `names` of `this` as a constructor does (`this.name = value`).
fn assign(c: &mut Context<'_>, this: RootId, names: &[&str], args: &[RootId]) -> Result<(), RootedError> {
    for (index, name) in names.iter().enumerate() {
        let value = crate::modules::stream_shared_vm::arg(c, args, index);
        crate::modules::stream_shared_vm::put(c, this, name, value)?;
    }
    Ok(())
}

/// `function StatsBase(dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks)`.
pub(crate) fn construct_base(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    assign(c, this, &BASE_FIELDS, args)?;
    Ok(c.undefined())
}

/// `function Stats(...base, atimeMs, mtimeMs, ctimeMs, birthtimeMs)`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    assign(c, this, &BASE_FIELDS, args)?;
    assign(c, this, &TIME_MS_FIELDS, args.get(BASE_FIELDS.len()..).unwrap_or_default())?;
    Ok(c.undefined())
}

/// `fs.Stats`: `deprecate(Stats, 'fs.Stats constructor is deprecated.', 'DEP0180')`.
pub(crate) fn construct_deprecated(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let shared = c.host_mut().shared_state();
    if !shared.borrow().fs_classes.stats_warning_emitted {
        shared.borrow_mut().fs_classes.stats_warning_emitted = true;
        let deprecated = classes::root(c, Class::DeprecatedStats)?;
        super::emit_deprecation(c, "fs.Stats constructor is deprecated.", "DEP0180", deprecated)?;
    }
    construct(c, this, args)
}

/// `function BigIntStats(...base, atimeNs, mtimeNs, ctimeNs, birthtimeNs)`.
pub(crate) fn construct_bigint(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    assign(c, this, &BASE_FIELDS, args)?;
    let divisor = c.bigint_rooted(NANOSECONDS_PER_MILLISECOND);
    for (index, (ms, ns)) in TIME_MS_FIELDS.into_iter().zip(TIME_NS_FIELDS).enumerate() {
        let time = crate::modules::stream_shared_vm::arg(c, args, BASE_FIELDS.len() + index);
        let milliseconds = c.arithmetic_rooted(NativeArithmetic::Divide, time, divisor)?;
        crate::modules::stream_shared_vm::put(c, this, ms, milliseconds)?;
        crate::modules::stream_shared_vm::put(c, this, ns, time)?;
    }
    Ok(c.undefined())
}

/// `StatsBase.prototype.isDirectory()` and its siblings: `this._checkModeProperty(S_IF*)`,
/// the mode type the function's data holds.
pub(crate) fn is_type(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let property = c.host_function_data()?;
    crate::modules::stream_shared_vm::call_method(c, this, "_checkModeProperty", &[property])
}

/// `Stats.prototype._checkModeProperty(property)`: `(this.mode & S_IFMT) === property`,
/// over BigInts for `BigIntStats` (the function's data says which).
pub(crate) fn check_mode(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let bigint = c.host_function_data()?;
    let bigint = c.truthy_rooted(bigint)?;
    let property = crate::modules::stream_shared_vm::arg(c, args, 0);
    let mode = get(c, this, "mode")?;
    let (mask, property) = if bigint {
        let property = c.to_number(property)?;
        (c.bigint_rooted(i128::from(libc::S_IFMT)), c.bigint_rooted(property as i128))
    } else {
        (c.number(f64::from(libc::S_IFMT)), property)
    };
    let actual = c.arithmetic_rooted(NativeArithmetic::BitwiseAnd, mode, mask)?;
    let equal = c.strict_equal_rooted(actual, property)?;
    Ok(c.boolean(equal))
}

/// The time field an accessor's data names.
fn time_field(c: &mut Context<'_>) -> Result<&'static str, RootedError> {
    let index = c.host_function_data()?;
    let index = c.to_number(index)? as usize;
    TIME_FIELDS.get(index).copied().ok_or_else(|| RootedError::host("unknown stat time field"))
}

/// `ObjectDefineProperty(this, name, { value, writable: true })`.
fn define_time(c: &mut Context<'_>, this: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    let fields = quench_runtime::DescriptorFields {
        value: Some(value),
        writable: Some(true),
        ..quench_runtime::DescriptorFields::default()
    };
    c.define_property_fields_rooted(this, key, fields).map(drop)
}

/// `get atime()`: `new Date(Math.round(Number(this.atimeMs)))`, then an own data property.
pub(crate) fn time_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let name = time_field(c)?;
    let milliseconds = get(c, this, &format!("{name}Ms"))?;
    let milliseconds = c.to_number(milliseconds)?;
    let date = classes::root(c, Class::Date)?;
    let time = c.number(round_half_up(milliseconds));
    let value = c.construct_rooted(date, date, &[time])?;
    define_time(c, this, name, value)?;
    Ok(value)
}

/// `Math.round`: halves round toward positive infinity.
fn round_half_up(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `set atime(value)`: an own data property.
pub(crate) fn time_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let name = time_field(c)?;
    let value = crate::modules::stream_shared_vm::arg(c, args, 0);
    define_time(c, this, name, value)?;
    Ok(c.undefined())
}

/// `class StatFs { constructor(type, bsize, blocks, bfree, bavail, files, ffree) }`.
pub(crate) fn construct_statfs(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    classes::require_new(c, "StatFs")?;
    assign(c, this, &STATFS_FIELDS, args)?;
    Ok(c.undefined())
}
