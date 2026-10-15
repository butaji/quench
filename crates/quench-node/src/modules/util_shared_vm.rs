//! Shared-VM utilities and native builtin-brand predicates.

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
    ("isDataView", BuiltinBrand::DataView),
    ("isTypedArray", BuiltinBrand::TypedArray),
    ("isArrayBufferView", BuiltinBrand::ArrayBufferView),
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

pub(crate) const MAX_SAFE_INTEGER: f64 = ((1_u64 << f64::MANTISSA_DIGITS) - 1) as f64;

const UTIL: &str = quench_js_check::checked_js!(
    r#"((quote, shallowLabel, customHook, formatString, builtinHook, warnedCodes) => {
  const propertyName = (key) => /^[A-Za-z_$][\w$]*$/.test(key) ? key : quote(key);

  function inspect(value, options = {}) {
    const settings = { ...inspect.defaultOptions, ...(options || {}) };
    const maxDepth = settings.depth === null ? Infinity : Number(settings.depth);
    const seen = new Set();

    const render = (current, depth) => {
      if (current === undefined) return "undefined";
      if (current === null) return "null";
      if (typeof current === "string") return quote(current);
      if (typeof current === "boolean") return String(current);
      if (typeof current === "bigint") return `${current}n`;
      if (typeof current === "number") {
        if (Number.isNaN(current)) return "NaN";
        if (current === Infinity) return "Infinity";
        if (current === -Infinity) return "-Infinity";
        if (Object.is(current, -0)) return "-0";
        return String(current);
      }
      if (typeof current === "symbol") return current.toString();
      const custom = customHook(current, settings, settings.depth === null ? null : maxDepth - depth, inspect);
      if (custom !== undefined) {
        const value = custom[0];
        return typeof value === "string" ? value : render(value, depth);
      }
      if (typeof current === "function") {
        return current.name ? `[Function: ${current.name}]` : "[Function (anonymous)]";
      }
      if (current instanceof Error) {
        if (current.name === "SystemError" && current.code === "ERR_SOCKET_BUFFER_SIZE" && current.info) {
          const { code, message, errno, syscall } = current.info;
          return `SystemError [ERR_SOCKET_BUFFER_SIZE]: ${current.message}\n` +
            `  code: 'ERR_SOCKET_BUFFER_SIZE',\n` +
            `  info: {\n` +
            `    errno: ${errno},\n` +
            `    code: '${code}',\n` +
            `    message: '${message}',\n` +
            `    syscall: '${syscall}'\n` +
            `  },\n` +
            `  errno: [Getter/Setter: ${current.errno}],\n` +
            `  syscall: [Getter/Setter: '${current.syscall}']\n` +
            `}`;
        }
        if (typeof current.stack === "string") return current.stack;
        return `${current.name || "Error"}${current.message ? `: ${current.message}` : ""}`;
      }
      if (depth > maxDepth) {
        return shallowLabel(current, settings);
      }
      if (seen.has(current)) return "[Circular]";
      seen.add(current);
      let result;
      const builtin = builtinHook(current, settings, depth, render, propertyName);
      if (builtin !== undefined) {
        result = builtin;
      } else if (Array.isArray(current)) {
        const items = [];
        for (let index = 0; index < current.length; index++) {
          items.push(index in current ? render(current[index], depth + 1) : "<empty>");
        }
        result = items.length ? `[ ${items.join(", ")} ]` : "[]";
      } else if (current instanceof RegExp) {
        result = String(current);
      } else if (current instanceof Map) {
        const entries = [];
        for (const [key, entry] of current) {
          entries.push(`${render(key, depth + 1)} => ${render(entry, depth + 1)}`);
        }
        result = entries.length
          ? `Map(${entries.length}) { ${entries.join(", ")} }`
          : "Map(0) {}";
      } else if (current instanceof Set) {
        const entries = [...current].map((entry) => render(entry, depth + 1));
        result = entries.length
          ? `Set(${entries.length}) { ${entries.join(", ")} }`
          : "Set(0) {}";
      } else {
        let keys = settings.showHidden
          ? Object.getOwnPropertyNames(current)
          : Object.keys(current);
        if (settings.sorted) keys = keys.sort();
        const entries = keys.map((key) => {
          const descriptor = Object.getOwnPropertyDescriptor(current, key);
          const value = descriptor && "value" in descriptor
            ? render(descriptor.value, depth + 1)
            : descriptor && descriptor.get && descriptor.set
            ? "[Getter/Setter]"
            : descriptor && descriptor.get
            ? "[Getter]"
            : descriptor && descriptor.set
            ? "[Setter]"
            : "undefined";
          return `${propertyName(key)}: ${value}`;
        });
        result = entries.length ? `{ ${entries.join(", ")} }` : "{}";
      }
      seen.delete(current);
      return result;
    };

    return render(value, 0);
  }

  function format(first, ...args) {
    if (typeof first !== "string") {
      return [first, ...args].map((value) => typeof value === "string" ? value : inspect(value)).join(" ");
    }
    let index = 0;
    const output = first.replace(/%[sdifjoOc%]/g, (token) => {
      if (token === "%%") return "%";
      if (index >= args.length) return token;
      const value = args[index++];
      switch (token) {
        case "%s": return formatString(value, inspect);
        case "%d": return String(Number(value));
        case "%i": return String(Number.parseInt(value, 10));
        case "%f": return String(Number.parseFloat(value));
        case "%j": try { return JSON.stringify(value); } catch { return "[Circular]"; }
        case "%o":
        case "%O": return typeof value === "string" ? value : inspect(value);
        case "%c": return "";
        default: return token;
      }
    });
    return output + args.slice(index).map((value) => typeof value === "string" ? value : inspect(value)).map((value) => ` ${value}`).join("");
  }

  inspect.defaultOptions = {
    colors: false,
    depth: 2,
    showHidden: false,
    customInspect: true,
    sorted: false,
  };
  inspect.custom = Symbol.for("nodejs.util.inspect.custom");
  const getCallSites = (frameCount = 10, options) => {
    if (frameCount !== null && typeof frameCount === "object") {
      options = frameCount;
      frameCount = 10;
    }
    if (options !== undefined && (options === null || typeof options !== "object")) {
      throw new TypeError("The options argument must be an object");
    }
    if (!Number.isInteger(frameCount) || frameCount < 1 || frameCount > 200) {
      throw new RangeError("The frame count must be an integer between 1 and 200");
    }
    const scriptName = globalThis.process?.argv?.[1] || "";
    return Array.from({ length: frameCount }, () => ({
      scriptName,
      scriptId: scriptName,
      lineNumber: 0,
      columnNumber: 0,
    }));
  };
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

pub(crate) fn module(context: &mut NativeContext<'_, NodeHost>) -> Result<RootId, RootedError> {
    let cached = context.host_mut().shared_state().borrow().util_intrinsics;
    if let Some(api) = cached {
        return Ok(api.module);
    }
    let factory = context.evaluate_script_rooted(UTIL, "node:util/shared.js")?;
    let quote = context.host_function(crate::host::shared_vm::operation("utilInspectQuote"))?;
    let undefined = context.undefined();
    let instance = context.evaluate_script_rooted(
        "(object, constructor) => object instanceof constructor",
        "node:util/instance.js",
    )?;
    let shallow = context.host_function_with_data(
        crate::host::shared_vm::operation("utilInspectShallowLabel"),
        instance,
    )?;
    let custom_key = context.evaluate_script_rooted(
        "Symbol.for('nodejs.util.inspect.custom')",
        "node:util/custom-symbol.js",
    )?;
    let custom = context.host_function_with_data(
        crate::host::shared_vm::operation("utilInspectCustom"),
        custom_key,
    )?;
    let format_string = super::util_format_string_shared_vm::function(context)?;
    let builtin = super::util_inspect_builtin_shared_vm::function(context)?;
    let global = context.global_root()?;
    let set_key = context.string_rooted("Set");
    let set = context.get_property_rooted(global, set_key)?;
    let warned_codes = context.construct_rooted(set, set, &[])?;
    let module = context.call_rooted(factory, undefined, &[quote, shallow, custom, format_string, builtin, warned_codes])?;
    let inspect_key = context.string_rooted("inspect");
    let inspect = context.get_property_rooted(module, inspect_key)?;
    let defaults_key=context.string_rooted("defaultOptions");let defaults=context.get_property_rooted(inspect,defaults_key)?;
    for(name,value)in[("maxArrayLength",super::util_inspect_builtin_shared_vm::DEFAULT_MAX_ARRAY_LENGTH),("breakLength",super::util_inspect_builtin_shared_vm::DEFAULT_BREAK_LENGTH)]{let key=context.string_rooted(name);let value=context.number(value);context.define_data_property_rooted(defaults,key,value,true,true,true)?;}

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
    let callbackify_key = context.string_rooted("callbackify");
    context.set_property_rooted(module, callbackify_key, callbackify, module)?;
    let types = context.object_rooted()?;
    for (index, &(name, brand)) in TYPE_PREDICATES.iter().enumerate() {
        let data = context.number(index as f64);
        let predicate = context
            .host_function_with_data(crate::host::shared_vm::operation("utilTypeCheck"), data)?;
        let name_key = context.string_rooted("name");
        let (function_name, length) = match brand {
            BuiltinBrand::ArrayBufferView => ("isView", 1),
            BuiltinBrand::Date
            | BuiltinBrand::RegExp
            | BuiltinBrand::NativeError
            | BuiltinBrand::Promise
            | BuiltinBrand::Map
            | BuiltinBrand::Set
            | BuiltinBrand::WeakMap
            | BuiltinBrand::WeakSet
            | BuiltinBrand::Proxy
            | BuiltinBrand::ArrayBuffer
            | BuiltinBrand::SharedArrayBuffer
            | BuiltinBrand::AnyArrayBuffer
            | BuiltinBrand::ArgumentsObject
            | BuiltinBrand::AsyncFunction
            | BuiltinBrand::GeneratorFunction
            | BuiltinBrand::GeneratorObject
            | BuiltinBrand::MapIterator
            | BuiltinBrand::SetIterator
            | BuiltinBrand::ModuleNamespaceObject
            | BuiltinBrand::BooleanObject
            | BuiltinBrand::NumberObject
            | BuiltinBrand::StringObject
            | BuiltinBrand::SymbolObject
            | BuiltinBrand::BigIntObject
            | BuiltinBrand::BoxedPrimitive => ("", 0),
            _ => (name, 1),
        };
        let name_value = context.string_rooted(function_name);
        context.define_data_property_rooted(predicate, name_key, name_value, false, false, true)?;
        let length_key = context.string_rooted("length");
        let length_value = context.number(f64::from(length));
        context.define_data_property_rooted(
            predicate,
            length_key,
            length_value,
            false,
            false,
            true,
        )?;
        let key = context.string_rooted(name);
        context.set_property_rooted(types, key, predicate, types)?;
    }
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
    // Node treats the validator's "Object" token as the object primitive type.
    let expected = if expected == "Object" {
        "of type object".to_owned()
    } else if expected.chars().next().is_some_and(char::is_uppercase) {
        format!("an instance of {expected}")
    } else {
        format!("of type {expected}")
    };
    argument_type_error_with_expected(context, root, name, &expected)
}

/// Already formatted union of accepted Node argument types.
pub(crate) fn argument_type_error_with_expected(
    context: &mut NativeContext<'_, NodeHost>,
    root: RootId,
    name: &str,
    expected: &str,
) -> Result<RootedError, RootedError> {
    let received = argument_type_description(context, root)?;
    let label = if name.ends_with(" argument") {
        name.to_owned()
    } else if name.contains('.') {
        format!("\"{name}\" property")
    } else {
        format!("\"{name}\" argument")
    };
    let error = context.type_error_rooted(&format!(
        "The {label} must be {expected}. Received {received}"
    ))?;
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
    let kind = if name.contains('.') {
        "property"
    } else {
        "argument"
    };
    let mut units = format!("The {kind} '{name}' {reason}. Received ")
        .encode_utf16()
        .collect::<Vec<_>>();
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

/// Inspect's depth boundary derives the constructor from prototype descriptors,
/// without invoking a user constructor getter.
pub(crate) fn inspect_shallow_label(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let object = args.first().copied().unwrap_or_else(|| context.undefined());
    // Empty ordinary null-prototype records keep their braces even beyond
    // the depth limit. Internal-slot brands retain their specialized labels.
    let mut ordinary = true;
    for (_, brand) in TYPE_PREDICATES {
        if context.has_builtin_brand_rooted(object, *brand)? {
            ordinary = false;
            break;
        }
    }
    if ordinary && !context.is_array_rooted(object)? && !context.is_callable_rooted(object)? {
        let prototype = context.prototype_rooted(object)?;
        if context.rooted_value(prototype).is_some_and(|value| value.is_null()) {
            let settings = args.get(1).copied().unwrap_or_else(|| context.undefined());
            let hidden = if context.is_object_rooted(settings)? {
                let key = context.string_rooted("showHidden");
                let hidden = context.get_property_rooted(settings, key)?;
                context.truthy_rooted(hidden)?
            } else { false };
            let keys = if hidden { context.own_keys_rooted(object)? }
                else { context.enumerable_own_keys_rooted(object, true, true)? };
            if keys.is_empty() {
                return Ok(context.string_rooted("[Object: null prototype] {}"));
            }
        }
    }
    let mut current = object;
    let constructor_key = context.string_rooted("constructor");
    let value_key = context.string_rooted("value");
    let name_key = context.string_rooted("name");
    let instance = context.host_function_data()?;
    let undefined = context.undefined();
    while context.is_object_rooted(current)? {
        let descriptor = context.own_property_descriptor_rooted(current, constructor_key)?;
        if context.is_object_rooted(descriptor)? {
            let constructor = context.get_property_rooted(descriptor, value_key)?;
            if context.is_callable_rooted(constructor)? {
                let name = context.get_property_rooted(constructor, name_key)?;
                let name_text = context.to_string(name)?;
                if !name_text.is_empty() {
                    if let Ok(result) =
                        context.call_rooted(instance, undefined, &[object, constructor])
                    {
                        if context.truthy_rooted(result)? {
                            return Ok(context.string_rooted(&format!("[{name_text}]")));
                        }
                    }
                }
            }
        }
        current = context.prototype_rooted(current)?;
    }
    Ok(context.string_rooted("[Object: null prototype]"))
}

/// Custom inspect dispatch precedes ordinary formatting and the depth boundary.
pub(crate) fn inspect_custom(
    context: &mut NativeContext<'_, NodeHost>,
    _: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let undefined = context.undefined();
    let object = args.first().copied().unwrap_or(undefined);
    let settings = args.get(1).copied().unwrap_or(undefined);
    let depth = args.get(2).copied().unwrap_or(undefined);
    let inspect = args.get(3).copied().unwrap_or(undefined);
    let enabled_key = context.string_rooted("customInspect");
    let enabled = context.get_property_rooted(settings, enabled_key)?;
    if !context.truthy_rooted(enabled)? {
        return Ok(undefined);
    }
    let key = context.host_function_data()?;
    let method = context.get_property_rooted(object, key)?;
    if !context.is_callable_rooted(method)? || context.strict_equal_rooted(method, inspect)? {
        return Ok(undefined);
    }
    let constructor_key = context.string_rooted("constructor");
    let descriptor = context.own_property_descriptor_rooted(object, constructor_key)?;
    if context.is_object_rooted(descriptor)? {
        let value_key = context.string_rooted("value");
        let constructor = context.get_property_rooted(descriptor, value_key)?;
        if context
            .rooted_value(constructor)
            .is_some_and(|value| !value.is_null() && !value.is_undefined())
        {
            let prototype_key = context.string_rooted("prototype");
            let prototype = context.get_property_rooted(constructor, prototype_key)?;
            if context.strict_equal_rooted(prototype, object)? {
                return Ok(undefined);
            }
        }
    }
    let result = context.call_rooted(method, object, &[depth, settings, inspect])?;
    if context.strict_equal_rooted(result, object)? {
        Ok(undefined)
    } else {
        context.array_rooted(&[result])
    }
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

pub(crate) fn typed_array_name(context: &mut NativeContext<'_, NodeHost>, value: RootId) -> Result<&'static str, RootedError> {
    for &(name,brand) in TYPE_PREDICATES {
        if brand != BuiltinBrand::TypedArray && name.ends_with("Array") && context.has_builtin_brand_rooted(value,brand)? { return Ok(name.strip_prefix("is").expect("type predicate prefix")); }
    }
    Err(RootedError::host("typed array brand has no display name"))
}
