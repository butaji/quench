//! Node's coded error prototypes and shared private brand.

use super::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{DescriptorFields, NativeContext, RootId, RootedError};

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
        // Node's private brand (`kIsNodeError`).
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
        ErrorClass::Error => context.error_rooted("")?,
        ErrorClass::TypeError => context.type_error_rooted("")?,
        ErrorClass::RangeError => context.range_error_rooted("")?,
    };
    let base_prototype = context.prototype_rooted(base_error)?;
    let base = get(context, base_prototype, "constructor")?;
    let constructor = context.host_function_named_with_data(
        crate::host::shared_vm::operation("nodeErrorConstructor"),
        "get constructor",
        base,
    )?;
    let brand = context.host_function_named(crate::host::shared_vm::operation("nodeErrorBrand"), "get [kIsNodeError]")?;
    let code_root = context.string_rooted(code);
    let string = context.host_function_named_with_data(
        crate::host::shared_vm::operation("nodeErrorToString"),
        "toString",
        code_root,
    )?;
    let prototype = context.object_rooted()?;
    context.set_prototype_rooted(prototype, base_prototype)?;
    for (key, getter) in [(context.string_rooted("constructor"), constructor), (key, brand)] {
        context.define_property_fields_rooted(prototype, key, DescriptorFields {
            get: Some(getter), enumerable: Some(false), configurable: Some(true), ..Default::default()
        })?;
    }
    context.define_named_data_property_rooted(prototype, "toString", string, true, false, true)?;
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
        let constructor = context.host_function_named_with_data(
            crate::host::shared_vm::operation("nodeErrorConstructor"),
            "get constructor",
            constructor,
        )?;
        let prototype = context.object_rooted()?;
        let constructor_key = context.string_rooted("constructor");
        context.define_property_fields_rooted(prototype, constructor_key, DescriptorFields {
            get: Some(constructor), enumerable: Some(false), configurable: Some(true), ..Default::default()
        })?;
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

pub(crate) fn decorate_type_error(
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

fn decorate_coded_error(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    code: &'static str,
    class: ErrorClass,
    fields: &[(&str, RootId)],
) -> Result<(), RootedError> {
    let prototype = coded_error_prototype(context, code, class)?;
    context.set_prototype_rooted(error, prototype)?;
    // Node's variadic error constructor installs its code field before message.
    let message_key = context.string_rooted("message");
    let message = context.get_property_rooted(error, message_key)?;
    context.delete_property_rooted(error, message_key)?;
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


/// A coded error's `toString()`: `${name} [${code}]: ${message}`.
pub(crate) fn to_string(
    context: &mut NativeContext<'_, NodeHost>,
    error: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    let code = context.host_function_data()?;
    let name = get(context, error, "name")?;
    let name = context.to_string(name)?;
    let code = context.to_string(code)?;
    let message = get(context, error, "message")?;
    let message = context.to_string(message)?;
    Ok(context.string_rooted(&format!("{name} [{code}]: {message}")))
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

/// `ErrorClass` and `ErrorPresentation` tags, in declaration order.
const ERROR_CLASSES: [ErrorClass; 3] = [ErrorClass::Error, ErrorClass::TypeError, ErrorClass::RangeError];
const ERROR_PRESENTATIONS: [ErrorPresentation; 2] = [ErrorPresentation::Direct, ErrorPresentation::HiddenStackFrames];

impl crate::host::startup_snapshot::HostSnap for NodeErrorApi {
    fn write(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        let Self { brand, prototypes } = self;
        writer.root(*brand)?;
        writer.len(prototypes.len());
        for (&(code, class, presentation), prototype) in prototypes {
            writer.str(code);
            writer.u8(ERROR_CLASSES.iter().position(|candidate| *candidate == class).expect("listed class") as u8);
            writer.u8(
                ERROR_PRESENTATIONS.iter().position(|candidate| *candidate == presentation).expect("listed presentation")
                    as u8,
            );
            writer.root(*prototype)?;
        }
        Ok(())
    }
    fn read(reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<Self, String> {
        let brand = reader.root()?;
        let mut prototypes = std::collections::HashMap::new();
        for _ in 0..reader.len()? {
            // Error codes are the program's own literals; a restored process names each one
            // the snapshot cached once, for the life of the process, as its literal would.
            let code: &'static str = Box::leak(reader.string()?.into_boxed_str());
            let class = *ERROR_CLASSES.get(usize::from(reader.u8()?)).ok_or("invalid snapshot error class")?;
            let presentation =
                *ERROR_PRESENTATIONS.get(usize::from(reader.u8()?)).ok_or("invalid snapshot error presentation")?;
            prototypes.insert((code, class, presentation), reader.root()?);
        }
        Ok(Self { brand, prototypes })
    }
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

/// A coded error's brand getter (`kIsNodeError`): every coded error is one.
pub(crate) fn brand(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    _: &[RootId],
) -> Result<RootId, RootedError> {
    Ok(context.boolean(true))
}
