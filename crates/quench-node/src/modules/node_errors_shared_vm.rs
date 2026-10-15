//! Node's coded error prototypes and shared private brand.

use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const INVALID_ARGUMENT_TYPE: &str = "ERR_INVALID_ARG_TYPE";
pub(crate) struct NodeErrorApi {
    brand: RootId,
    prototypes: std::collections::HashMap<(&'static str, ErrorClass, ErrorPresentation), RootId>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorClass {
    TypeError,
    RangeError,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorPresentation {
    Direct,
    HiddenStackFrames,
}
const HIDDEN_PROTOTYPE: &str = quench_js_check::checked_js!(
    r#"constructorNative => class HideStackFramesError {
      get ['constructor']() { return constructorNative(); }
    }.prototype"#
);
const PROTOTYPE: &str = quench_js_check::checked_js!(
    r#"(Base, constructorNative, brandNative, stringNative, brand) =>
    class NodeError extends Base {
      get ['constructor']() { return constructorNative(); }
      get [brand]() { return brandNative(); }
      toString() { return stringNative(this); }
    }.prototype"#
);

fn coded_error_prototype(
    context: &mut NativeContext<'_, NodeHost>,
    code: &'static str,
    class: ErrorClass,
) -> Result<RootId, RootedError> {
    let state = context.host_mut().shared_state();
    if let Some(prototype) = state
        .borrow()
        .node_error_api
        .as_ref()
        .and_then(|api| {
            api.prototypes
                .get(&(code, class, ErrorPresentation::Direct))
        })
        .copied()
    {
        return Ok(prototype);
    }
    if state.borrow().node_error_api.is_none() {
        let brand = context.symbol_rooted(Some("kIsNodeError"));
        let brand = context.retain(brand)?;
        state.borrow_mut().node_error_api = Some(NodeErrorApi {
            brand,
            prototypes: std::collections::HashMap::new(),
        });
    }
    let key = state
        .borrow()
        .node_error_api
        .as_ref()
        .expect("initialized error API")
        .brand;
    let base_error = match class {
        ErrorClass::TypeError => context.type_error_rooted("")?,
        ErrorClass::RangeError => context.range_error_rooted("")?,
    };
    let base_prototype = context.prototype_rooted(base_error)?;
    let base = get(context, base_prototype, "constructor")?;
    let constructor = context.host_function_with_data(
        crate::host::shared_vm::operation("nodeErrorConstructor"),
        base,
    )?;
    let brand = context.host_function(crate::host::shared_vm::operation("nodeErrorBrand"))?;
    let code_root = context.string_rooted(code);
    let string = context.host_function_with_data(
        crate::host::shared_vm::operation("nodeErrorToString"),
        code_root,
    )?;
    let factory =
        context.evaluate_script_rooted(PROTOTYPE, "node:internal/errors/argument-prototype.js")?;
    let undefined = context.undefined();
    let prototype =
        context.call_rooted(factory, undefined, &[base, constructor, brand, string, key])?;
    let prototype = context.retain(prototype)?;
    state
        .borrow_mut()
        .node_error_api
        .as_mut()
        .expect("initialized error API")
        .prototypes
        .insert((code, class, ErrorPresentation::Direct), prototype);
    Ok(prototype)
}

/// Validators use a distinct subclass while inheriting the code and brand.
pub(super) fn hide_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    hide_error(context, error, code, ErrorClass::TypeError)
}

pub(super) fn hide_range_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    hide_error(context, error, code, ErrorClass::RangeError)
}

fn hide_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
    class: ErrorClass,
) -> Result<(), RootedError> {
    let base = coded_error_prototype(context, code, class)?;
    let state = context.host_mut().shared_state();
    let key = (code, class, ErrorPresentation::HiddenStackFrames);
    let cached = state
        .borrow()
        .node_error_api
        .as_ref()
        .and_then(|api| api.prototypes.get(&key))
        .copied();
    let prototype = if let Some(prototype) = cached {
        prototype
    } else {
        let constructor = get(context, base, "constructor")?;
        let constructor = context.host_function_with_data(
            crate::host::shared_vm::operation("nodeErrorConstructor"),
            constructor,
        )?;
        let factory = context.evaluate_script_rooted(
            HIDDEN_PROTOTYPE,
            "node:internal/errors/validator-prototype.js",
        )?;
        let undefined = context.undefined();
        let prototype = context.call_rooted(factory, undefined, &[constructor])?;
        context.set_prototype_rooted(prototype, base)?;
        let prototype = context.retain(prototype)?;
        state
            .borrow_mut()
            .node_error_api
            .as_mut()
            .expect("initialized error API")
            .prototypes
            .insert(key, prototype);
        prototype
    };
    context.set_prototype_rooted(error, prototype)?;
    Ok(())
}

pub(super) fn decorate_argument_type(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
) -> Result<(), RootedError> {
    decorate_type_error(context, error, INVALID_ARGUMENT_TYPE)
}

pub(super) fn decorate_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    decorate_coded_error(context, error, code, ErrorClass::TypeError, &[])
}

pub(super) fn decorate_range_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    decorate_coded_error(context, error, code, ErrorClass::RangeError, &[])
}

/// Fields produced by a Node error message constructor precede its message.
pub(super) fn decorate_type_error_with_fields(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
    fields: &[(&str, RootId)],
) -> Result<(), RootedError> {
    decorate_coded_error(context, error, code, ErrorClass::TypeError, fields)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageOrder {
    BeforeCode,
    AfterConstructorFields,
}

pub(super) fn decorate_formatted_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    decorate_ordered_error(
        context,
        error,
        code,
        ErrorClass::TypeError,
        &[],
        MessageOrder::BeforeCode,
    )
}

pub(super) fn decorate_formatted_range_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    decorate_ordered_error(
        context,
        error,
        code,
        ErrorClass::RangeError,
        &[],
        MessageOrder::BeforeCode,
    )
}

fn decorate_coded_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
    class: ErrorClass,
    fields: &[(&str, RootId)],
) -> Result<(), RootedError> {
    decorate_ordered_error(
        context,
        error,
        code,
        class,
        fields,
        MessageOrder::AfterConstructorFields,
    )
}

fn decorate_ordered_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
    class: ErrorClass,
    fields: &[(&str, RootId)],
    order: MessageOrder,
) -> Result<(), RootedError> {
    super::util_shared_vm::module(context)?;
    let prototype = coded_error_prototype(context, code, class)?;
    context.set_prototype_rooted(error, prototype)?;
    // Node's variadic error constructor installs its code field before message.
    let message_key = context.string_rooted("message");
    let message = context.get_property_rooted(error, message_key)?;
    if order == MessageOrder::AfterConstructorFields {
        let data = context
            .host_mut()
            .shared_state()
            .borrow()
            .event_targets_api
            .ok_or_else(|| RootedError::host("argument error assignment API missing"))?;
        let delete = get(context, data, "delete")?;
        let undefined = context.undefined();
        context.call_rooted(delete, undefined, &[error, message_key])?;
    }
    let code = context.string_rooted(code);
    let key = context.string_rooted("code");
    context.define_data_property_rooted(error, key, code, true, true, true)?;
    for &(name, value) in fields {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(error, key, value, true, true, true)?;
    }
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
