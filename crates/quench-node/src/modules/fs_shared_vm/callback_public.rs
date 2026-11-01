//! Configured SDK callback dispatch before the retained native continuations.
use super::get;
use crate::{host::NodeHost, modules::fs_policy_shared_vm as policy};
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

#[derive(Clone, Copy)]
enum Method {
    Access,
    Chmod,
    Fchmod,
    Statfs,
    Exists,
    Owner,
    Fowner,
    Times,
    Ftimes,
    Sync,
    Ftruncate,
    Lstat,
    Rm,
}
impl Method {
    fn parse(name: &str) -> Self {
        match name {
            "access" => Self::Access,
            "chmod" => Self::Chmod,
            "fchmod" => Self::Fchmod,
            "statfs" => Self::Statfs,
            "exists" => Self::Exists,
            "chown" | "lchown" => Self::Owner,
            "fchown" => Self::Fowner,
            "utimes" | "lutimes" => Self::Times,
            "futimes" => Self::Ftimes,
            "fsync" | "fdatasync" => Self::Sync,
            "ftruncate" => Self::Ftruncate,
            "lstat" => Self::Lstat,
            "rm" => Self::Rm,
            _ => unreachable!("named SDK callback boundary"),
        }
    }
    fn callback_index(self) -> usize {
        match self {
            Self::Exists | Self::Sync => 1,
            Self::Owner | Self::Fowner | Self::Times | Self::Ftimes => 3,
            _ => 2,
        }
    }
    fn length(self) -> usize {
        match self {
            Self::Statfs | Self::Ftruncate | Self::Lstat => 1,
            _ => self.callback_index() + 1,
        }
    }
}
pub(super) fn wrap(
    c: &mut Context<'_>,
    original: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let name_root = c.string_rooted(name);
    let data = c.array_rooted(&[original, name_root])?;
    let function =
        c.host_constructor_with_data(crate::host::shared_vm::operation("fsVfsCallback"), data)?;
    let key = c.string_rooted("name");
    c.define_data_property_rooted(function, key, name_root, false, false, true)?;
    let key = c.string_rooted("length");
    let arity = c.number(Method::parse(name).length() as f64);
    c.define_data_property_rooted(function, key, arity, false, false, true)?;
    Ok(function)
}
fn validate_callback(c: &mut Context<'_>, callback: RootId) -> Result<(), RootedError> {
    let label = c.string_rooted("cb");
    policy::call(c, "validateFunction", &[callback, label])?;
    Ok(())
}
fn next_tick(c: &mut Context<'_>, args: &[RootId]) -> Result<RootId, RootedError> {
    let process = c
        .host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("Node process is not initialized"))?;
    let function = get(c, process, "nextTick")?;
    c.call_rooted(function, process, args)
}
fn validate_owner(c: &mut Context<'_>, values: &[RootId; 3]) -> Result<(), RootedError> {
    let maximum = policy::export(c, "kMaxUserId")?;
    let minimum = c.number(-1.0);
    for (index, label) in [(1, "uid"), (2, "gid")] {
        let label = c.string_rooted(label);
        policy::call(
            c,
            "validateInteger",
            &[values[index], label, minimum, maximum],
        )?;
    }
    Ok(())
}
pub(crate) fn dispatch(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let original = get(c, data, "0")?;
    let name = get(c, data, "1")?;
    let name = c.to_string(name)?;
    let method = Method::parse(&name);
    let undefined = c.undefined();
    let mut values = [undefined; 3];
    for (slot, value) in values.iter_mut().zip(args) {
        *slot = *value;
    }
    let mut callback = args
        .get(method.callback_index())
        .copied()
        .unwrap_or(undefined);
    if matches!(method, Method::Access | Method::Statfs | Method::Ftruncate)
        && c.is_callable_rooted(values[1])?
    {
        callback = values[1];
        values[1] = if matches!(method, Method::Statfs) {
            policy::export(c, "kEmptyObject")?
        } else {
            c.number(0.0)
        };
    } else if matches!(method, Method::Statfs)
        && c.rooted_value(values[1]).is_some_and(|v| v.is_undefined())
    {
        values[1] = c.null_object_rooted()?;
        let key = c.string_rooted("bigint");
        let value = c.boolean(false);
        c.define_data_property_rooted(values[1], key, value, true, true, true)?;
    } else if matches!(method, Method::Ftruncate)
        && c.rooted_value(values[1]).is_some_and(|v| v.is_undefined())
    {
        values[1] = c.number(0.0);
    }
    if matches!(method, Method::Rm) && c.is_callable_rooted(values[1])? {
        callback = values[1];
        values[1] = undefined;
    }
    if matches!(method, Method::Access) {
        values[0] = crate::modules::fs_path::validate(c, values[0], "path")?;
        if c.rooted_value(values[1])
            .is_some_and(|value| value.is_undefined())
        {
            values[1] = c.number(0.0);
        }
        let label = c.string_rooted("mode");
        let minimum = c.number(0.0);
        let maximum = c.number(7.0);
        policy::call(c, "validateInteger", &[values[1], label, minimum, maximum])?;
    }
    if matches!(method, Method::Lstat) {
        if c.is_callable_rooted(values[1])? {
            callback = values[1];
            values[1] = policy::export(c, "kEmptyObject")?;
        } else if c.rooted_value(values[1]).is_some_and(|v| v.is_undefined()) {
            values[1] = c.null_object_rooted()?;
            let key = c.string_rooted("bigint");
            let no = c.boolean(false);
            c.define_data_property_rooted(values[1], key, no, true, true, true)?;
        } else if c.rooted_value(values[1]).is_some_and(|v| v.is_null())
            || !c.is_object_rooted(values[1])?
        {
            values[1] = policy::export(c, "kEmptyObject")?;
        } else {
            let defaults = c.object_rooted()?;
            let key = c.string_rooted("bigint");
            let no = c.boolean(false);
            c.define_data_property_rooted(defaults, key, no, true, true, true)?;
            values[1] = policy::call(c, "getOptions", &[values[1], defaults])?;
        }
    }
    if matches!(
        method,
        Method::Exists | Method::Statfs | Method::Sync | Method::Owner | Method::Times
    ) {
        validate_callback(c, callback)?;
    }
    if matches!(method, Method::Chmod | Method::Owner | Method::Times) {
        values[0] = policy::call(c, "getValidatedPath", &[values[0]])?;
    }
    if matches!(method, Method::Chmod | Method::Fchmod) {
        let label = c.string_rooted("mode");
        values[1] = policy::call(c, "parseFileMode", &[values[1], label])?;
    }
    if matches!(method, Method::Owner | Method::Fowner) {
        validate_owner(c, &values)?;
    }
    if matches!(method, Method::Ftimes) {
        let label = c.string_rooted("fd");
        let minimum = c.number(0.0);
        let maximum = c.number(i32::MAX as f64);
        policy::call(c, "validateInteger", &[values[0], label, minimum, maximum])?;
    }
    if matches!(method, Method::Times | Method::Ftimes) {
        for (index, label) in [(1, "atime"), (2, "mtime")] {
            values[index] = if matches!(method, Method::Ftimes) {
                let label = c.string_rooted(label);
                policy::call(c, "toUnixTimestamp", &[values[index], label])?
            } else {
                policy::call(c, "toUnixTimestamp", &[values[index]])?
            };
        }
    }
    if matches!(method, Method::Ftruncate) {
        let label = c.string_rooted("len");
        policy::call(c, "validateInteger", &[values[1], label])?;
        if c.to_number(values[1])? < 0.0 {
            values[1] = c.number(0.0);
        }
    }
    if matches!(
        method,
        Method::Chmod | Method::Fchmod | Method::Fowner | Method::Ftimes | Method::Ftruncate
    ) {
        validate_callback(c, callback)?;
    }
    let handlers = policy::vfs_handlers(c)?;
    let vfs_active = handlers.is_some();
    if let Some(handlers) = handlers {
        match method {
            Method::Statfs => match policy::vfs_call(c, handlers, "statfsSync", &values[..2]) {
                Ok(Some(value)) => {
                    let null = c.null();
                    next_tick(c, &[callback, null, value])?;
                    return Ok(undefined);
                }
                Ok(None) => {}
                Err(error) => {
                    let Some(exception) = error.exception else {
                        return Err(error);
                    };
                    next_tick(c, &[callback, exception])?;
                    return Ok(undefined);
                }
            },
            Method::Lstat => {
                if let Some(promise) = policy::vfs_call(c, handlers, &name, &values[..2])? {
                    policy::call(c, "vfsResult", &[promise, callback])?;
                    return Ok(undefined);
                }
            }
            Method::Exists => {
                if let Some(value) = policy::vfs_call(c, handlers, "existsSync", &values[..1])? {
                    next_tick(c, &[callback, value])?;
                    return Ok(undefined);
                }
            }
            _ => {
                let arity = match method {
                    Method::Sync | Method::Fowner => 1,
                    _ => method.callback_index(),
                };
                if let Some(promise) = policy::vfs_call(c, handlers, &name, &values[..arity])? {
                    policy::call(c, "vfsVoid", &[promise, callback])?;
                    return Ok(undefined);
                }
            }
        }
    }
    if matches!(method, Method::Access) {
        values[0] = policy::call(c, "getValidatedPath", &[values[0]])?;
        validate_callback(c, callback)?;
    }
    if matches!(method, Method::Exists) && vfs_active {
        let module = crate::host::shared_vm::commonjs::fs_module(c)?;
        let access = get(c, module, "access")?;
        let complete = c.host_function_with_data(
            crate::host::shared_vm::operation("fsVfsExistsComplete"),
            callback,
        )?;
        let mode = c.number(0.0);
        match c.call_rooted(access, module, &[values[0], mode, complete]) {
            Ok(_) => return Ok(undefined),
            Err(error) if error.exception.is_some() => {
                let no = c.boolean(false);
                return c.call_rooted(callback, undefined, &[no]);
            }
            Err(error) => return Err(error),
        }
    }
    let mut fallback = [undefined; 4];
    let arity = method.callback_index();
    fallback[..arity].copy_from_slice(&values[..arity]);
    fallback[arity] = callback;
    c.call_rooted(original, this, &fallback[..=arity])
}
pub(crate) fn exists_complete(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = c.host_function_data()?;
    let undefined = c.undefined();
    let error = args.first().copied().unwrap_or(undefined);
    let success = !c.truthy_rooted(error)?;
    let success = c.boolean(success);
    c.call_rooted(callback, undefined, &[success])?;
    Ok(undefined)
}
