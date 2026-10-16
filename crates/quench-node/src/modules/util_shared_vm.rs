//! Shared-VM utilities and native builtin-brand predicates.

use crate::modules::event_constructor_shared_vm::get;
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};

const TYPE_PREDICATES: &[(&str, BuiltinBrand)] = &[
    ("isDate", BuiltinBrand::Date),
    ("isRegExp", BuiltinBrand::RegExp),
    ("isNativeError", BuiltinBrand::NativeError),
    ("isPromise", BuiltinBrand::Promise),
    ("isMap", BuiltinBrand::Map),
    ("isSet", BuiltinBrand::Set),
    ("isWeakMap", BuiltinBrand::WeakMap),
    ("isWeakSet", BuiltinBrand::WeakSet),
    ("isProxy", BuiltinBrand::Proxy),
    ("isArrayBuffer", BuiltinBrand::ArrayBuffer),
    ("isSharedArrayBuffer", BuiltinBrand::SharedArrayBuffer),
    ("isAnyArrayBuffer", BuiltinBrand::AnyArrayBuffer),
    ("isExternal", BuiltinBrand::External),
    ("isDataView", BuiltinBrand::DataView),
    ("isArgumentsObject", BuiltinBrand::ArgumentsObject),
    ("isAsyncFunction", BuiltinBrand::AsyncFunction),
    ("isGeneratorFunction", BuiltinBrand::GeneratorFunction),
    ("isGeneratorObject", BuiltinBrand::GeneratorObject),
    ("isMapIterator", BuiltinBrand::MapIterator),
    ("isSetIterator", BuiltinBrand::SetIterator),
    (
        "isModuleNamespaceObject",
        BuiltinBrand::ModuleNamespaceObject,
    ),
    ("isBooleanObject", BuiltinBrand::BooleanObject),
    ("isNumberObject", BuiltinBrand::NumberObject),
    ("isStringObject", BuiltinBrand::StringObject),
    ("isSymbolObject", BuiltinBrand::SymbolObject),
    ("isBigIntObject", BuiltinBrand::BigIntObject),
    ("isBoxedPrimitive", BuiltinBrand::BoxedPrimitive),
    ("isUint8Array", BuiltinBrand::Uint8Array),
    ("isUint8ClampedArray", BuiltinBrand::Uint8ClampedArray),
    ("isUint16Array", BuiltinBrand::Uint16Array),
    ("isUint32Array", BuiltinBrand::Uint32Array),
    ("isInt8Array", BuiltinBrand::Int8Array),
    ("isInt16Array", BuiltinBrand::Int16Array),
    ("isInt32Array", BuiltinBrand::Int32Array),
    ("isBigInt64Array", BuiltinBrand::BigInt64Array),
    ("isBigUint64Array", BuiltinBrand::BigUint64Array),
    ("isFloat16Array", BuiltinBrand::Float16Array),
    ("isFloat32Array", BuiltinBrand::Float32Array),
    ("isFloat64Array", BuiltinBrand::Float64Array),
];

// Node src/node_util.cc validates the native stack-frame request against this limit.
const NODE_CALL_SITE_FRAME_LIMIT: usize = 200;

pub(crate) const MAX_SAFE_INTEGER: f64 = ((1_u64 << f64::MANTISSA_DIGITS) - 1) as f64;

const UTIL: &str = quench_js_check::checked_js!(
    r#"((inspect, format, formatWithOptions, warnedCodes, getCallSites) => {
  const inherits = (ctor, superCtor) => {
    const invalidType = (kind, name, expected, received) => {
      const error = new TypeError(
        `The "${name}" ${kind} must be of type ${expected}. Received ${received}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      return error;
    };
    if (ctor === undefined || ctor === null) {
      throw invalidType("argument", "ctor", "function", ctor);
    }
    if (superCtor === undefined || superCtor === null) {
      throw invalidType("argument", "superCtor", "function", superCtor);
    }
    if (superCtor.prototype === undefined) {
      throw invalidType(
        "property",
        "superCtor.prototype",
        "object",
        superCtor.prototype,
      );
    }
    Object.defineProperty(ctor, "super_", {
      value: superCtor,
      writable: true,
      configurable: true,
    });
    Object.setPrototypeOf(ctor.prototype, superCtor.prototype);
  };
  const debuglog = (section) => {
    if (typeof section !== "string") {
      throw new TypeError("The \"section\" argument must be of type string");
    }
    const debug = () => {};
    debug.enabled = false;
    return debug;
  };
  const warnedFunctions = new WeakSet();
  const deprecate = (callback, message = "", code, options = {}) => {
    if (typeof callback !== "function") {
      throw new TypeError("The \"fn\" argument must be of type function");
    }
    if (code !== undefined && typeof code !== "string") {
      const received = code === null
        ? " Received null"
        : typeof code === "object"
        ? " Received an instance of Object"
        : ` Received type ${typeof code} (${String(code)})`;
      const error = new TypeError(
        `The \"code\" argument must be of type string.${received}`
      );
      error.code = "ERR_INVALID_ARG_TYPE";
      throw error;
    }

    function deprecated(...args) {
      const alreadyWarned = code === undefined
        ? warnedFunctions.has(callback)
        : warnedCodes.has(code);
      if (!alreadyWarned) {
        if (code === undefined) warnedFunctions.add(callback);
        else warnedCodes.add(code);
        if (typeof process.emitWarning === "function") {
          process.emitWarning(message, { type: "DeprecationWarning", code });
        }
      }
      if (new.target) {
        return Reflect.construct(callback, args, new.target === deprecated ? callback : new.target);
      }
      return Reflect.apply(callback, this, args);
    }

    Object.defineProperty(deprecated, "length", { value: callback.length });
    Object.defineProperty(deprecated, "name", { value: callback.name, configurable: true });
    if (options.modifyPrototype === false) {
      deprecated.prototype = {};
    } else {
      deprecated.prototype = callback.prototype;
      Object.setPrototypeOf(deprecated, callback);
    }
    return deprecated;
  };
  const promisifyCustom = Symbol.for("nodejs.util.promisify.custom");
  const promisifyCustomArgs = Symbol.for("nodejs.util.promisify.customArgs");
  const promisify = (original) => {
    if (typeof original !== "function") {
      const received = original === null
        ? " Received null"
        : ` Received type ${typeof original} (${String(original)})`;
      throw Object.assign(
        new TypeError(`The "original" argument must be of type function.${received}`),
        { code: "ERR_INVALID_ARG_TYPE" },
      );
    }
    const custom = original[promisifyCustom];
    if (custom !== undefined) {
      if (typeof custom !== "function") {
        throw Object.assign(
          new TypeError('The "util.promisify.custom" property must be of type function'),
          { code: "ERR_INVALID_ARG_TYPE" },
        );
      }
      Object.defineProperty(custom, promisifyCustom, {
        value: custom,
        configurable: true,
      });
      return custom;
    }
    const argumentNames = original[promisifyCustomArgs];
    function promisified(...args) {
      return new Promise((resolve, reject) => {
        args.push((error, ...values) => {
          if (error) return reject(error);
          if (argumentNames !== undefined && values.length > 1) {
            const result = {};
            for (let index = 0; index < argumentNames.length; index++) {
              result[argumentNames[index]] = values[index];
            }
            resolve(result);
          } else {
            resolve(values[0]);
          }
        });
        Reflect.apply(original, this, args);
      });
    }
    Object.setPrototypeOf(promisified, Object.getPrototypeOf(original));
    Object.defineProperty(promisified, promisifyCustom, {
      value: promisified,
      configurable: true,
    });
    Object.defineProperties(promisified, Object.getOwnPropertyDescriptors(original));
    return promisified;
  };
  promisify.custom = promisifyCustom;
  return {
    format,
    formatWithOptions,
    inspect,
    getCallSites,
    inherits,
    debuglog,
    deprecate,
    promisify,
    TextEncoder: globalThis.TextEncoder,
    TextDecoder: globalThis.TextDecoder,
  };
})"#
);

/// The public module and original inspector share one canonical function identity.
#[derive(Clone, Copy)]
pub(crate) struct UtilIntrinsics {
    pub(crate) module: RootId,
    pub(crate) inspect: RootId,
    pub(crate) formatters: RootId,
}

pub(crate) fn internal_formatters(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    module(context)?;
    context.host_mut().shared_state().borrow().util_intrinsics
        .map(|api| api.formatters)
        .ok_or_else(|| RootedError::host("util formatters missing"))
}

pub(crate) fn internal_inspect(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<RootId, RootedError> {
    module(context)?;
    context
        .host_mut()
        .shared_state()
        .borrow()
        .util_intrinsics
        .map(|api| api.inspect)
        .ok_or_else(|| RootedError::host("util intrinsics missing"))
}

/// Capture the configured call-site policy before guests can replace internal
/// validator exports. The original Node module retains those lexical imports.
pub(crate) fn initialize_call_site_policy(
    context: &mut NativeContext<'_, NodeHost>,
) -> Result<(), RootedError> {
    crate::host::shared_vm::commonjs::node_library_export(
        context, "util.js", "util.js", "getCallSites",
    )?;
    Ok(())
}

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let cached = context.host_mut().shared_state().borrow().util_intrinsics;
    if let Some(api) = cached {
        return Ok(api.module);
    }
    let inspect = crate::host::shared_vm::commonjs::node_library_export(context, "util.js", "internal/util/inspect.js", "inspect")?;
    let format = crate::host::shared_vm::commonjs::node_library_export(context, "util.js", "internal/util/inspect.js", "format")?;
    let format_with_options = crate::host::shared_vm::commonjs::node_library_export(context, "util.js", "internal/util/inspect.js", "formatWithOptions")?;
    let get_call_sites = crate::host::shared_vm::commonjs::node_library_export(
        context, "util.js", "util.js", "getCallSites",
    )?;
    let factory = context.evaluate_script_rooted(UTIL, "node:util/shared.js")?;
    let undefined = context.undefined();
    let global = context.global_root()?;
    let set_key = context.string_rooted("Set");
    let set = context.get_property_rooted(global, set_key)?;
    let warned_codes = context.construct_rooted(set, set, &[])?;
    let module = context.call_rooted(factory, undefined, &[inspect, format, format_with_options, warned_codes, get_call_sites])?;
    super::util_style_shared_vm::install(context, module, inspect)?;
    super::util_extend_shared_vm::install(context, module, warned_codes)?;
    let compare = context.host_function(crate::host::shared_vm::operation("isDeepStrictEqual"))?;
    let compare_key = context.string_rooted("isDeepStrictEqual");
    context.set_property_rooted(module, compare_key, compare, module)?;
    for name in ["stripVTControlCharacters", "parseEnv", "getSystemErrorName"] {
        let function = context.host_function(crate::host::shared_vm::operation(name))?;
        let key = context.string_rooted(name);
        context.set_property_rooted(module, key, function, module)?;
    }
    let callbackify = crate::modules::util_callbackify_shared_vm::function(context)?;
    let parse_args = super::util_parse_args_shared_vm::function(context)?;
    let parse_args_key = context.string_rooted("parseArgs");
    context.set_property_rooted(module, parse_args_key, parse_args, module)?;
    let callbackify_key = context.string_rooted("callbackify");
    context.set_property_rooted(module, callbackify_key, callbackify, module)?;
    let types = crate::host::shared_vm::commonjs::node_library_module(
        context, "util.js", "internal/util/types.js",
    )?;
    let key = context.string_rooted("types");
    context.set_property_rooted(module, key, types, module)?;
    // Retain original identities before publishing mutable util exports. Console
    // can request these lazily without observing later public export replacement.
    let formatters = context.null_object_rooted()?;
    for name in ["format", "formatWithOptions"] {
        let key = context.string_rooted(name);
        let function = context.get_property_rooted(module, key)?;
        context.define_data_property_rooted(formatters, key, function, false, true, false)?;
    }
    let module = context.retain(module)?;
    let inspect = context.retain(inspect)?;
    let formatters = context.retain(formatters)?;
    context
        .host_mut()
        .shared_state()
        .borrow_mut()
        .util_intrinsics = Some(UtilIntrinsics { module, inspect, formatters });
    Ok(module)
}

pub(crate) fn type_check(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let data = context.host_function_data()?;
    let index = context
        .rooted_value(data)
        .and_then(|value| value.as_number())
        .ok_or_else(|| RootedError::host("missing util predicate brand"))? as usize;
    let brand = TYPE_PREDICATES
        .get(index)
        .ok_or_else(|| RootedError::host("invalid util predicate brand"))?
        .1;
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let matches = context.has_builtin_brand_rooted(value, brand)?;
    Ok(context.boolean(matches))
}

pub(crate) fn strip_vt(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(units) = context.string_code_units(value)? else {
        return Err(argument_type_error(context, value, "str", "string")?);
    };
    if !crate::modules::util_strip::has_vt_controls(&units) {
        return Ok(value);
    }
    let stripped = crate::modules::util_strip::strip_vt_control_units(&units);
    if stripped == units {
        return Ok(value);
    }
    Ok(context.string_units_rooted(&stripped))
}

pub(crate) fn parse_env(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(text) = context.string_text(value)? else {
        return Err(argument_type_error(context, value, "content", "string")?);
    };
    let result = context.null_object_rooted()?;
    for (name, value) in crate::modules::util_env::parse_env_file(&text) {
        let key = context.string_rooted(&name);
        let value = context.string_rooted(&value);
        context.define_data_property_rooted(result, key, value, true, true, true)?;
    }
    Ok(result)
}

/// Node inspect string literals: pick a delimiter, escape controls and lone
/// surrogates, and preserve valid UTF-16 pairs without host-text conversion.
pub(crate) fn inspect_quote(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    let units = context
        .string_code_units(value)?
        .ok_or_else(|| RootedError::host("inspect quote requires string code units"))?;
    let delimiter = if !units.contains(&(b'\'' as u16)) {
        b'\''
    } else if !units.contains(&(b'"' as u16)) {
        b'"'
    } else if !units.contains(&(b'`' as u16))
        && !units
            .windows(2)
            .any(|pair| pair == [b'$' as u16, b'{' as u16])
    {
        b'`'
    } else {
        b'\''
    };
    let mut output = vec![u16::from(delimiter)];
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        let escape = match unit {
            8 => Some("\\b".to_owned()),
            9 => Some("\\t".to_owned()),
            10 => Some("\\n".to_owned()),
            12 => Some("\\f".to_owned()),
            13 => Some("\\r".to_owned()),
            92 => Some("\\\\".to_owned()),
            _ if unit == u16::from(delimiter) && delimiter == b'\'' => Some("\\'".to_owned()),
            0..=31 | 127..=159 => Some(format!("\\x{unit:02X}")),
            0xd800..=0xdbff
                if units
                    .get(index + 1)
                    .is_some_and(|unit| (0xdc00..=0xdfff).contains(unit)) =>
            {
                output.extend_from_slice(&units[index..index + 2]);
                index += 2;
                continue;
            }
            0xd800..=0xdfff => Some(format!("\\u{unit:04x}")),
            _ => None,
        };
        if let Some(escape) = escape {
            output.extend(escape.encode_utf16());
        } else {
            output.push(unit);
        }
        index += 1;
    }
    output.push(u16::from(delimiter));
    Ok(context.string_units_rooted(&output))
}

/// Shared Node AbortSignal shape validation. HasProperty observes Proxy traps
/// and inherited aborted properties; functions and null are rejected.
pub(crate) fn validate_abort_signal(
    context: &mut NativeContext<'_, NodeHost>,
    signal: RootId,
    name: &str,
) -> Result<(), RootedError> {
    if context
        .rooted_value(signal)
        .is_some_and(|value| value.is_undefined())
    {
        return Ok(());
    }
    let valid = context.is_object_rooted(signal)? && !context.is_callable_rooted(signal)?;
    let key = context.string_rooted("aborted");
    if !valid || !context.has_property_rooted(signal, key)? {
        return Err(argument_type_error(context, signal, name, "AbortSignal")?);
    }
    Ok(())
}

pub(crate) fn argument_type_error(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
    name: &str,
    expected: &str,
) -> Result<RootedError, RootedError> {
    argument_type_error_units(context, root, &name.encode_utf16().collect::<Vec<_>>(), expected)
}

pub(crate) fn argument_type_error_units(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
    name: &[u16],
    expected: &str,
) -> Result<RootedError, RootedError> {
    // Node treats the validator's "Object" token as the object primitive type.
    let expected = if expected == "Object" {
        "of type object".to_owned()
    } else if expected.chars().next().is_some_and(char::is_uppercase) {
        format!("an instance of {expected}")
    } else {
        format!("of type {expected}")
    };
    argument_type_error_with_expected_units(context, root, name, &expected)
}

/// Already formatted union of accepted Node argument types.
pub(crate) fn argument_type_error_with_expected(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
    name: &str,
    expected: &str,
) -> Result<RootedError, RootedError> {
    argument_type_error_with_expected_units(context, root, &name.encode_utf16().collect::<Vec<_>>(), expected)
}

pub(crate) fn argument_type_error_with_expected_units(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
    name: &[u16],
    expected: &str,
) -> Result<RootedError, RootedError> {
    let received = argument_type_description(context, root)?;
    let mut units = "The ".encode_utf16().collect::<Vec<_>>();
    if name.ends_with(&" argument".encode_utf16().collect::<Vec<_>>()) {
        units.extend_from_slice(name);
    } else {
        units.push(u16::from(b'"'));
        units.extend_from_slice(name);
        units.extend(if name.contains(&u16::from(b'.')) { "\" property" } else { "\" argument" }.encode_utf16());
    }
    units.extend(format!(" must be {expected}. Received {received}").encode_utf16());
    let error = context.type_error_rooted("")?;
    let message = context.string_units_rooted(&units);
    context.assign_named_property_rooted(error, "message", message)?;
    super::node_errors_shared_vm::decorate_argument_type(context, error)?;
    Ok(context.throw(error))
}

/// Node's value error inspects the original input and caps UTF-16 presentation.
pub(crate) fn argument_value_error(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
    reason: &str,
) -> Result<RootedError, RootedError> {
    argument_value_error_units(context, value, &name.encode_utf16().collect::<Vec<_>>(), reason)
}

pub(crate) fn argument_value_error_units(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &[u16],
    reason: &str,
) -> Result<RootedError, RootedError> {
    const MAX_INSPECTED_VALUE_UNITS: usize = 128;
    let inspect = internal_inspect(context)?;
    let undefined = context.undefined();
    let inspected = context.call_rooted(inspect, undefined, &[value])?;
    let mut detail = context
        .string_code_units(inspected)?
        .ok_or_else(|| RootedError::host("inspect returned a non-string"))?;
    if detail.len() > MAX_INSPECTED_VALUE_UNITS {
        detail.truncate(MAX_INSPECTED_VALUE_UNITS);
        detail.extend("...".encode_utf16());
    }
    let kind = if name.contains(&u16::from(b'.')) {
        "property"
    } else {
        "argument"
    };
    let mut units = format!("The {kind} '").encode_utf16().collect::<Vec<_>>();
    units.extend_from_slice(name);
    units.extend(format!("' {reason}. Received ").encode_utf16());
    units.extend(detail);
    let message = context.string_units_rooted(&units);
    let error = context.type_error_rooted("")?;
    let key = context.string_rooted("message");
    context.set_property_rooted(error, key, message, error)?;
    super::node_errors_shared_vm::decorate_type_error(context, error, "ERR_INVALID_ARG_VALUE")?;
    Ok(context.throw(error))
}

/// Shared Node received-type description for coded argument and port errors.
pub(crate) fn argument_type_description(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
) -> Result<String, RootedError> {
    let value = context
        .rooted_value(root)
        .ok_or_else(|| RootedError::host("invalid argument root"))?;
    let received = if value.is_undefined() {
        "undefined".to_owned()
    } else if value.is_null() {
        "null".to_owned()
    } else if value.as_bool().is_some() {
        format!("type boolean ({})", context.to_string(root)?)
    } else if value.as_number().is_some() {
        format!("type number ({})", context.to_string(root)?)
    } else if let Some(text) = context.string_text(root)? {
        format!(
            "type string ('{}')",
            text.replace('\\', "\\\\").replace('\'', "\\'")
        )
    } else if context.is_symbol_rooted(root)? {
        let global = context.global_root()?;
        let key = context.string_rooted("String");
        let string = context.get_property_rooted(global, key)?;
        let undefined = context.undefined();
        let text = context.call_rooted(string, undefined, &[root])?;
        format!("type symbol ({})", context.to_string(text)?)
    } else if context.is_callable_rooted(root)? {
        let key = context.string_rooted("name");
        let name = context.get_property_rooted(root, key)?;
        format!("function {}", context.to_string(name)?)
    } else if context.is_object_rooted(root)? {
        let key = context.string_rooted("constructor");
        let constructor = context.get_property_rooted(root, key)?;
        let name = context.string_rooted("name");
        if context.truthy_rooted(constructor)? {
            let constructor = context.get_property_rooted(root, key)?;
            if context.has_property_rooted(constructor, name)? {
                let constructor = context.get_property_rooted(root, key)?;
                let name = context.get_property_rooted(constructor, name)?;
                format!("an instance of {}", context.to_string(name)?)
            } else {
                "an instance of Object".to_owned()
            }
        } else {
            let inspect = internal_inspect(context)?;
            let options = context.null_object_rooted()?;
            let depth = context.string_rooted("depth");
            let shallow = context.number(-1.0);
            context.define_data_property_rooted(options, depth, shallow, true, true, true)?;
            let undefined = context.undefined();
            let text = context.call_rooted(inspect, undefined, &[root, options])?;
            context.to_string(text)?
        }
    } else {
        format!("type bigint ({}n)", context.to_string(root)?)
    };
    Ok(received)
}

/// ERR_OUT_OF_RANGE rendering shares Node inspection and integer separators.
pub(crate) fn range_error_received(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
) -> Result<String, RootedError> {
    if let Some(number) = context.rooted_value(value).and_then(|value| value.as_number()) {
        let text = context.to_string(value)?;
        if number == 0.0 && number.is_sign_negative() { return Ok("-0".to_owned()); }
        return Ok(if number.fract() == 0.0 && number.abs() > f64::from(u32::MAX) + 1.0 {
            integer_separators(&text)
        } else { text });
    }
    let inspect = internal_inspect(context)?;
    let undefined = context.undefined();
    let inspected = context.call_rooted(inspect, undefined, &[value])?;
    let text = context.to_string(inspected)?;
    if !context.is_object_rooted(value)? {
        if let Some(integer) = text.strip_suffix('n') {
            let magnitude = integer.strip_prefix('-').unwrap_or(integer);
            let boundary = (u32::MAX as u64 + 1).to_string();
            if magnitude.len() > boundary.len() || (magnitude.len() == boundary.len() && magnitude > boundary.as_str()) {
                return Ok(format!("{}n", integer_separators(integer)));
            }
        }
    }
    Ok(text)
}

fn integer_separators(text: &str) -> String {
    let start = usize::from(text.starts_with('-'));
    let mut result = String::with_capacity(text.len() + text.len() / 3);
    for (index, character) in text.char_indices() {
        if index > start && (text.len() - index) % 3 == 0 { result.push('_'); }
        result.push(character);
    }
    result
}

/// Node integer validators reject coercion of the value, then compare original bounds.
pub(crate) fn validate_integer(context: &mut NativeContext<'_, NodeHost>, value: RootId, name: &str, minimum: f64, maximum: RootId) -> Result<f64, RootedError> {
    let Some(number) = context.rooted_value(value).and_then(|v| v.as_number()) else {
        return Err(argument_type_error(context, value, name, "number")?);
    };
    let requirement = if !number.is_finite() || number.fract() != 0.0 { Some("an integer".to_owned()) }
        else if number < minimum || context.greater_than_rooted(value, maximum)? {
            Some(format!(">= {minimum} && <= {}", context.to_string(maximum)?))
        } else { None };
    if let Some(requirement) = requirement {
        let received = range_error_received(context, value)?;
        let error = context.range_error_rooted(&format!("The value of \"{name}\" is out of range. It must be {requirement}. Received {received}"))?;
        let key = context.string_rooted("code");let code=context.string_rooted("ERR_OUT_OF_RANGE");context.set_property_rooted(error,key,code,error)?;
        return Err(context.throw(error));
    }
    Ok(number)
}

/// C++ binding validators use intrinsic errors with message before enumerable code,
/// unlike the coded subclasses exposed by Node's JavaScript validators.
pub(crate) fn validate_binding_integer(
    context: &mut NativeContext<'_, NodeHost>,
    value: RootId,
    name: &str,
    minimum: f64,
    maximum: RootId,
) -> Result<f64, RootedError> {
    match validate_integer(context, value, name, minimum, maximum) {
        Ok(number) => Ok(number),
        Err(original) => {
            let Some(exception) = original.exception else {
                return Err(original);
            };
            let key = context.string_rooted("message");
            let message = context.get_property_rooted(exception, key)?;
            let message = context.to_string(message)?;
            let (error, code) = if context
                .rooted_value(value)
                .and_then(|value| value.as_number())
                .is_some()
            {
                (context.range_error_rooted(&message)?, "ERR_OUT_OF_RANGE")
            } else {
                (context.type_error_rooted(&message)?, "ERR_INVALID_ARG_TYPE")
            };
            let key = context.string_rooted("code");
            let code = context.string_rooted(code);
            context.define_data_property_rooted(error, key, code, true, true, true)?;
            Err(context.throw(error))
        }
    }
}

const ONLY_WRITABLE: u32 = 1;
const ONLY_ENUMERABLE: u32 = 2;
const ONLY_CONFIGURABLE: u32 = 4;
const SKIP_STRINGS: u32 = 8;
const SKIP_SYMBOLS: u32 = 16;

/// V8's internal utility returns string keys, excluding array indices, with
/// the requested enumerable filter. The Core internal methods own key semantics.
pub(crate) fn own_non_index_properties(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let missing = context.undefined();
    let object = args.first().copied().unwrap_or(missing);
    let flags = args.get(1).and_then(|root| context.rooted_value(*root))
        .and_then(|value| value.as_number()).unwrap_or_default() as u32;
    // V8's own-only key accumulator treats proxy results as trap snapshots:
    // ALL_PROPERTIES preserves integer keys as numbers, while filtered queries
    // skip indices and only the enumerable filter consults descriptors.
    if context.proxy_target_rooted(object)?.is_some() {
        let mut keys = Vec::new();
        for key in context.own_keys_rooted(object)? {
            if let Some(text) = context.string_text(key)? {
                if flags & SKIP_STRINGS != 0 { continue; }
                let index = text.parse::<u32>().ok().filter(|index| *index != u32::MAX && index.to_string() == text);
                if let Some(index) = index {
                    if flags == 0 { keys.push(context.number(f64::from(index))); }
                    continue;
                }
            } else if flags & SKIP_SYMBOLS != 0 { continue; }
            if flags & ONLY_ENUMERABLE != 0 {
                let descriptor = context.own_property_descriptor_rooted(object, key)?;
                if !context.is_object_rooted(descriptor)? { continue; }
                let enumerable = get(context, descriptor, "enumerable")?;
                if !context.truthy_rooted(enumerable)? { continue; }
            }
            keys.push(key);
        }
        return context.array_rooted(&keys);
    }
    let mut keys = Vec::new();
    for key in context.own_non_index_keys_rooted(object, flags & ONLY_ENUMERABLE != 0)? {
        let string = context.string_text(key)?.is_some();
        if (string && flags & SKIP_STRINGS != 0) || (!string && flags & SKIP_SYMBOLS != 0) { continue; }
        if flags & (ONLY_WRITABLE | ONLY_CONFIGURABLE) != 0 {
            let descriptor = context.own_property_descriptor_rooted(object, key)?;
            if !context.is_object_rooted(descriptor)? { continue; }
            if flags & ONLY_WRITABLE != 0 {
                let name = context.string_rooted("writable");
                let writable = context.own_property_descriptor_rooted(descriptor, name)?;
                // V8 accessors have no READ_ONLY attribute. A data property's
                // explicit writable=false is the only rejection here.
                if context.is_object_rooted(writable)? {
                    let value = get(context, writable, "value")?;
                    if !context.truthy_rooted(value)? { continue; }
                }
            }
            if flags & ONLY_CONFIGURABLE != 0 {
                let value = get(context, descriptor, "configurable")?;
                if !context.truthy_rooted(value)? { continue; }
            }
        }
        keys.push(key);
    }
    context.array_rooted(&keys)
}

/// Inspect Core's promise slots without invoking guest properties or reactions.
pub(crate) fn promise_details(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    use quench_runtime::PromiseSnapshot;
    let promise = args.first().copied().unwrap_or_else(|| context.undefined());
    let Some(snapshot) = context.promise_snapshot_rooted(promise)? else { return Ok(context.undefined()); };
    let (state, result) = match snapshot {
        PromiseSnapshot::Pending => {
            let state = context.number(0.0);
            return context.array_rooted(&[state]);
        },
        PromiseSnapshot::Fulfilled(value) => (1, value),
        PromiseSnapshot::Rejected(value) => (2, value),
    };
    let state = context.number(f64::from(state));
    context.array_rooted(&[state, result])
}

pub(crate) fn mark_promise_handled(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let promise = args.first().copied().unwrap_or_else(|| context.undefined());
    context.mark_promise_handled_rooted(promise)?;
    Ok(context.undefined())
}

/// Cache the genuine internal key binding in Core-owned private formatter data.
pub(crate) fn key_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(context, "\0binding:util", build_key_binding)
}
fn build_key_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    for (name, operation) in [
        ("getConstructorName", "utilConstructorName"),
        ("getCallSites", "utilGetCallSites"),
        ("getOwnNonIndexProperties", "utilOwnNonIndexProperties"),
        ("getPromiseDetails", "utilPromiseDetails"),
        ("getProxyDetails", "utilProxyDetails"),
        ("previewEntries", "utilPreviewEntries"),
        ("getExternalValue", "utilExternalValue"),
        ("markPromiseAsHandled", "utilMarkPromiseHandled"),
    ] {
        let operation = crate::host::shared_vm::operation(operation);
        let function = context.host_function_named(operation, name)?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(binding, key, function, true, true, true)?;
    }
    let symbols = context.object_rooted()?;
    for (name, description) in [
        ("arrow_message_private_symbol", "node:arrowMessage"),
        ("decorated_private_symbol", "node:decorated"),
        ("transfer_mode_private_symbol", "node:transfer_mode"),
    ] {
        let key = context.string_rooted(name);
        let symbol = context.symbol_rooted(Some(description));
        context.define_data_property_rooted(symbols, key, symbol, true, true, true)?;
    }
    let key = context.string_rooted("privateSymbols");
    context.define_data_property_rooted(binding, key, symbols, true, true, true)?;
    let constants = context.object_rooted()?;
    for (name, number) in [
        ("kPending",0),("kFulfilled",1),("kRejected",2),
        ("kExiting",0),("kExitCode",1),("kHasExitCode",2),
        ("ALL_PROPERTIES",0),("ONLY_WRITABLE",ONLY_WRITABLE),
        ("ONLY_ENUMERABLE",ONLY_ENUMERABLE),("ONLY_CONFIGURABLE",ONLY_CONFIGURABLE),
        ("SKIP_STRINGS",SKIP_STRINGS),("SKIP_SYMBOLS",SKIP_SYMBOLS),
        ("kDisallowCloneAndTransfer",0),("kTransferable",1),("kCloneable",2),
    ] {
        let key = context.string_rooted(name);
        let value = context.number(f64::from(number));
        context.define_data_property_rooted(constants, key, value, true, true, true)?;
    }
    let key = context.string_rooted("constants");
    context.define_data_property_rooted(binding, key, constants, true, true, true)?;
    Ok(binding)
}

/// Messaging hooks use realm-owned unique symbols, never registry aliases.
pub(crate) fn symbols_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(context, "\0binding:symbols", build_symbols_binding)
}
fn build_symbols_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let binding = context.object_rooted()?;
    for name in ["messaging_deserialize_symbol", "messaging_transfer_symbol", "messaging_clone_symbol", "messaging_transfer_list_symbol"] {
        let key = context.string_rooted(name);
        let symbol = context.symbol_rooted(Some(name));
        context.define_data_property_rooted(binding, key, symbol, true, true, true)?;
    }
    Ok(binding)
}

pub(crate) fn types_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(context, "\0binding:types", build_types_binding)
}
fn build_types_binding(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    // node_types.cc's raw native slot predicates, in its export order. The
    // configured internal/util/types.js owns JavaScript predicates and aliases.
    use BuiltinBrand as B;
    const NATIVE_BRANDS: &[BuiltinBrand] = &[
        B::ArgumentsObject,
        B::ArrayBuffer,
        B::AsyncFunction,
        B::BigIntObject,
        B::BooleanObject,
        B::Date,
        B::External,
        B::GeneratorFunction,
        B::GeneratorObject,
        B::Map,
        B::MapIterator,
        B::ModuleNamespaceObject,
        B::NativeError,
        B::NumberObject,
        B::Promise,
        B::Proxy,
        B::RegExp,
        B::Set,
        B::SetIterator,
        B::SharedArrayBuffer,
        B::StringObject,
        B::SymbolObject,
        B::WeakMap,
        B::WeakSet,
        B::AnyArrayBuffer,
        B::BoxedPrimitive,
    ];
    let types = context.object_rooted()?;
    for brand in NATIVE_BRANDS {
        let (index, (name, _)) = TYPE_PREDICATES
            .iter()
            .enumerate()
            .find(|(_, (_, candidate))| candidate == brand)
            .expect("native type predicate brand");
        let data = context.number(index as f64);
        let predicate = context.host_function_named_with_data(
            crate::host::shared_vm::operation("utilTypeCheck"),
            "",
            data,
        )?;
        let key = context.string_rooted(name);
        context.define_data_property_rooted(types, key, predicate, true, true, true)?;
    }
    Ok(types)
}

/// Node's optional full-details argument tests literal true, not truthiness.
pub(crate) fn proxy_details(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let value = args.first().copied().unwrap_or(absent);
    let Some((target, handler)) = context.proxy_details_rooted(value)? else { return Ok(absent); };
    let full = args.len() == 1 || args.get(1).and_then(|value| context.rooted_value(*value)).is_some_and(|value| value.as_bool() == Some(true));
    if full { context.array_rooted(&[target, handler]) } else { Ok(target) }
}

/// One argument returns entries alone; any second argument requests both fields.
pub(crate) fn preview_entries(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let absent = context.undefined();
    let value = args.first().copied().unwrap_or(absent);
    let Some(preview) = context.collection_preview_rooted(value)? else { return Ok(absent); };
    let entries = context.array_rooted(&preview.entries)?;
    if args.len() == 1 { Ok(entries) } else {
        let paired = context.boolean(preview.is_key_value);
        context.array_rooted(&[entries, paired])
    }
}

/// Core owns creation identity, safe prototype lookup and intrinsic class display.
pub(crate) fn constructor_name(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    Ok(context.constructor_name_rooted(value)?
        .unwrap_or_else(|| context.string_rooted("Object")))
}

/// External wrappers expose the stored address, without touching native memory.
pub(crate) fn external_value(context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = args.first().copied().unwrap_or_else(|| context.undefined());
    match context.external_address_rooted(value) {
        Some(address) => Ok(context.bigint_rooted(address as i128)),
        None => { let error = context.type_error_rooted("Value is not an external")?; Err(context.throw(error)) }
    }
}

/// The configured util.js validates the public API and owns source-map policy.
pub(crate) fn get_call_sites(
    context: &mut NativeContext<'_, NodeHost>, _: RootId, args: &[RootId],
) -> Result<RootId, RootedError> {
    let count = args.first().and_then(|root| context.rooted_value(*root)).and_then(quench_runtime::Value::as_number);
    // Match the native binding's validated frame-count domain without guest coercion.
    let Some(count) = count.filter(|n| n.is_finite() && n.fract() == 0.0 && (1.0..=NODE_CALL_SITE_FRAME_LIMIT as f64).contains(n)) else {
        let error = context.range_error_rooted("Invalid stack frame count")?;
        return Err(context.throw(error));
    };
    // Node's native operation drops the util.js caller frame, even when borrowed directly.
    let frames = context.source_stack_frames(count as usize + 1);
    let mut sites = Vec::with_capacity(frames.len().saturating_sub(1));
    for frame in frames.into_iter().skip(1) {
        let site = context.null_object_rooted()?;
        let function = context.string_units_rooted(&frame.function_name);
        let id = context.string_rooted(&frame.script_id.to_string());
        let script = context.string_rooted(&frame.script_name);
        let line = context.number(f64::from(frame.line_number));
        let column = context.number(f64::from(frame.column_number));
        for (name, value) in [("functionName", function), ("scriptId", id), ("scriptName", script), ("lineNumber", line), ("columnNumber", column), ("column", column)] {
            let key = context.string_rooted(name);
            context.define_data_property_rooted(site, key, value, true, true, true)?;
        }
        sites.push(site);
    }
    context.array_rooted(&sites)
}
