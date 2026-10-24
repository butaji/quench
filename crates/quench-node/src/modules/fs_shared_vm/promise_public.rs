//! Original SDK async readlink policy over the retained native promise operation.
use super::{get, set};
use crate::{
    host::{
        shared_vm::{commonjs, policy_projection},
        NodeHost,
    },
    modules::fs_policy_shared_vm as policy,
};
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;
const CAPTURES: &[&str] = &[
    "vfsState",
    "getOptions",
    "getValidatedPath",
    "PromisePrototypeThen",
    "PromiseReject",
    "ErrorCaptureStackTrace",
    "binding",
    "kUsePromises",
];
pub(super) fn install(c: &mut Context<'_>, public: RootId) -> Result<(), RootedError> {
    c.with_startup_intrinsics(|c| {
        let root = c.host_mut().node_library_root.clone();
        let filename = root.join("lib/internal/fs/promises.js");
        let source = std::fs::read_to_string(&filename)
            .or_else(|error| {
                crate::node_sources::source(&filename, &root)
                    .map(str::to_owned)
                    .ok_or(error)
            })
            .map_err(|error| RootedError::host(error.to_string()))?;
        let source = policy_projection::project_with_bindings(&source, &["readlink"], CAPTURES)
            .map_err(RootedError::host)?;
        let factory = c.evaluate_script_rooted(
            &format!(
                "({}, module) => {{\n{source}\nreturn module.exports.readlink;\n}}",
                CAPTURES.join(", ")
            ),
            &filename.to_string_lossy(),
        )?;
        let primordials = commonjs::node_primordials(c)?
            .ok_or_else(|| RootedError::host("SDK promise policy requires primordials"))?;
        let original = get(c, public, "readlink")?;
        // This unique symbol is a capability for the promise-only native
        // continuation, matching Node's opaque fs_use_promises_symbol role.
        let token = c.symbol_rooted(Some("fs_use_promises_symbol"));
        let data = c.array_rooted(&[original, token])?;
        let readlink = c.host_function_named_with_data(
            crate::host::shared_vm::operation("fsVfsReadlinkPromise"),
            "readlink",
            data,
        )?;
        let binding = c.null_object_rooted()?;
        set(c, binding, "readlink", readlink)?;
        let state = policy::export(c, "vfsState")?;
        let options = policy::export(c, "getOptions")?;
        let path = policy::export(c, "getValidatedPath")?;
        let then = get(c, primordials, "PromisePrototypeThen")?;
        let reject = get(c, primordials, "PromiseReject")?;
        let capture = get(c, primordials, "ErrorCaptureStackTrace")?;
        let module = c.object_rooted()?;
        let undefined = c.undefined();
        let function = c.call_rooted(
            factory,
            undefined,
            &[
                state, options, path, then, reject, capture, binding, token, module,
            ],
        )?;
        set(c, public, "readlink", function)
    })
}

pub(crate) fn readlink_continuation(
    c: &mut Context<'_>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let original = get(c, data, "0")?;
    let token = get(c, data, "1")?;
    let undefined = c.undefined();
    let mode = args.get(2).copied().unwrap_or(undefined);
    if !c.strict_equal_rooted(token, mode)? {
        return Err(RootedError::host(
            "filesystem continuation requires its promise capability",
        ));
    }
    let path = args.first().copied().unwrap_or(undefined);
    let encoding = args.get(1).copied().unwrap_or(undefined);
    c.with_startup_intrinsics(|c| c.call_rooted(original, undefined, &[path, encoding]))
}
