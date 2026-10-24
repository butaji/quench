//! SDK VFS dispatch boundaries around existing native metadata continuations.
use super::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

#[derive(Clone, Copy)]
enum Method {
    Access,
    Stat,
    Lstat,
    Statfs,
    Fstat,
    Close,
    Fchmod,
    RawVoid(usize),
    RawValue(usize),
    Truncate,
    Rmdir,
    Chmod,
    Owner,
    Times,
    Fowner,
    Ftimes,
    Ftruncate,
}
impl Method {
    fn parse(name: &str) -> Self {
        match name {
            "statSync" => Self::Stat,
            "lstatSync" => Self::Lstat,
            "statfsSync" => Self::Statfs,
            "fstatSync" => Self::Fstat,
            "closeSync" => Self::Close,
            "fchmodSync" => Self::Fchmod,
            "accessSync" => Self::Access,
            "renameSync" | "linkSync" => Self::RawVoid(2),
            "copyFileSync" | "symlinkSync" => Self::RawVoid(3),
            "rmSync" => Self::RawVoid(2),
            "unlinkSync" | "fsyncSync" | "fdatasyncSync" => Self::RawVoid(1),
            "mkdtempSync" | "readlinkSync" | "createReadStream" | "createWriteStream" => {
                Self::RawValue(2)
            }
            "chmodSync" => Self::Chmod,
            "chownSync" | "lchownSync" => Self::Owner,
            "utimesSync" | "lutimesSync" => Self::Times,
            "truncateSync" => Self::Truncate,
            "ftruncateSync" => Self::Ftruncate,
            "fchownSync" => Self::Fowner,
            "futimesSync" => Self::Ftimes,
            "rmdirSync" => Self::Rmdir,
            _ => unreachable!("metadata boundary is installed only for its named SDK methods"),
        }
    }
    fn catches_missing(self) -> bool {
        matches!(self, Self::Stat | Self::Lstat)
    }
    fn returns_void(self) -> bool {
        matches!(self, Self::Close | Self::Fchmod)
    }
    fn required_arguments(self) -> usize {
        // SDK stat/lstat/fstat have a defaulted second parameter; close has
        // only fd, whereas fchmod requires both fd and mode.
        match self {
            Self::Fchmod | Self::Access => 2,
            Self::RawVoid(arity) | Self::RawValue(arity) => arity,
            Self::Truncate | Self::Rmdir | Self::Chmod => 2,
            Self::Owner | Self::Times | Self::Fowner | Self::Ftimes => 3,
            _ => 1,
        }
    }
}

pub(super) fn wrap(
    c: &mut Context<'_>,
    original: RootId,
    name: &str,
) -> Result<RootId, RootedError> {
    let method = Method::parse(name);
    let name_root = c.string_rooted(name);
    let data = c.array_rooted(&[original, name_root])?;
    let function =
        c.host_constructor_with_data(crate::host::shared_vm::operation("fsVfsMetadata"), data)?;
    let name_key = c.string_rooted("name");
    c.define_data_property_rooted(function, name_key, name_root, false, false, true)?;
    let length_key = c.string_rooted("length");
    let length = c.number(method.required_arguments() as f64);
    c.define_data_property_rooted(function, length_key, length, false, false, true)?;
    Ok(function)
}

fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value)
        .is_some_and(|v| v.is_null() || v.is_undefined())
}
fn stat_options(
    c: &mut Context<'_>,
    value: RootId,
    missing_entry_policy: bool,
) -> Result<RootId, RootedError> {
    if !c.rooted_value(value).is_some_and(|v| v.is_undefined()) {
        return Ok(value);
    }
    let options = c.null_object_rooted()?;
    let defaults = [("bigint", false), ("throwIfNoEntry", true)];
    let defaults = if missing_entry_policy {
        &defaults[..]
    } else {
        &defaults[..1]
    };
    for &(name, value) in defaults {
        let key = c.string_rooted(name);
        let value = c.boolean(value);
        c.define_data_property_rooted(options, key, value, true, true, true)?;
    }
    Ok(options)
}
fn suppress_missing(
    c: &mut Context<'_>,
    error: &RootedError,
    options: RootId,
) -> Result<bool, RootedError> {
    let Some(exception) = error.exception else {
        return Ok(false);
    };
    if nullish(c, exception) {
        return Ok(false);
    }
    let code = get(c, exception, "code")?;
    let enoent = c.string_rooted("ENOENT");
    if !c.strict_equal_rooted(code, enoent)? || nullish(c, options) {
        return Ok(false);
    }
    let flag = get(c, options, "throwIfNoEntry")?;
    let no = c.boolean(false);
    c.strict_equal_rooted(flag, no)
}

pub(crate) fn dispatch(
    c: &mut Context<'_>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let original = get(c, data, "0")?;
    let name_root = get(c, data, "1")?;
    let name = c.to_string(name_root)?;
    let method = Method::parse(&name);
    let undefined = c.undefined();
    // SDK raw boundaries dispatch before path validation. Void operations
    // use any defined handler result as a presence envelope; value operations
    // return that same result. Truncate alone supplies a default length.
    let raw_arity = match method {
        Method::RawVoid(arity) | Method::RawValue(arity) => Some(arity),
        Method::Truncate | Method::Ftruncate | Method::Chmod | Method::Access => Some(2),
        Method::Owner | Method::Times | Method::Fowner | Method::Ftimes => Some(3),
        Method::Rmdir => Some(1),
        _ => None,
    };
    if let Some(arity) = raw_arity {
        let mut forwarded = [undefined; 3];
        for (slot, value) in forwarded.iter_mut().zip(args) {
            *slot = *value;
        }
        if matches!(method, Method::Truncate | Method::Ftruncate)
            && c.rooted_value(forwarded[1])
                .is_some_and(|v| v.is_undefined())
        {
            forwarded[1] = c.number(0.0);
        }
        if matches!(method, Method::Ftruncate) {
            let label = c.string_rooted("len");
            crate::modules::fs_policy_shared_vm::call(
                c,
                "validateInteger",
                &[forwarded[1], label],
            )?;
            if c.to_number(forwarded[1])? < 0.0 {
                forwarded[1] = c.number(0.0);
            }
        }
        if matches!(method, Method::Access) {
            use crate::modules::fs_policy_shared_vm as policy;
            forwarded[0] = crate::modules::fs_path::validate(c, forwarded[0], "path")?;
            if c.rooted_value(forwarded[1])
                .is_some_and(|value| value.is_undefined())
            {
                forwarded[1] = c.number(0.0);
            }
            let label = c.string_rooted("mode");
            let minimum = c.number(0.0);
            let maximum = c.number(7.0);
            policy::call(
                c,
                "validateInteger",
                &[forwarded[1], label, minimum, maximum],
            )?;
        }
        if matches!(
            method,
            Method::Chmod | Method::Owner | Method::Times | Method::Fowner | Method::Ftimes
        ) {
            use crate::modules::fs_policy_shared_vm as policy;
            if !matches!(method, Method::Fowner | Method::Ftimes) {
                forwarded[0] = policy::call(c, "getValidatedPath", &[forwarded[0]])?;
            }
            match method {
                Method::Chmod => {
                    let label = c.string_rooted("mode");
                    forwarded[1] = policy::call(c, "parseFileMode", &[forwarded[1], label])?;
                }
                Method::Owner | Method::Fowner => {
                    let maximum = policy::export(c, "kMaxUserId")?;
                    let minimum = c.number(-1.0);
                    for (index, label) in [(1, "uid"), (2, "gid")] {
                        let label = c.string_rooted(label);
                        policy::call(
                            c,
                            "validateInteger",
                            &[forwarded[index], label, minimum, maximum],
                        )?;
                    }
                }
                Method::Times | Method::Ftimes => {
                    if matches!(method, Method::Ftimes) {
                        let label = c.string_rooted("fd");
                        let minimum = c.number(0.0);
                        let maximum = c.number(i32::MAX as f64);
                        policy::call(
                            c,
                            "validateInteger",
                            &[forwarded[0], label, minimum, maximum],
                        )?;
                        let atime = c.string_rooted("atime");
                        let mtime = c.string_rooted("mtime");
                        forwarded[1] = policy::call(c, "toUnixTimestamp", &[forwarded[1], atime])?;
                        forwarded[2] = policy::call(c, "toUnixTimestamp", &[forwarded[2], mtime])?;
                    } else {
                        forwarded[1] = policy::call(c, "toUnixTimestamp", &[forwarded[1]])?;
                        forwarded[2] = policy::call(c, "toUnixTimestamp", &[forwarded[2]])?;
                    }
                }
                _ => unreachable!("normalized VFS path policy"),
            }
        }
        let handler_arity = if matches!(method, Method::Fowner) {
            1
        } else {
            arity
        };
        if let Some(value) =
            crate::modules::fs_policy_shared_vm::vfs_read(c, &name, &forwarded[..handler_arity])?
        {
            return Ok(if matches!(method, Method::RawValue(_)) {
                value
            } else {
                undefined
            });
        }
        let fallback_arity = if matches!(method, Method::Rmdir) {
            forwarded[1] = args.get(1).copied().unwrap_or(undefined);
            2
        } else {
            arity
        };
        return c.call_rooted(original, this, &forwarded[..fallback_arity]);
    }
    let path_or_fd = args.first().copied().unwrap_or(undefined);
    let mut second = args.get(1).copied().unwrap_or(undefined);
    if method.catches_missing() || matches!(method, Method::Statfs) {
        second = stat_options(c, second, method.catches_missing())?;
    }
    if matches!(method, Method::Fchmod) {
        let mode_name = c.string_rooted("mode");
        second =
            crate::modules::fs_policy_shared_vm::call(c, "parseFileMode", &[second, mode_name])?;
    }
    let forwarded = [path_or_fd, second];
    let handler_args = if matches!(method, Method::Fstat | Method::Close) {
        &forwarded[..1]
    } else {
        &forwarded[..]
    };
    // SDK reads state.handlers outside stat/lstat's try. A throwing state
    // accessor must never be mistaken for a missing filesystem entry.
    if let Some(handlers) = crate::modules::fs_policy_shared_vm::vfs_handlers(c)? {
        match crate::modules::fs_policy_shared_vm::vfs_call(c, handlers, &name, handler_args) {
            Ok(Some(value)) => {
                return Ok(if method.returns_void() {
                    undefined
                } else {
                    value
                });
            }
            Ok(None) => {}
            Err(error) => {
                if method.catches_missing() && suppress_missing(c, &error, second)? {
                    return Ok(undefined);
                }
                return Err(error);
            }
        }
    }
    if matches!(method, Method::Fstat) {
        // SDK evaluates options.bigint before binding-level fd validation.
        // The retained native facade expects an options object; snapshot this
        // one already-read scalar so its continuation cannot repeat a getter.
        let bigint = if c.rooted_value(second).is_some_and(|v| v.is_undefined()) {
            c.boolean(false)
        } else {
            get(c, second, "bigint")?
        };
        let options = c.null_object_rooted()?;
        let key = c.string_rooted("bigint");
        c.define_data_property_rooted(options, key, bigint, true, true, true)?;
        return c.call_rooted(original, this, &[path_or_fd, options]);
    }
    let fallback_args = if matches!(method, Method::Close) {
        &forwarded[..1]
    } else {
        &forwarded[..]
    };
    c.call_rooted(original, this, fallback_args)
}
