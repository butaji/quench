//! The constructors `fs` defines (`Stats`, `Dirent`, `Dir`, `FileHandle`, ...) and the
//! intrinsics their instances use, built once per VM.

use crate::modules::stream_shared_vm::{get, put, Context, Outcome};
use quench_runtime::{DescriptorFields, RootId, RootedError};

/// A root `fs` keeps for the VM's lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Class {
    StatsBase,
    Stats,
    DeprecatedStats,
    BigIntStats,
    StatFs,
    Dirent,
    Dir,
    FileHandle,
    /// The `Date` constructor stat times are made with.
    Date,
    /// `Symbol('type')`, a `Dirent`'s type.
    DirentType,
    /// `Symbol('kFd')`, a `FileHandle`'s descriptor.
    HandleFd,
    /// `Symbol('kClosePromise')`, a closing `FileHandle`'s close.
    HandleClosing,
    /// The private state of a `Dir`.
    DirState,
}

impl Class {
    const COUNT: usize = 13;
}

#[derive(Clone, Copy)]
pub(crate) struct Roots {
    values: [RootId; Class::COUNT],
    prototypes: [RootId; Class::COUNT],
}

crate::host::startup_snapshot::host_snap_struct!(Roots { values, prototypes });

#[derive(Default)]
pub(crate) struct State {
    roots: Option<Roots>,
    pub(crate) stats_warning_emitted: bool,
}

crate::host::startup_snapshot::host_snap_state!(State { roots, stats_warning_emitted });

fn roots(c: &mut Context<'_>) -> Result<Roots, RootedError> {
    if let Some(roots) = c.host_mut().shared_state().borrow().fs_classes.roots {
        return Ok(roots);
    }
    build(c)
}

/// The root `class` names.
pub(crate) fn root(c: &mut Context<'_>, class: Class) -> Outcome {
    Ok(roots(c)?.values[class as usize])
}

/// A new object whose prototype is `class.prototype`, as `new class()` allocates it.
pub(crate) fn instance(c: &mut Context<'_>, class: Class) -> Outcome {
    let prototype = roots(c)?.prototypes[class as usize];
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    Ok(object)
}

/// A class constructor called without `new` throws as JavaScript's do.
pub(crate) fn require_new(c: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!("Class constructor {name} cannot be invoked without 'new'"))?;
    Err(c.throw(error))
}

fn operation(name: &str) -> quench_runtime::HostFunctionId {
    crate::host::shared_vm::operation(name)
}

/// A constructor whose prototype inherits `parent`'s, and which inherits `parent` itself.
fn constructor(c: &mut Context<'_>, op: &str, name: &str, parent: Option<RootId>) -> Result<(RootId, RootId), RootedError> {
    let constructor = c.host_constructor_named(operation(op), name)?;
    let prototype = get(c, constructor, "prototype")?;
    if let Some(parent) = parent {
        let parent_prototype = get(c, parent, "prototype")?;
        c.set_prototype_rooted(prototype, parent_prototype)?;
        c.set_prototype_rooted(constructor, parent)?;
    }
    Ok((constructor, prototype))
}

/// A method as a class body defines it: writable, configurable, not enumerable.
pub(crate) fn method(c: &mut Context<'_>, target: RootId, key: RootId, op: &str, name: &str, data: Option<RootId>) -> Result<RootId, RootedError> {
    let function = match data {
        Some(data) => c.host_function_named_with_data(operation(op), name, data)?,
        None => c.host_function_named(operation(op), name)?,
    };
    c.define_data_property_rooted(target, key, function, true, false, true)?;
    Ok(function)
}

/// An accessor `{ get, set }` with the given enumerability, configurable.
pub(crate) fn accessor(c: &mut Context<'_>, target: RootId, name: &str, getter: RootId, setter: Option<RootId>, enumerable: bool) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(setter.unwrap_or_else(|| c.undefined())),
        enumerable: Some(enumerable),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields).map(drop)
}

/// `Symbol[name]` where the engine has it, else `Symbol.for(fallback)`.
pub(crate) fn well_known_symbol(c: &mut Context<'_>, name: &str, fallback: &str) -> Outcome {
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let known = get(c, symbol, name)?;
    if c.is_symbol_rooted(known)? {
        return Ok(known);
    }
    let key = c.string_rooted(fallback);
    crate::modules::stream_shared_vm::call_method(c, symbol, "for", &[key])
}

fn stats_classes(c: &mut Context<'_>, values: &mut [Option<(RootId, RootId)>; Class::COUNT]) -> Result<(), RootedError> {
    let (base, base_prototype) = constructor(c, "fsStatsBase", "StatsBase", None)?;
    for (name, mode, _) in super::stats::TYPE_PREDICATES {
        let data = c.number(f64::from(mode));
        let function = c.host_function_named_with_data(operation("fsStatsIsType"), "", data)?;
        put(c, base_prototype, name, function)?;
    }
    let stat_class = |c: &mut Context<'_>, op: &str, name: &str, bigint: bool| -> Result<(RootId, RootId), RootedError> {
        let (class, prototype) = constructor(c, op, name, Some(base))?;
        for (index, field) in super::stats::TIME_FIELDS.into_iter().enumerate() {
            let data = c.number(index as f64);
            let getter = c.host_function_named_with_data(operation("fsStatsTimeGet"), "get", data)?;
            let setter = c.host_function_named_with_data(operation("fsStatsTimeSet"), "set", data)?;
            accessor(c, prototype, field, getter, Some(setter), true)?;
        }
        let data = c.boolean(bigint);
        let check = c.host_function_named_with_data(operation("fsStatsCheckMode"), "", data)?;
        put(c, prototype, "_checkModeProperty", check)?;
        Ok((class, prototype))
    };
    let stats = stat_class(c, "fsStatsConstruct", "Stats", false)?;
    let bigint = stat_class(c, "fsBigIntStatsConstruct", "BigIntStats", true)?;
    let deprecated = c.host_constructor_named(operation("fsStatsDeprecated"), "deprecated")?;
    let key = c.string_rooted("prototype");
    let fields = DescriptorFields { value: Some(stats.1), ..DescriptorFields::default() };
    c.define_property_fields_rooted(deprecated, key, fields)?;
    c.set_prototype_rooted(deprecated, stats.0)?;
    values[Class::StatsBase as usize] = Some((base, base_prototype));
    values[Class::Stats as usize] = Some(stats);
    values[Class::BigIntStats as usize] = Some(bigint);
    values[Class::DeprecatedStats as usize] = Some((deprecated, stats.1));
    values[Class::StatFs as usize] = Some(constructor(c, "fsStatFsConstruct", "StatFs", None)?);
    Ok(())
}

fn build(c: &mut Context<'_>) -> Result<Roots, RootedError> {
    let mut values: [Option<(RootId, RootId)>; Class::COUNT] = [None; Class::COUNT];
    stats_classes(c, &mut values)?;
    let global = c.global_root()?;
    let date = get(c, global, "Date")?;
    let date_prototype = get(c, date, "prototype")?;
    values[Class::Date as usize] = Some((date, date_prototype));
    let undefined = c.undefined();
    for (class, description) in [
        (Class::DirentType, "type"),
        (Class::HandleFd, "kFd"),
        (Class::HandleClosing, "kClosePromise"),
    ] {
        values[class as usize] = Some((c.symbol_rooted(Some(description)), undefined));
    }
    values[Class::DirState as usize] = Some((c.private_symbol_rooted(Some("#state")), undefined));
    let dirent_type = values[Class::DirentType as usize].map_or(undefined, |(symbol, _)| symbol);
    values[Class::Dirent as usize] = Some(super::dir::dirent_class(c, dirent_type)?);
    values[Class::Dir as usize] = Some(super::dir::dir_class(c)?);
    let emitter = get(c, global, "__nodeEventEmitter")?;
    let (handle, handle_prototype) = constructor(c, "fsFileHandleConstruct", "FileHandle", Some(emitter))?;
    values[Class::FileHandle as usize] = Some((handle, handle_prototype));
    let mut roots = Roots { values: [undefined; Class::COUNT], prototypes: [undefined; Class::COUNT] };
    for (index, entry) in values.into_iter().enumerate() {
        let (value, prototype) = entry.ok_or_else(|| RootedError::host("fs class left unbuilt"))?;
        roots.values[index] = c.retain(value)?;
        roots.prototypes[index] = c.retain(prototype)?;
    }
    c.host_mut().shared_state().borrow_mut().fs_classes.roots = Some(roots);
    super::file_handle::define_prototype(c, handle_prototype)?;
    Ok(roots)
}
