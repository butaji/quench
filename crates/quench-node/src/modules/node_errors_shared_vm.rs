//! Node's coded argument-error prototype, distinct from the ECMAScript base.

use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const INVALID_ARGUMENT_TYPE: &str = "ERR_INVALID_ARG_TYPE";
const PROTOTYPE: &str = quench_js_check::checked_js!(
    r#"(Base, constructorNative, brandNative, stringNative, brand) =>
    class NodeError extends Base {
      get ['constructor']() { return constructorNative(); }
      get [brand]() { return brandNative(); }
      toString() { return stringNative(this); }
    }.prototype"#
);

pub(super) fn argument_type_prototype(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    let base_error = context.type_error_rooted("")?;
    let base_prototype = context.prototype_rooted(base_error)?;
    let base = get(context, base_prototype, "constructor")?;
    let constructor = context.host_function_with_data(
        crate::host::shared_vm::operation("nodeErrorConstructor"),
        base,
    )?;
    let brand = context.host_function(crate::host::shared_vm::operation("nodeErrorBrand"))?;
    let code = context.string_rooted(INVALID_ARGUMENT_TYPE);
    let string = context
        .host_function_with_data(crate::host::shared_vm::operation("nodeErrorToString"), code)?;
    let key = context.symbol_rooted(Some("kIsNodeError"));
    let factory =
        context.evaluate_script_rooted(PROTOTYPE, "node:internal/errors/argument-prototype.js")?;
    let undefined = context.undefined();
    context.call_rooted(factory, undefined, &[base, constructor, brand, string, key])
}

pub(super) fn decorate_argument_type(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
) -> Result<(), RootedError> {
    super::util_shared_vm::module(context)?;
    let prototype = context
        .host_mut()
        .shared_state()
        .borrow()
        .util_intrinsics
        .ok_or_else(|| RootedError::host("argument error prototype missing"))?
        .argument_type_prototype;
    context.set_prototype_rooted(error, prototype)?;
    // Node's variadic error constructor installs its code field before message.
    let message_key = context.string_rooted("message");
    let message = context.get_property_rooted(error, message_key)?;
    let data = context
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("argument error assignment API missing"))?;
    let delete = get(context, data, "delete")?;
    let undefined = context.undefined();
    context.call_rooted(delete, undefined, &[error, message_key])?;
    let code = context.string_rooted(INVALID_ARGUMENT_TYPE);
    let key = context.string_rooted("code");
    context.define_data_property_rooted(error, key, code, true, true, true)?;
    context.define_data_property_rooted(error, message_key, message, true, false, true)?;
    Ok(())
}

pub(crate) fn constructor(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    context.host_function_data()
}

pub(crate) fn brand(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.boolean(true))
}

pub(crate) fn to_string(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let error = args.first().copied().unwrap_or(undefined);
    let code = context.host_function_data()?;
    let name = get(context, error, "name")?;
    let name = context.to_string(name)?;
    let code = context.to_string(code)?;
    let message = get(context, error, "message")?;
    let message = context.to_string(message)?;
    Ok(context.string_rooted(&format!("{name} [{code}]: {message}")))
}
