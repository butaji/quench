//! SDK VFS dispatch boundaries around existing native metadata continuations.
use super::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

#[derive(Clone, Copy)]
enum Method { Stat, Lstat, Fstat, Close, Fchmod }
impl Method {
    fn parse(name: &str) -> Self {
        match name {
            "statSync" => Self::Stat, "lstatSync" => Self::Lstat,
            "fstatSync" => Self::Fstat, "closeSync" => Self::Close,
            "fchmodSync" => Self::Fchmod,
            _ => unreachable!("metadata boundary is installed only for its named SDK methods"),
        }
    }
    fn catches_missing(self) -> bool { matches!(self, Self::Stat | Self::Lstat) }
    fn returns_void(self) -> bool { matches!(self, Self::Close | Self::Fchmod) }
    fn required_arguments(self) -> usize {
        // SDK stat/lstat/fstat have a defaulted second parameter; close has
        // only fd, whereas fchmod requires both fd and mode.
        match self { Self::Fchmod => 2, _ => 1 }
    }
}

pub(super) fn wrap(c: &mut Context<'_>, original: RootId, name: &str) -> Result<RootId, RootedError> {
    let method = Method::parse(name);
    let name_root = c.string_rooted(name);
    let data = c.array_rooted(&[original, name_root])?;
    let function = c.host_constructor_with_data(crate::host::shared_vm::operation("fsVfsMetadata"), data)?;
    let name_key = c.string_rooted("name");
    c.define_data_property_rooted(function, name_key, name_root, false, false, true)?;
    let length_key = c.string_rooted("length");
    let length = c.number(method.required_arguments() as f64);
    c.define_data_property_rooted(function, length_key, length, false, false, true)?;
    Ok(function)
}

fn nullish(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).is_some_and(|v| v.is_null() || v.is_undefined())
}
fn stat_options(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    if !c.rooted_value(value).is_some_and(|v| v.is_undefined()) { return Ok(value); }
    let options = c.null_object_rooted()?;
    for (name, value) in [("bigint", false), ("throwIfNoEntry", true)] {
        let key = c.string_rooted(name);
        let value = c.boolean(value);
        c.define_data_property_rooted(options, key, value, true, true, true)?;
    }
    Ok(options)
}
fn suppress_missing(c: &mut Context<'_>, error: &RootedError, options: RootId) -> Result<bool, RootedError> {
    let Some(exception) = error.exception else { return Ok(false); };
    if nullish(c, exception) { return Ok(false); }
    let code = get(c, exception, "code")?;
    let enoent = c.string_rooted("ENOENT");
    if !c.strict_equal_rooted(code, enoent)? || nullish(c, options) { return Ok(false); }
    let flag = get(c, options, "throwIfNoEntry")?;
    let no = c.boolean(false);
    c.strict_equal_rooted(flag, no)
}

pub(crate) fn dispatch(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let original = get(c, data, "0")?;
    let name_root = get(c, data, "1")?;
    let name = c.to_string(name_root)?;
    let method = Method::parse(&name);
    let undefined = c.undefined();
    let path_or_fd = args.first().copied().unwrap_or(undefined);
    let mut second = args.get(1).copied().unwrap_or(undefined);
    if method.catches_missing() { second = stat_options(c, second)?; }
    if matches!(method, Method::Fchmod) {
        let mode_name = c.string_rooted("mode");
        second = crate::modules::fs_policy_shared_vm::call(c, "parseFileMode", &[second, mode_name])?;
    }
    let forwarded = [path_or_fd, second];
    let handler_args = if matches!(method, Method::Fstat | Method::Close) { &forwarded[..1] } else { &forwarded[..] };
    // SDK reads state.handlers outside stat/lstat's try. A throwing state
    // accessor must never be mistaken for a missing filesystem entry.
    if let Some(handlers) = crate::modules::fs_policy_shared_vm::vfs_handlers(c)? {
        match crate::modules::fs_policy_shared_vm::vfs_call(c, handlers, &name, handler_args) {
            Ok(Some(value)) => return Ok(if method.returns_void() { undefined } else { value }),
            Ok(None) => {},
            Err(error) => {
                if method.catches_missing() && suppress_missing(c, &error, second)? { return Ok(undefined); }
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
        } else { get(c, second, "bigint")? };
        let options = c.null_object_rooted()?;
        let key = c.string_rooted("bigint");
        c.define_data_property_rooted(options, key, bigint, true, true, true)?;
        return c.call_rooted(original, this, &[path_or_fd, options]);
    }
    let fallback_args = if matches!(method, Method::Close) { &forwarded[..1] } else { &forwarded[..] };
    c.call_rooted(original, this, fallback_args)
}
