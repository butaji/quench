//! Node's coded error prototypes and shared private brand.

use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};

const NODE_ERROR_BRAND: &str = "\0node/errors/privateBrand";
pub(crate) const DEFAULT_STACK_FORMATTER: &str = "\0node/errors/defaultStackFormatter";
/// Node's stack policy, without loading it: `internal/errors.js` and what it requires cost
/// about as much as the rest of the bootstrap together, and a script that never reads a
/// stack or `Error.prepareStackTrace` needs none of it. The formatter is a native trampoline
/// that loads Node's callback on its first call, and `Error.prepareStackTrace` a lazy data
/// property; both load the library inside the startup intrinsics (`node_library_module`), so
/// it captures the original intrinsics whatever a script has done to them since.
pub(crate) fn install_stack_formatter(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<(), RootedError> {
    let formatter = context.host_function(crate::host::shared_vm::operation("errorStackFormat"))?;
    let formatter = context.retain(formatter)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(DEFAULT_STACK_FORMATTER.into(), formatter);
    let initializer = context.host_function(crate::host::shared_vm::operation(
        "errorPrepareStackTraceInitialize",
    ))?;
    let global = context.global_root()?;
    let constructor = get(context, global, "Error")?;
    context.define_lazy_data_properties_rooted(
        constructor,
        &["prepareStackTrace"],
        false,
        initializer,
    )
}

/// Materializes `Error.prepareStackTrace` if it is still lazy, which loads Node's primordials
/// and errors module in Node's order: both see the property unset, as an eager bootstrap
/// did, and it is defined once the module has loaded.
pub(crate) fn materialize_prepare_stack_trace(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<(), RootedError> {
    let global = context.global_root()?;
    let constructor = get(context, global, "Error")?;
    context.materialize_lazy_data_property_rooted(constructor, "prepareStackTrace")
}

/// One of `internal/errors.js`'s exports, loading the module on first use.
fn errors_export(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<RootId, RootedError> {
    materialize_prepare_stack_trace(context)?;
    library_errors_export(context, name)
}

fn library_errors_export(
    context: &mut NativeContext<'_, NodeHost>,
    name: &str,
) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_export(
        context,
        "internal/errors.js",
        "internal/errors.js",
        name,
    )
}

/// The stack formatter: Node's `prepareStackTraceCallback(global, error, sites)`.
pub(crate) fn format_stack(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let callback = errors_export(context, "prepareStackTraceCallback")?;
    let undefined = context.undefined();
    context.call_rooted(callback, undefined, args)
}

/// Defines `Error.prepareStackTrace` as Node's `ErrorPrepareStackTrace`, and keeps the
/// module's private brand as loaded, before script can reach its exports.
pub(crate) fn initialize_prepare_stack_trace(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let prepare = library_errors_export(context, "ErrorPrepareStackTrace")?;
    let global = context.global_root()?;
    let constructor = get(context, global, "Error")?;
    let key = context.string_rooted("prepareStackTrace");
    context.define_data_property_rooted(constructor, key, prepare, true, false, true)?;
    let brand = library_errors_export(context, "kIsNodeError")?;
    let brand = context.retain(brand)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .builtin_cache
        .insert(NODE_ERROR_BRAND.into(), brand);
    Ok(context.undefined())
}

const INVALID_ARGUMENT_TYPE: &str = "ERR_INVALID_ARG_TYPE";
pub(crate) struct NodeErrorApi {
    brand: RootId,
    prototypes: std::collections::HashMap<(&'static str, ErrorClass, ErrorPresentation), RootId>,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ErrorClass {
    Error,
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
        // Node's private brand, from its library when present.
        if crate::host::shared_vm::commonjs::node_library_present(context) {
            materialize_prepare_stack_trace(context)?;
        }
        let brand = state.borrow().builtin_cache.get(NODE_ERROR_BRAND).copied();
        let brand = brand.unwrap_or_else(|| context.symbol_rooted(Some("kIsNodeError")));
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
        ErrorClass::Error => context.error_rooted("")?,
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

pub(crate) fn decorate_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
) -> Result<(), RootedError> {
    decorate_coded_error(context, error, code, ErrorClass::Error, &[])
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

/// A ShadowRealm has its own original Node error module and primordial snapshot.
/// Its private fallback must never reach the principal realm's hooks or brands.
pub(crate) fn initialize_shadow_policy(
    context: &mut NativeContext<'_, NodeHost>,
    global: RootId,
) -> Result<RootId, RootedError> {
    let seed = context.null_object_rooted()?;
    let primordials = crate::host::shared_vm::commonjs::node_library_module_in_realm(
        context,
        global,
        "internal/per_context/primordials.js",
        seed,
        seed,
    )?;
    let exports = context.object_rooted()?;
    let errors = crate::host::shared_vm::commonjs::node_library_module_in_realm(
        context,
        global,
        "internal/errors.js",
        exports,
        primordials,
    )?;
    let constructor = get(context, global, "Error")?;
    let prepare = get(context, errors, "ErrorPrepareStackTrace")?;
    let key = context.string_rooted("prepareStackTrace");
    context.define_data_property_rooted(constructor, key, prepare, true, false, true)?;
    get(context, errors, "prepareStackTraceCallback")
}

/// Project compiler-owned error source metadata for the original Node SDK policy.
pub(crate) fn source_positions(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let error = args.first().copied().unwrap_or(absent);
    if !context.is_object_rooted(error)? {
        let error = context.type_error_rooted("Error source positions require an object")?;
        return Err(context.throw(error));
    }
    let Some(position) = context.error_source_position_rooted(error)? else {
        return Ok(context.undefined());
    };
    let result = context.null_object_rooted()?;
    let source_line = context.string_rooted(position.source_line.as_deref().unwrap_or(""));
    let resource = match position.script_name {
        Some(name) => context.string_rooted(&name),
        None => context.undefined(),
    };
    let line = context.number(f64::from(position.line_number));
    let column = context.number(f64::from(position.column_number.saturating_sub(1)));
    for (name, value) in [
        ("sourceLine", source_line),
        ("scriptResourceName", resource),
        ("lineNumber", line),
        ("startColumn", column),
    ] {
        let key = context.string_rooted(name);
        context.define_data_property_rooted(result, key, value, true, true, true)?;
    }
    Ok(result)
}

pub(crate) fn binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(
        context,
        "\0binding:errors",
        build_binding,
    )
}

fn build_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    let function = context.host_function_named(
        crate::host::shared_vm::operation("errorSourcePositions"),
        "getErrorSourcePositions",
    )?;
    let key = context.string_rooted("getErrorSourcePositions");
    context.define_data_property_rooted(binding, key, function, true, true, true)?;
    let function = context.host_function_named(
        crate::host::shared_vm::operation("errorSetSourceMapsEnabled"),
        "setSourceMapsEnabled",
    )?;
    let key = context.string_rooted("setSourceMapsEnabled");
    context.define_data_property_rooted(binding, key, function, true, true, true)?;
    let function = context.host_function_named(
        crate::host::shared_vm::operation("errorTriggerUncaughtException"),
        "triggerUncaughtException",
    )?;
    let key = context.string_rooted("triggerUncaughtException");
    context.define_data_property_rooted(binding, key, function, true, true, true)?;
    let codes = context.object_rooted()?;
    for &(name, code) in super::node_protocol_constants::EXIT_CODES {
        let key = context.string_rooted(name);
        let value = context.number(f64::from(code));
        context.define_data_property_rooted(codes, key, value, false, true, false)?;
    }
    let key = context.string_rooted("exitCodes");
    context.define_data_property_rooted(binding, key, codes, false, true, true)?;
    Ok(binding)
}

/// Node's source-map cache calls this internal engine hook after validating
/// the boolean option. Quench has no V8 source-map toggle, so the native hook
/// is intentionally inert while the JavaScript stack formatter remains in
/// control of stack presentation.
pub(crate) fn set_source_maps_enabled(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.undefined())
}

/// Node's internal errors binding routes Promise callback failures through the
/// same fatal-exception policy used by top-level unhandled Promise rejections.
pub(crate) fn trigger_uncaught_exception(
    context: &mut NativeContext<'_, NodeHost>,
    this: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    super::process_fatal_exception_route::dispatch(context, this, args)?;
    Ok(context.undefined())
}
