//! `node:url` is Node's lib/url.js and `internal/url` its lib/internal/url.js over
//! `internalBinding('url')`; host code converts file URLs through those same exports.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

/// Call an export of lib/internal/url.js.
fn internal(c: &mut Context<'_>, name: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::internal_url_module(c)?;
    let function = get(c, module, name)?;
    let undefined = c.undefined();
    c.call_rooted(function, undefined, args)
}

/// `isURL`: Node's URL-like shape test (`href` and `protocol`, no `auth` or `path`).
pub(crate) fn is_url_like(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    let result = internal(c, "isURL", &[value])?;
    c.truthy_rooted(result)
}

pub(crate) fn to_path_if_file_url(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = match args.first() {
        Some(value) => *value,
        None => c.undefined(),
    };
    internal(c, "toPathIfFileURL", &[value])
}

pub(crate) fn file_url_to_path(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    internal(c, "fileURLToPath", args)
}

pub(crate) fn file_url_to_path_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    internal(c, "fileURLToPathBuffer", args)
}

/// A string argument, or the serialization of a URL object; `None` for anything else.
pub(crate) fn url_argument_text(c: &mut Context<'_>, value: RootId) -> Result<Option<String>, RootedError> {
    if let Some(text) = c.string_text(value)? {
        return Ok(Some(text));
    }
    if !is_url_like(c, value)? {
        return Ok(None);
    }
    let href = get(c, value, "href")?;
    c.string_text(href)
}
