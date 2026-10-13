//! One pending next() owns its listener, resolver, and optional abort subscription.
use super::{closed, get, method, nullish, operation, set};
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

fn done(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let result = context.null_object_rooted()?;
    let yes = context.boolean(true);
    set(context, result, "done", yes)?;
    Ok(result)
}
pub(crate) fn iterate(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    operation(context, "makeIterator", undefined, args)
}
pub(crate) fn next(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let owner = args.first().copied().unwrap_or_else(|| context.undefined());
    let state = args.get(1).copied().unwrap_or_else(|| context.undefined());
    if closed(context, state)? {
        return done(context);
    }
    let data = context.object_rooted()?;
    set(context, data, "owner", owner)?;
    set(context, data, "state", state)?;
    let executor = context.host_function_with_data(
        crate::host::shared_vm::operation("fsRecursiveIteratorExecutor"),
        data,
    )?;
    let global = context.global_root()?;
    let promise = get(context, global, "Promise")?;
    context.construct_rooted(promise, promise, &[executor])
}
pub(crate) fn finish(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let owner = args.first().copied().unwrap_or_else(|| context.undefined());
    method(context, owner, "close")?;
    done(context)
}
pub(crate) fn executor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let resolve = args.first().copied().unwrap_or_else(|| context.undefined());
    let reject = args.get(1).copied().unwrap_or_else(|| context.undefined());
    set(context, data, "resolve", resolve)?;
    set(context, data, "reject", reject)?;
    let owner = get(context, data, "owner")?;
    let state = get(context, data, "state")?;
    let options = get(context, state, "options")?;
    let signal = get(context, options, "signal")?;
    if !nullish(context, signal) {
        set(context, data, "signal", signal)?;
        let abort = context.host_function_with_data(
            crate::host::shared_vm::operation("fsRecursiveIteratorAbort"),
            data,
        )?;
        set(context, data, "abort", abort)?;
        let aborted = get(context, signal, "aborted")?;
        if context.truthy_rooted(aborted)? {
            let undefined = context.undefined();
            return context.call_rooted(abort, undefined, &[]);
        }
        let add = get(context, signal, "addEventListener")?;
        let event = context.string_rooted("abort");
        let options = context.object_rooted()?;
        let yes = context.boolean(true);
        set(context, options, "once", yes)?;
        context.call_rooted(add, signal, &[event, abort, options])?;
    }
    let callback = context.host_function_with_data(
        crate::host::shared_vm::operation("fsRecursiveIteratorChange"),
        data,
    )?;
    let once = get(context, owner, "once")?;
    let change = context.string_rooted("change");
    context.call_rooted(once, owner, &[change, callback])?;
    Ok(context.undefined())
}
pub(crate) fn change(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let signal = get(context, data, "signal")?;
    if !nullish(context, signal) {
        let abort = get(context, data, "abort")?;
        let remove = get(context, signal, "removeEventListener")?;
        let event = context.string_rooted("abort");
        context.call_rooted(remove, signal, &[event, abort])?;
    }
    let value = context.object_rooted()?;
    let kind = args.first().copied().unwrap_or_else(|| context.undefined());
    let filename = args.get(1).copied().unwrap_or_else(|| context.undefined());
    set(context, value, "eventType", kind)?;
    set(context, value, "filename", filename)?;
    let result = context.null_object_rooted()?;
    set(context, result, "value", value)?;
    let resolve = get(context, data, "resolve")?;
    let undefined = context.undefined();
    context.call_rooted(resolve, undefined, &[result])
}
pub(crate) fn abort(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let owner = get(context, data, "owner")?;
    method(context, owner, "close")?;
    let error = context.error_rooted("The operation was aborted")?;
    let signal = get(context, data, "signal")?;
    let cause = get(context, signal, "reason")?;
    let key = context.string_rooted("cause");
    context.define_data_property_rooted(error, key, cause, true, false, true)?;
    let code = context.string_rooted("ABORT_ERR");
    set(context, error, "code", code)?;
    let name = context.string_rooted("AbortError");
    set(context, error, "name", name)?;
    let reject = get(context, data, "reject")?;
    let undefined = context.undefined();
    context.call_rooted(reject, undefined, &[error])
}
