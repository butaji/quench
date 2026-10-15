//! Stat object presentation is separate from filesystem polling ownership.
use super::{MILLISECONDS_PER_SECOND, NANOSECONDS_PER_MILLISECOND};
use crate::host::NodeHost;
use quench_runtime::{NativeArithmetic, NativeContext, RootId, RootedError};
pub(crate) const BASE_FIELDS: [&str; 10] = [
    "dev", "mode", "nlink", "uid", "gid", "rdev", "blksize", "ino", "size", "blocks",
];
pub(crate) const TIME_FIELDS: [&str; 4] = ["atime", "mtime", "ctime", "birthtime"];
#[derive(Default)]
pub(crate) struct State {
    api: Option<RootId>,
    stats_warning_emitted: bool,
}
const FACTORY: &str = quench_js_check::checked_js!(
    r#"(statsConstruct, mode, checkMode, date, DateConstructor, NumberConstructor, round, emitWarning) => {
 'use strict';
 function StatsBase(dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks) {}
 StatsBase.prototype.isDirectory = function() { return mode(this, 'isDirectory'); };
 StatsBase.prototype.isFile = function() { return mode(this, 'isFile'); };
 StatsBase.prototype.isBlockDevice = function() { return mode(this, 'isBlockDevice'); };
 StatsBase.prototype.isCharacterDevice = function() { return mode(this, 'isCharacterDevice'); };
 StatsBase.prototype.isSymbolicLink = function() { return mode(this, 'isSymbolicLink'); };
 StatsBase.prototype.isFIFO = function() { return mode(this, 'isFIFO'); };
 StatsBase.prototype.isSocket = function() { return mode(this, 'isSocket'); };
 function Stats(dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atimeS, atimeNs, mtimeS, mtimeNs, ctimeS, ctimeNs, birthtimeS, birthtimeNs) {
   return statsConstruct(this, arguments, false, false);
 }
 function deprecated(dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atimeS, atimeNs, mtimeS, mtimeNs, ctimeS, ctimeNs, birthtimeS, birthtimeNs) {
   return statsConstruct(this, arguments, false, true);
 }
 function BigIntStats(dev, mode, nlink, uid, gid, rdev, blksize, ino, size, blocks, atimeNs, mtimeNs, ctimeNs, birthtimeNs) {
   return statsConstruct(this, arguments, true);
 }
 Object.setPrototypeOf(Stats.prototype, StatsBase.prototype);
 Object.setPrototypeOf(Stats, StatsBase);
 deprecated.prototype = Stats.prototype;
 Object.setPrototypeOf(deprecated, Stats);
 Object.setPrototypeOf(BigIntStats.prototype, StatsBase.prototype);
 Object.setPrototypeOf(BigIntStats, StatsBase);
 Stats.prototype._checkModeProperty = function(property) { return checkMode(this, property, false); };
 BigIntStats.prototype._checkModeProperty = function(property) { return checkMode(this, property, true); };
 Object.defineProperties(Stats.prototype, {
   atime: { enumerable: true, configurable: true, get() { return date(this, 'atime'); }, set(value) { Object.defineProperty(this, 'atime', { value, writable: true, configurable: true, enumerable: true }); } },
   mtime: { enumerable: true, configurable: true, get() { return date(this, 'mtime'); }, set(value) { Object.defineProperty(this, 'mtime', { value, writable: true, configurable: true, enumerable: true }); } },
   ctime: { enumerable: true, configurable: true, get() { return date(this, 'ctime'); }, set(value) { Object.defineProperty(this, 'ctime', { value, writable: true, configurable: true, enumerable: true }); } },
   birthtime: { enumerable: true, configurable: true, get() { return date(this, 'birthtime'); }, set(value) { Object.defineProperty(this, 'birthtime', { value, writable: true, configurable: true, enumerable: true }); } }
 });
 for (const name of ['atime', 'mtime', 'ctime', 'birthtime']) Object.defineProperty(BigIntStats.prototype, name, Object.getOwnPropertyDescriptor(Stats.prototype, name));
 Object.defineProperty(BigIntStats.prototype, 'constructor', { value: BigIntStats, writable: true, configurable: true });
 return { Stats: deprecated, BigIntStats, DateConstructor, NumberConstructor, round, emitWarning };
}"#
);
fn get(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let key = context.string_rooted(name);
    context.get_property_rooted(object, key)
}
fn set(
    context: &mut NativeContext<'_, NodeHost>,
    object: RootId,
    name: &str,
    value: RootId,
) -> Result<(), RootedError> {
    let key = context.string_rooted(name);
    context.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

pub(crate) fn install(
    context: &mut NativeContext<'_, NodeHost>,
    fs: RootId,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let mut arguments = Vec::new();
    for name in [
        "fsStatsConstruct",
        "fsStatsMode",
        "fsStatsCheckMode",
        "fsStatsDate",
    ] {
        arguments.push(context.host_function(crate::host::shared_vm::operation(name))?);
    }
    arguments.push(get(context, global, "Date")?);
    arguments.push(get(context, global, "Number")?);
    let math = get(context, global, "Math")?;
    arguments.push(get(context, math, "round")?);
    let process = get(context, global, "process")?;
    arguments.push(get(context, process, "emitWarning")?);
    let factory = context.evaluate_script_rooted(FACTORY, "node:fs/stat-types-facade.js")?;
    let undefined = context.undefined();
    let api = context.call_rooted(factory, undefined, &arguments)?;
    let shared = context.host_mut().shared_state();
    let canonical = crate::modules::fs_policy_shared_vm::cached(context, "BigIntStats")?;
    if let Some(constructor) = canonical {
        set(context, api, "BigIntStats", constructor)?;
    }
    for name in ["Atime", "Mtime", "Ctime", "Birthtime"] {
        let description = if name == "Birthtime" {
            "kPartialBirthtimeNs".to_owned()
        } else {
            format!("partial{name}Ns")
        };
        let symbol = context.symbol_rooted(Some(&description));
        set(context, api, &format!("partial{name}"), symbol)?;
    }
    let stats = get(context, api, "Stats")?;
    set(context, fs, "Stats", stats)?;
    let api = context.retain(api)?;
    shared.borrow_mut().fs_stats.api = Some(api);
    Ok(())
}
/// Internal fs utilities export the same public stat type used by their
/// binding projection. Preserve that identity when the module is available.
pub(crate) fn adopt_bigint_stats(
    context: &mut NativeContext<'_, NodeHost>,
    constructor: RootId,
) -> Result<(), RootedError> {
    let api = context.host_mut().shared_state().borrow().fs_stats.api;
    if let Some(api) = api {
        set(context, api, "BigIntStats", constructor)?;
    }
    Ok(())
}

/// Preserve the configured Node library's private StatFs constructor and prototype.
pub(crate) fn statfs_from_values(
    context: &mut NativeContext<'_, NodeHost>,
    values: &[RootId],
) -> Result<RootId, RootedError> {
    let values = context.array_rooted(values)?;
    crate::modules::fs_policy_shared_vm::call(context, "getStatFsFromBinding", &[values])
}

pub(crate) fn decorate_stats(
    context: &mut NativeContext<'_, NodeHost>,
    result: RootId,
    bigint: bool,
) -> Result<RootId, RootedError> {
    let shared = context.host_mut().shared_state();
    let api = shared
        .borrow()
        .fs_stats
        .api
        .ok_or_else(|| RootedError::host("stat API missing"))?;
    let constructor = get(context, api, if bigint { "BigIntStats" } else { "Stats" })?;
    let prototype = get(context, constructor, "prototype")?;
    context.set_prototype_rooted(result, prototype)?;
    Ok(result)
}
pub(crate) fn construct(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args.first().copied().unwrap_or_else(|| context.undefined());
    let values = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let bigint = args
        .get(2)
        .copied()
        .unwrap_or_else(|| context.boolean(false));
    let bigint = context.truthy_rooted(bigint)?;
    let shared = context.host_mut().shared_state();
    let warn = args
        .get(3)
        .copied()
        .unwrap_or_else(|| context.boolean(false));
    let warn = context.truthy_rooted(warn)?;
    let global = context.global_root()?;
    let process = get(context, global, "process")?;
    let suppressed = if warn {
        let value = get(context, process, "noDeprecation")?;
        context.truthy_rooted(value)?
    } else {
        true
    };
    let warn = !bigint && !suppressed && !shared.borrow().fs_stats.stats_warning_emitted;
    if warn {
        shared.borrow_mut().fs_stats.stats_warning_emitted = true;
        let api = shared
            .borrow()
            .fs_stats
            .api
            .ok_or_else(|| RootedError::host("stat API missing"))?;
        let warning = get(context, api, "emitWarning")?;
        let text = context.string_rooted("fs.Stats constructor is deprecated.");
        let options = context.object_rooted()?;
        let kind = context.string_rooted("DeprecationWarning");
        let code = context.string_rooted("DEP0180");
        set(context, options, "type", kind)?;
        set(context, options, "code", code)?;
        context.call_rooted(warning, process, &[text, options])?;
    }
    let field_names = BASE_FIELDS;
    for (index, name) in field_names.into_iter().enumerate() {
        let value = get(context, values, &index.to_string())?;
        set(context, target, name, value)?;
    }
    for (index, name) in TIME_FIELDS.into_iter().enumerate() {
        let offset = field_names.len() + if bigint { index } else { index * 2 };
        let value = get(context, values, &offset.to_string())?;
        if bigint {
            let divisor = context.bigint_rooted(NANOSECONDS_PER_MILLISECOND);
            let milliseconds =
                context.arithmetic_rooted(NativeArithmetic::Divide, value, divisor)?;
            set(context, target, &format!("{name}Ms"), milliseconds)?;
            set(context, target, &format!("{name}Ns"), value)?;
        } else {
            let nanos = get(context, values, &(offset + 1).to_string())?;
            let original_nanos = nanos;
            let scale = context.number(MILLISECONDS_PER_SECOND);
            let seconds = context.arithmetic_rooted(NativeArithmetic::Multiply, value, scale)?;
            let scale = context.number(NANOSECONDS_PER_MILLISECOND as f64);
            let nanos = context.arithmetic_rooted(NativeArithmetic::Divide, nanos, scale)?;
            let milliseconds = context.arithmetic_rooted(NativeArithmetic::Add, seconds, nanos)?;
            set(context, target, &format!("{name}Ms"), milliseconds)?;
            set_partial_time(context, target, name, original_nanos)?;
        }
    }
    Ok(target)
}
pub(crate) fn set_partial_time(
    context: &mut NativeContext<'_, NodeHost>,
    stats: RootId,
    name: &str,
    nanos: RootId,
) -> Result<(), RootedError> {
    let title = match name {
        "atime" => "Atime",
        "mtime" => "Mtime",
        "ctime" => "Ctime",
        "birthtime" => "Birthtime",
        _ => return Err(RootedError::host("unknown stat time")),
    };
    let shared = context.host_mut().shared_state();
    let api = shared
        .borrow()
        .fs_stats
        .api
        .ok_or_else(|| RootedError::host("stat API missing"))?;
    let key = get(context, api, &format!("partial{title}"))?;
    context.define_data_property_rooted(stats, key, nanos, true, true, true)?;
    Ok(())
}
pub(crate) fn mode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args.first().copied().unwrap_or_else(|| context.undefined());
    let kind = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let kind = context.to_string(kind)?;
    let property = match kind.as_str() {
        "isDirectory" => libc::S_IFDIR,
        "isFile" => libc::S_IFREG,
        "isBlockDevice" => libc::S_IFBLK,
        "isCharacterDevice" => libc::S_IFCHR,
        "isSymbolicLink" => libc::S_IFLNK,
        "isFIFO" => libc::S_IFIFO,
        "isSocket" => libc::S_IFSOCK,
        _ => return Err(RootedError::host("unknown stat mode method")),
    };
    let check = get(context, target, "_checkModeProperty")?;
    let property = context.number(f64::from(property));
    context.call_rooted(check, target, &[property])
}
pub(crate) fn check_mode(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args.first().copied().unwrap_or_else(|| context.undefined());
    let property = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let bigint = args
        .get(2)
        .copied()
        .unwrap_or_else(|| context.boolean(false));
    let bigint = context.truthy_rooted(bigint)?;
    let mode = get(context, target, "mode")?;
    let global = context.global_root()?;
    let undefined = context.undefined();
    let mask = context.number(f64::from(libc::S_IFMT));
    let mask = if bigint {
        let constructor = get(context, global, "BigInt")?;
        context.call_rooted(constructor, undefined, &[mask])?
    } else {
        mask
    };
    let actual = context.arithmetic_rooted(NativeArithmetic::BitwiseAnd, mode, mask)?;
    let expected = if bigint {
        let constructor = get(context, global, "BigInt")?;
        context.call_rooted(constructor, undefined, &[property])?
    } else {
        property
    };
    let equal = context.strict_equal_rooted(actual, expected)?;
    Ok(context.boolean(equal))
}
pub(crate) fn date(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let target = args.first().copied().unwrap_or_else(|| context.undefined());
    let name = args.get(1).copied().unwrap_or_else(|| context.undefined());
    let name = context.to_string(name)?;
    let milliseconds = get(context, target, &format!("{name}Ms"))?;
    let shared = context.host_mut().shared_state();
    let api = shared
        .borrow()
        .fs_stats
        .api
        .ok_or_else(|| RootedError::host("stat API missing"))?;
    let number = get(context, api, "NumberConstructor")?;
    let undefined = context.undefined();
    let milliseconds = context.call_rooted(number, undefined, &[milliseconds])?;
    let round = get(context, api, "round")?;
    let milliseconds = context.call_rooted(round, undefined, &[milliseconds])?;
    let constructor = get(context, api, "DateConstructor")?;
    let date = context.construct_rooted(constructor, constructor, &[milliseconds])?;
    set(context, target, &name, date)?;
    Ok(date)
}
