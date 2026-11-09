//! `node:util` in Rust (lib/util.js): the module's exports over the native inspector
//! (`util_inspect_shared_vm`), predicates, comparisons and policies.
use crate::host::shared_vm::operation;
use crate::host::NodeHost;
use crate::modules::util_inspect_shared_vm::{self as inspect, DefaultOption};
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError, ValueType};

pub(super) type Context<'a> = NativeContext<'a, NodeHost>;

/// The registry key of `util.promisify.custom`.
pub(crate) const PROMISIFY_CUSTOM: &str = "nodejs.util.promisify.custom";

/// A legacy `util.is*` predicate: its export name, the deprecation it emits and its test.
struct Legacy {
    name: &'static str,
    code: &'static str,
    message: &'static str,
    test: LegacyTest,
}

#[derive(Clone, Copy)]
enum LegacyTest {
    Array,
    Boolean,
    Buffer,
    Null,
    NullOrUndefined,
    Number,
    String,
    Symbol,
    Undefined,
    RegExp,
    Object,
    Date,
    Error,
    Function,
    Primitive,
}

/// The module's predicate exports, in their order (lib/util.js): the deprecated legacy
/// predicates and `isDeepStrictEqual`.
enum Predicate {
    Legacy(Legacy),
    DeepStrictEqual,
}

const PREDICATES: &[Predicate] = &[
    Predicate::Legacy(Legacy { name: "isArray", code: "DEP0044", message: "The `util.isArray` API is deprecated. Please use `Array.isArray()` instead.", test: LegacyTest::Array }),
    Predicate::Legacy(Legacy { name: "isBoolean", code: "DEP0045", message: "The `util.isBoolean` API is deprecated.  Please use `typeof arg === \"boolean\"` instead.", test: LegacyTest::Boolean }),
    Predicate::Legacy(Legacy { name: "isBuffer", code: "DEP0046", message: "The `util.isBuffer` API is deprecated. Please use `Buffer.isBuffer()` instead.", test: LegacyTest::Buffer }),
    Predicate::DeepStrictEqual,
    Predicate::Legacy(Legacy { name: "isNull", code: "DEP0050", message: "The `util.isNull` API is deprecated. Please use `arg === null` instead.", test: LegacyTest::Null }),
    Predicate::Legacy(Legacy { name: "isNullOrUndefined", code: "DEP0051", message: "The `util.isNullOrUndefined` API is deprecated. Please use `arg === null || arg === undefined` instead.", test: LegacyTest::NullOrUndefined }),
    Predicate::Legacy(Legacy { name: "isNumber", code: "DEP0052", message: "The `util.isNumber` API is deprecated. Please use `typeof arg === \"number\"` instead.", test: LegacyTest::Number }),
    Predicate::Legacy(Legacy { name: "isString", code: "DEP0056", message: "The `util.isString` API is deprecated.  Please use `typeof arg === \"string\"` instead.", test: LegacyTest::String }),
    Predicate::Legacy(Legacy { name: "isSymbol", code: "DEP0057", message: "The `util.isSymbol` API is deprecated.  Please use `arg === \"symbol\"` instead.", test: LegacyTest::Symbol }),
    Predicate::Legacy(Legacy { name: "isUndefined", code: "DEP0058", message: "The `util.isUndefined` API is deprecated. Please use `arg === undefined` instead.", test: LegacyTest::Undefined }),
    Predicate::Legacy(Legacy { name: "isRegExp", code: "DEP0055", message: "The `util.isRegExp` API is deprecated. Please use `arg instanceof RegExp` instead.", test: LegacyTest::RegExp }),
    Predicate::Legacy(Legacy { name: "isObject", code: "DEP0053", message: "The `util.isObject` API is deprecated. Please use `arg !== null && typeof arg === \"object\"` instead.", test: LegacyTest::Object }),
    Predicate::Legacy(Legacy { name: "isDate", code: "DEP0047", message: "The `util.isDate` API is deprecated.  Please use `arg instanceof Date` instead.", test: LegacyTest::Date }),
    Predicate::Legacy(Legacy { name: "isError", code: "DEP0048", message: "The `util.isError` API is deprecated. Please use `ObjectPrototypeToString(e) === \"[object Error]\" || e instanceof Error` instead.", test: LegacyTest::Error }),
    Predicate::Legacy(Legacy { name: "isFunction", code: "DEP0049", message: "The `util.isFunction` API is deprecated.  Please use `typeof arg === \"function\"` instead.", test: LegacyTest::Function }),
    Predicate::Legacy(Legacy { name: "isPrimitive", code: "DEP0054", message: "The `util.isPrimitive` API is deprecated. Please use `arg === null || (typeof arg !== \"object\" && typeof arg !== \"function\")` instead.", test: LegacyTest::Primitive }),
];

/// `util._extend`'s and `util.log`'s deprecations.
const EXTEND_DEPRECATION: (&str, &str) = ("DEP0060", "The `util._extend` API is deprecated. Please use Object.assign() instead.");
const LOG_DEPRECATION: (&str, &str) = ("DEP0059", "The `util.log API is deprecated. Please use console.log() with a custom formatter or a third-party logger instead.");
const GET_CALL_SITE_WARNING: &str = "The `util.getCallSite` API has been renamed to `util.getCallSites()`.";

/// `util.types`, in Node's key order.
pub(crate) const TYPES: &[(&str, BuiltinBrand)] = &[
    ("isArgumentsObject", BuiltinBrand::ArgumentsObject),
    ("isArrayBuffer", BuiltinBrand::ArrayBuffer),
    ("isAsyncFunction", BuiltinBrand::AsyncFunction),
    ("isBigIntObject", BuiltinBrand::BigIntObject),
    ("isBooleanObject", BuiltinBrand::BooleanObject),
    ("isDataView", BuiltinBrand::DataView),
    ("isDate", BuiltinBrand::Date),
    ("isExternal", BuiltinBrand::External),
    ("isGeneratorFunction", BuiltinBrand::GeneratorFunction),
    ("isGeneratorObject", BuiltinBrand::GeneratorObject),
    ("isMap", BuiltinBrand::Map),
    ("isMapIterator", BuiltinBrand::MapIterator),
    ("isModuleNamespaceObject", BuiltinBrand::ModuleNamespaceObject),
    ("isNativeError", BuiltinBrand::NativeError),
    ("isNumberObject", BuiltinBrand::NumberObject),
    ("isPromise", BuiltinBrand::Promise),
    ("isProxy", BuiltinBrand::Proxy),
    ("isRegExp", BuiltinBrand::RegExp),
    ("isSet", BuiltinBrand::Set),
    ("isSetIterator", BuiltinBrand::SetIterator),
    ("isSharedArrayBuffer", BuiltinBrand::SharedArrayBuffer),
    ("isStringObject", BuiltinBrand::StringObject),
    ("isSymbolObject", BuiltinBrand::SymbolObject),
    ("isWeakMap", BuiltinBrand::WeakMap),
    ("isWeakSet", BuiltinBrand::WeakSet),
    ("isAnyArrayBuffer", BuiltinBrand::AnyArrayBuffer),
    ("isBoxedPrimitive", BuiltinBrand::BoxedPrimitive),
    ("isArrayBufferView", BuiltinBrand::ArrayBufferView),
    ("isTypedArray", BuiltinBrand::TypedArray),
    ("isUint8Array", BuiltinBrand::Uint8Array),
    ("isUint8ClampedArray", BuiltinBrand::Uint8ClampedArray),
    ("isUint16Array", BuiltinBrand::Uint16Array),
    ("isUint32Array", BuiltinBrand::Uint32Array),
    ("isInt8Array", BuiltinBrand::Int8Array),
    ("isInt16Array", BuiltinBrand::Int16Array),
    ("isInt32Array", BuiltinBrand::Int32Array),
    ("isFloat16Array", BuiltinBrand::Float16Array),
    ("isFloat32Array", BuiltinBrand::Float32Array),
    ("isFloat64Array", BuiltinBrand::Float64Array),
    ("isBigInt64Array", BuiltinBrand::BigInt64Array),
    ("isBigUint64Array", BuiltinBrand::BigUint64Array),
];

/// `util.types` predicates that test the crypto key classes by identity.
const KEY_TYPES: &[(&str, &str)] = &[("isKeyObject", "KeyObject"), ("isCryptoKey", "CryptoKey")];

pub(super) fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

pub(super) fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

pub(super) fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

pub(super) fn type_of(c: &Context<'_>, value: RootId) -> ValueType {
    c.value_type_rooted(value).unwrap_or(ValueType::Undefined)
}

fn function(c: &mut Context<'_>, operation_name: &str, name: &str) -> Result<RootId, RootedError> {
    c.host_function_named(operation(operation_name), name)
}

fn function_with(c: &mut Context<'_>, operation_name: &str, name: &str, data: RootId) -> Result<RootId, RootedError> {
    c.host_function_named_with_data(operation(operation_name), name, data)
}

pub(super) fn process(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    c.host_mut()
        .shared_state()
        .borrow()
        .process_module
        .ok_or_else(|| RootedError::host("process is not initialized"))
}

fn validate_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error(c, value, name, "function")?)
}

fn validate_string(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if type_of(c, value) == ValueType::String {
        return Ok(());
    }
    Err(super::util_shared_vm::argument_type_error(c, value, name, "string")?)
}

/// The original values the formatter reads and the inspect tables, before the module.
pub(crate) fn build_primordials(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let primordials = c.null_object_rooted()?;
    c.with_startup_intrinsics(|c| {
        let global = c.global_root()?;
        for &(name, path) in inspect::PRIMORDIALS {
            let mut value = global;
            for segment in path {
                value = get(c, value, segment)?;
            }
            put(c, primordials, name, value)?;
        }
        Ok(())
    })?;
    let symbol_for = get(c, primordials, "symbolFor")?;
    let undefined = c.undefined();
    for (name, key) in [("custom", inspect::INSPECT_CUSTOM), ("promisifyCustom", PROMISIFY_CUSTOM)] {
        let key = c.string_rooted(key);
        let symbol = c.call_rooted(symbol_for, undefined, &[key])?;
        put(c, primordials, name, symbol)?;
    }
    let arguments = c.symbol_rooted(Some("customPromisifyArgs"));
    put(c, primordials, "promisifyArgs", arguments)?;
    let defaults = c.object_rooted()?;
    for (name, value) in inspect::DEFAULT_OPTIONS {
        let value = match value {
            DefaultOption::Boolean(value) => c.boolean(value),
            DefaultOption::Number(value) => c.number(value),
        };
        let key = c.string_rooted(name);
        c.define_data_property_rooted(defaults, key, value, true, true, false)?;
    }
    c.prevent_extensions_rooted(defaults)?;
    put(c, primordials, "defaultOptions", defaults)?;
    let colors = c.null_object_rooted()?;
    for &(name, open, close) in inspect::COLORS {
        let open = c.number(f64::from(open));
        let close = c.number(f64::from(close));
        let pair = c.array_rooted(&[open, close])?;
        put(c, colors, name, pair)?;
    }
    for &(alias, target) in inspect::COLOR_ALIASES {
        let target = c.string_rooted(target);
        let getter = function_with(c, "utilColorAliasGet", "get", target)?;
        let setter = function_with(c, "utilColorAliasSet", "set", target)?;
        let descriptor = c.null_object_rooted()?;
        put(c, descriptor, "get", getter)?;
        put(c, descriptor, "set", setter)?;
        let yes = c.boolean(true);
        let no = c.boolean(false);
        put(c, descriptor, "configurable", yes)?;
        put(c, descriptor, "enumerable", no)?;
        let key = c.string_rooted(alias);
        c.define_property_descriptor_rooted(colors, key, descriptor)?;
    }
    put(c, primordials, "colors", colors)?;
    let styles = c.null_object_rooted()?;
    for &(name, color) in inspect::STYLES {
        let color = c.string_rooted(color);
        put(c, styles, name, color)?;
    }
    put(c, primordials, "styles", styles)?;
    let warned = c.null_object_rooted()?;
    put(c, primordials, "codesWarned", warned)?;
    let debug = c.null_object_rooted()?;
    put(c, primordials, "debugImpls", debug)?;
    let inspect_function = function(c, "utilInspect", "inspect")?;
    put(c, primordials, "inspect", inspect_function)?;
    Ok(primordials)
}

/// The exports of `node:util`, in Node's order, and the formatter identities it shares.
pub(crate) fn build(c: &mut Context<'_>, primordials: RootId) -> Result<(RootId, RootId), RootedError> {
    let module = c.object_rooted()?;
    let inspect_function = get(c, primordials, "inspect")?;
    decorate_inspect(c, inspect_function, primordials)?;
    export_functions(c, module, &[("_errnoException", "utilErrnoException"), ("_exceptionWithHostPort", "utilExceptionWithHostPort")])?;
    let extend = function(c, "utilExtend", "_extend")?;
    let extend = deprecated_export(c, extend, EXTEND_DEPRECATION)?;
    put(c, module, "_extend", extend)?;
    let callbackify = super::util_callbackify_shared_vm::function(c)?;
    put(c, module, "callbackify", callbackify)?;
    let debuglog = function(c, "utilDebuglog", "debuglog")?;
    put(c, module, "debug", debuglog)?;
    put(c, module, "debuglog", debuglog)?;
    export_functions(c, module, &[("deprecate", "utilDeprecate"), ("format", "utilFormat")])?;
    let style = super::util_style_shared_vm::function(c, inspect_function)?;
    put(c, module, "styleText", style)?;
    export_functions(c, module, &[("formatWithOptions", "utilFormatWithOptions")])?;
    let get_call_site = function(c, "utilGetCallSite", "deprecated")?;
    put(c, module, "getCallSite", get_call_site)?;
    export_functions(c, module, &[
        ("getCallSites", "utilGetCallSitesPublic"),
        ("getSystemErrorMap", "utilGetSystemErrorMap"),
        ("getSystemErrorName", "utilGetSystemErrorName"),
        ("getSystemErrorMessage", "utilGetSystemErrorMessage"),
        ("inherits", "utilInherits"),
    ])?;
    put(c, module, "inspect", inspect_function)?;
    export_predicates(c, module)?;
    let log = function(c, "utilLog", "log")?;
    let log = deprecated_export(c, log, LOG_DEPRECATION)?;
    put(c, module, "log", log)?;
    let promisify = function(c, "utilPromisify", "promisify")?;
    let custom = get(c, primordials, "promisifyCustom")?;
    put(c, promisify, "custom", custom)?;
    put(c, module, "promisify", promisify)?;
    export_functions(c, module, &[
        ("stripVTControlCharacters", "utilStripVTControlCharacters"),
        ("toUSVString", "utilToUSVString"),
        ("transferableAbortSignal", "utilTransferableAbortSignal"),
        ("transferableAbortController", "utilTransferableAbortController"),
        ("aborted", "utilAborted"),
    ])?;
    export_late(c, module)?;
    let formatters = c.null_object_rooted()?;
    for name in ["format", "formatWithOptions"] {
        let value = get(c, module, name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(formatters, key, value, false, true, false)?;
    }
    super::console_shared_vm::note_inspect_loaded(c)?;
    Ok((module, formatters))
}

/// Exports each operation's function under its export name, which is also its own name.
fn export_functions(c: &mut Context<'_>, module: RootId, exports: &[(&str, &str)]) -> Result<(), RootedError> {
    for &(name, operation_name) in exports {
        let value = function(c, operation_name, name)?;
        put(c, module, name, value)?;
    }
    Ok(())
}

/// The legacy `util.is*` predicates behind their deprecations, and `isDeepStrictEqual`.
fn export_predicates(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    for (index, predicate) in PREDICATES.iter().enumerate() {
        let Predicate::Legacy(legacy) = predicate else {
            export_functions(c, module, &[("isDeepStrictEqual", "utilIsDeepStrictEqual")])?;
            continue;
        };
        let data = c.number(index as f64);
        let test = function_with(c, "utilLegacyIs", legacy.name, data)?;
        let length = c.number(f64::from(legacy_length(legacy.test)));
        let key = c.string_rooted("length");
        c.define_data_property_rooted(test, key, length, false, false, true)?;
        let value = deprecated_export(c, test, (legacy.code, legacy.message))?;
        put(c, module, legacy.name, value)?;
    }
    Ok(())
}

/// The exports after `aborted`: `types`, the parsers, the lazy text codecs, MIME classes,
/// `diff` and `setTraceSigInt`.
fn export_late(c: &mut Context<'_>, module: RootId) -> Result<(), RootedError> {
    let types = types_object(c)?;
    put(c, module, "types", types)?;
    export_functions(c, module, &[("parseEnv", "parseEnv")])?;
    let parse_args = super::util_parse_args_shared_vm::function(c)?;
    put(c, module, "parseArgs", parse_args)?;
    for name in ["TextDecoder", "TextEncoder"] {
        let name_value = c.string_rooted(name);
        let data = c.record_rooted(false, &["module", "name"], &[module, name_value])?;
        let initializer = function_with(c, "utilGlobalExport", "", data)?;
        c.define_lazy_data_properties_rooted(module, &[name], true, initializer)?;
    }
    let (mime, params) = super::util_mime_shared_vm::classes(c)?;
    put(c, module, "MIMEType", mime)?;
    put(c, module, "MIMEParams", params)?;
    export_functions(c, module, &[("diff", "utilDiff"), ("setTraceSigInt", "utilSetTraceSigInt")])
}

/// `inspect.custom`, `inspect.defaultOptions`, `inspect.colors` and `inspect.styles`.
fn decorate_inspect(c: &mut Context<'_>, inspect_function: RootId, primordials: RootId) -> Result<(), RootedError> {
    let custom = get(c, primordials, "custom")?;
    put(c, inspect_function, "custom", custom)?;
    let getter = function(c, "utilInspectDefaultsGet", "get")?;
    let setter = function(c, "utilInspectDefaultsSet", "set")?;
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "get", getter)?;
    put(c, descriptor, "set", setter)?;
    let no = c.boolean(false);
    put(c, descriptor, "enumerable", no)?;
    put(c, descriptor, "configurable", no)?;
    let key = c.string_rooted("defaultOptions");
    c.define_property_descriptor_rooted(inspect_function, key, descriptor)?;
    for name in ["colors", "styles"] {
        let table = get(c, primordials, name)?;
        put(c, inspect_function, name, table)?;
    }
    Ok(())
}

/// `util.types`.
pub(crate) fn types_object(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let types = c.object_rooted()?;
    for (index, &(name, _)) in TYPES.iter().enumerate() {
        let data = c.number(index as f64);
        let predicate = function_with(c, "utilTypeCheck", name, data)?;
        put(c, types, name, predicate)?;
    }
    for &(name, class) in KEY_TYPES {
        let data = c.string_rooted(class);
        let predicate = function_with(c, "utilKeyTypeCheck", name, data)?;
        put(c, types, name, predicate)?;
    }
    Ok(types)
}

fn legacy_length(test: LegacyTest) -> u8 {
    match test {
        LegacyTest::RegExp | LegacyTest::Date => 0,
        _ => 1,
    }
}

/// `deprecate(fn, message, code)` for a deprecated export of Node's own.
pub(crate) fn deprecated_export(c: &mut Context<'_>, target: RootId, (code, message): (&str, &str)) -> Result<RootId, RootedError> {
    let message = c.string_rooted(message);
    let code = c.string_rooted(code);
    deprecated_wrapper(c, target, message, code)
}

pub(crate) fn types_check(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    let value = argument(c, args, 0);
    let matches = c.has_builtin_brand_rooted(value, TYPES[index].1)?;
    Ok(c.boolean(matches))
}

pub(crate) fn key_type_check(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let class = c.to_string(data)?;
    let value = argument(c, args, 0);
    let matches = is_key_class(c, value, &class)?;
    Ok(c.boolean(matches))
}

/// Whether `value` is an instance of the crypto key class `class` (`KeyObject` or `CryptoKey`).
pub(crate) fn is_key_class(c: &mut Context<'_>, value: RootId, class: &str) -> Result<bool, RootedError> {
    if !c.is_object_rooted(value)? {
        return Ok(false);
    }
    let global = c.global_root()?;
    let constructor = if class == "KeyObject" {
        let crypto = crate::host::shared_vm::commonjs::builtin_require(c, "crypto")?;
        get(c, crypto, class)?
    } else {
        get(c, global, class)?
    };
    if !c.is_callable_rooted(constructor)? {
        return Ok(false);
    }
    c.instance_of_rooted(value, constructor)
}

pub(crate) fn legacy_is(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let index = c.to_number(data)? as usize;
    let value = argument(c, args, 0);
    let kind = type_of(c, value);
    let Some(Predicate::Legacy(legacy)) = PREDICATES.get(index) else {
        return Err(RootedError::host("legacy predicate index out of range"));
    };
    let matches = match legacy.test {
        LegacyTest::Array => c.is_array_rooted(value)?,
        LegacyTest::Boolean => kind == ValueType::Boolean,
        LegacyTest::Buffer => {
            let buffer = crate::host::shared_vm::commonjs::builtin_require(c, "buffer")?;
            let buffer = get(c, buffer, "Buffer")?;
            let is_buffer = get(c, buffer, "isBuffer")?;
            let result = c.call_rooted(is_buffer, buffer, &[value])?;
            c.truthy_rooted(result)?
        }
        LegacyTest::Null => kind == ValueType::Null,
        LegacyTest::NullOrUndefined => kind.is_nullish(),
        LegacyTest::Number => kind == ValueType::Number,
        LegacyTest::String => kind == ValueType::String,
        LegacyTest::Symbol => kind == ValueType::Symbol,
        LegacyTest::Undefined => kind == ValueType::Undefined,
        LegacyTest::RegExp => c.has_builtin_brand_rooted(value, BuiltinBrand::RegExp)?,
        LegacyTest::Object => kind == ValueType::Object,
        LegacyTest::Date => c.has_builtin_brand_rooted(value, BuiltinBrand::Date)?,
        LegacyTest::Error => c.is_object_rooted(value)? && inspect::is_error(c, value)?,
        LegacyTest::Function => kind == ValueType::Function,
        LegacyTest::Primitive => !matches!(kind, ValueType::Object | ValueType::Function),
    };
    Ok(c.boolean(matches))
}

/// `sys` (lib/sys.js): the util module, with DEP0025 when first required.
pub(crate) fn sys_module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let module = super::util_shared_vm::module(c)?;
    let process = process(c)?;
    let emit = get(c, process, "emitWarning")?;
    let message = c.string_rooted("sys is deprecated. Use util instead.");
    let kind = c.string_rooted("DeprecationWarning");
    let code = c.string_rooted("DEP0025");
    c.call_rooted(emit, process, &[message, kind, code])?;
    Ok(module)
}

/// `inspect.defaultOptions` getter.
pub(crate) fn inspect_defaults_get(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    inspect::primordial(c, "defaultOptions")
}

/// `inspect.defaultOptions = options`: `validateObject`, then `Object.assign`.
pub(crate) fn inspect_defaults_set(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let options = argument(c, args, 0);
    if type_of(c, options) != ValueType::Object {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let defaults = inspect::primordial(c, "defaultOptions")?;
    for key in c.enumerable_own_keys_rooted(options, true, true)? {
        let value = c.get_property_rooted(options, key)?;
        c.set_property_rooted(defaults, key, value, defaults)?;
    }
    Ok(defaults)
}

/// A color alias getter: `this[target]`.
pub(crate) fn color_alias_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let target = c.host_function_data()?;
    c.get_property_rooted(this, target)
}

/// A color alias setter: `this[target] = value`.
pub(crate) fn color_alias_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = c.host_function_data()?;
    let value = argument(c, args, 0);
    c.set_property_rooted(this, target, value, this)?;
    Ok(c.undefined())
}

/// `util.inherits(ctor, superCtor)`.
pub(crate) fn inherits(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let constructor = argument(c, args, 0);
    let parent = argument(c, args, 1);
    for (value, name) in [(constructor, "ctor"), (parent, "superCtor")] {
        if type_of(c, value).is_nullish() {
            return Err(super::util_shared_vm::argument_type_error(c, value, name, "function")?);
        }
    }
    let parent_prototype = get(c, parent, "prototype")?;
    if type_of(c, parent_prototype) == ValueType::Undefined {
        return Err(super::util_shared_vm::argument_type_error(c, parent_prototype, "superCtor.prototype", "object")?);
    }
    let key = c.string_rooted("super_");
    c.define_data_property_rooted(constructor, key, parent, true, false, true)?;
    let prototype = get(c, constructor, "prototype")?;
    c.set_prototype_rooted(prototype, parent_prototype)?;
    Ok(c.undefined())
}

/// `util._extend(target, source)`.
pub(crate) fn extend(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = argument(c, args, 0);
    let source = argument(c, args, 1);
    if !c.is_object_rooted(source)? || !c.truthy_rooted(source)? {
        return Ok(target);
    }
    for key in c.enumerable_own_keys_rooted(source, false, true)? {
        let value = c.get_property_rooted(source, key)?;
        c.set_property_rooted(target, key, value, target)?;
    }
    Ok(target)
}

/// `util.deprecate(fn, msg, code)`.
pub(crate) fn deprecate(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let target = argument(c, args, 0);
    let message = argument(c, args, 1);
    let code = argument(c, args, 2);
    if type_of(c, code) != ValueType::Undefined {
        validate_string(c, code, "code")?;
    }
    deprecated_wrapper(c, target, message, code)
}

pub(crate) fn deprecated_wrapper(c: &mut Context<'_>, target: RootId, message: RootId, code: RootId) -> Result<RootId, RootedError> {
    let state = c.null_object_rooted()?;
    for (name, value) in [("target", target), ("message", message), ("code", code)] {
        put(c, state, name, value)?;
    }
    let wrapper = c.host_constructor_named_with_data(crate::host::shared_vm::operation!("utilDeprecated"), "deprecated", state)?;
    put(c, state, "wrapper", wrapper)?;
    c.set_prototype_rooted(wrapper, target)?;
    let length = get(c, target, "length")?;
    if type_of(c, length) == ValueType::Number {
        let key = c.string_rooted("length");
        c.define_data_property_rooted(wrapper, key, length, false, false, true)?;
    }
    let prototype = get(c, target, "prototype")?;
    let key = c.string_rooted("prototype");
    if c.truthy_rooted(prototype)? {
        c.set_property_rooted(wrapper, key, prototype, wrapper)?;
    } else {
        c.delete_property_rooted(wrapper, key)?;
    }
    Ok(wrapper)
}

/// The deprecated wrapper: warns once, then calls or constructs the original.
pub(crate) fn deprecated(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = c.host_function_data()?;
    let process = process(c)?;
    let silenced = get(c, process, "noDeprecation")?;
    if !c.truthy_rooted(silenced)? {
        warn_deprecation(c, state)?;
    }
    let target = get(c, state, "target")?;
    match c.new_target_rooted() {
        Some(new_target) => c.construct_rooted(target, new_target, args),
        None => c.call_rooted(target, this, args),
    }
}

/// `getDeprecationWarningEmitter`: once per wrapper, and once per code across wrappers.
fn warn_deprecation(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let warned = get(c, state, "warned")?;
    if c.truthy_rooted(warned)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    put(c, state, "warned", yes)?;
    let message = get(c, state, "message")?;
    let code = get(c, state, "code")?;
    let wrapper = get(c, state, "wrapper")?;
    let kind = c.string_rooted("DeprecationWarning");
    let process = process(c)?;
    if type_of(c, code) == ValueType::Undefined {
        let emit = get(c, process, "emitWarning")?;
        c.call_rooted(emit, process, &[message, kind, wrapper])?;
        return Ok(());
    }
    emit_deprecation(c, message, code, wrapper)
}

/// One code history for util wrappers, legacy exports and crypto's implicit-length warning.
pub(crate) fn emit_deprecation(c: &mut Context<'_>, message: RootId, code: RootId, wrapper: RootId) -> Result<(), RootedError> {
    let codes = inspect::primordial(c, "codesWarned")?;
    let seen = c.get_property_rooted(codes, code)?;
    if c.truthy_rooted(seen)? {
        return Ok(());
    }
    let process = process(c)?;
    let emit = get(c, process, "emitWarning")?;
    let kind = c.string_rooted("DeprecationWarning");
    let yes = c.boolean(true);
    c.call_rooted(emit, process, &[message, kind, code, wrapper])?;
    c.set_property_rooted(codes, code, yes, codes)?;
    Ok(())
}

/// `util.getCallSite`: warns that it was renamed, then is `getCallSites`.
pub(crate) fn get_call_site(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = inspect::primordial(c, "codesWarned")?;
    let key = c.string_rooted("getCallSite");
    let warned = c.get_property_rooted(state, key)?;
    if !c.truthy_rooted(warned)? {
        let yes = c.boolean(true);
        c.set_property_rooted(state, key, yes, state)?;
        let process = process(c)?;
        let emit = get(c, process, "emitWarning")?;
        let message = c.string_rooted(GET_CALL_SITE_WARNING);
        let kind = c.string_rooted("ExperimentalWarning");
        c.call_rooted(emit, process, &[message, kind])?;
    }
    get_call_sites(c, this, args)
}

/// Frames `getCallSites` returns by default.
const DEFAULT_CALL_SITE_FRAMES: f64 = 10.0;
/// The most frames `getCallSites` returns.
const MAX_CALL_SITE_FRAMES: f64 = 200.0;

/// `util.getCallSites(frameCount, options)`.
pub(crate) fn get_call_sites(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mut frames = argument(c, args, 0);
    let mut options = argument(c, args, 1);
    if type_of(c, frames) == ValueType::Object && type_of(c, options) == ValueType::Undefined {
        options = frames;
        frames = c.undefined();
    }
    if type_of(c, options) != ValueType::Undefined && type_of(c, options) != ValueType::Object {
        return Err(super::util_shared_vm::argument_type_error(c, options, "options", "Object")?);
    }
    let count = if type_of(c, frames) == ValueType::Undefined {
        c.number(DEFAULT_CALL_SITE_FRAMES)
    } else {
        frames
    };
    let maximum = c.number(MAX_CALL_SITE_FRAMES);
    super::util_shared_vm::validate_integer(c, count, "frameCount", 1.0, maximum)?;
    let count = c.to_number(count)? as usize;
    super::util_shared_vm::call_sites(c, count, 0)
}

pub(crate) fn get_system_error_name(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let number = system_error_number(c, args)?;
    Ok(c.string_rooted(&super::util_system_error_shared_vm::system_error_name(number)))
}

pub(crate) fn get_system_error_message(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let number = system_error_number(c, args)?;
    let number = c.number(number);
    super::util_system_error_shared_vm::error_message(c, this, &[number])
}

pub(crate) fn get_system_error_map(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    super::util_system_error_shared_vm::error_map(c, this, args)
}

/// `validateNumber(err, 'err')` and `err < 0 && isSafeInteger(err)`.
fn system_error_number(c: &mut Context<'_>, args: &[RootId]) -> Result<f64, RootedError> {
    let value = argument(c, args, 0);
    let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) else {
        return Err(super::util_shared_vm::argument_type_error(c, value, "err", "number")?);
    };
    if number >= 0.0 || number.fract() != 0.0 || number < -super::util_shared_vm::MAX_SAFE_INTEGER {
        let received = super::util_shared_vm::range_error_received(c, value)?;
        let error = c.range_error_rooted(&format!(
            "The value of \"err\" is out of range. It must be a negative integer. Received {received}"
        ))?;
        super::node_errors_shared_vm::decorate_range_error(c, error, "ERR_OUT_OF_RANGE")?;
        return Err(c.throw(error));
    }
    Ok(number)
}

/// `util._errnoException(err, syscall, original)`.
pub(crate) fn errno_exception(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let errno = argument(c, args, 0);
    let syscall = argument(c, args, 1);
    let original = argument(c, args, 2);
    let code = super::util_system_error_shared_vm::system_error_name(c.to_number(errno)?);
    let syscall_text = c.to_string(syscall)?;
    let message = if c.truthy_rooted(original)? {
        format!("{syscall_text} {code} {}", c.to_string(original)?)
    } else {
        format!("{syscall_text} {code}")
    };
    let error = c.error_rooted(&message)?;
    let code = c.string_rooted(&code);
    for (name, value) in [("errno", errno), ("code", code), ("syscall", syscall)] {
        c.assign_named_property_rooted(error, name, value)?;
    }
    Ok(error)
}

/// `util._exceptionWithHostPort(err, syscall, address, port, additional)`.
pub(crate) fn exception_with_host_port(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let errno = argument(c, args, 0);
    let syscall = argument(c, args, 1);
    let address = argument(c, args, 2);
    let port = argument(c, args, 3);
    let additional = argument(c, args, 4);
    let code = super::util_system_error_shared_vm::system_error_name(c.to_number(errno)?);
    let mut details = String::new();
    let has_port = c.truthy_rooted(port)? && number_positive(c, port);
    if has_port {
        details = format!("{}:{}", c.to_string(address)?, c.to_string(port)?);
    } else if c.truthy_rooted(address)? {
        details = c.to_string(address)?;
    }
    if c.truthy_rooted(additional)? {
        details.push_str(&format!(" - Local ({})", c.to_string(additional)?));
    }
    let syscall_text = c.to_string(syscall)?;
    let error = c.error_rooted(&format!("{syscall_text} {code} {details}"))?;
    let code = c.string_rooted(&code);
    for (name, value) in [("errno", errno), ("code", code), ("syscall", syscall), ("address", address)] {
        c.assign_named_property_rooted(error, name, value)?;
    }
    if has_port {
        c.assign_named_property_rooted(error, "port", port)?;
    }
    Ok(error)
}

fn number_positive(c: &Context<'_>, value: RootId) -> bool {
    c.rooted_value(value).and_then(|value| value.as_number()).is_none_or(|number| number > 0.0)
}

/// `util.stripVTControlCharacters(str)`.
pub(crate) fn strip_vt_control_characters(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    validate_string(c, value, "str")?;
    let units = c.string_code_units(value)?.unwrap_or_default();
    Ok(c.string_units_rooted(&super::util_strip::strip_vt_control_units(&units)))
}

/// `util.toUSVString(string)`: lone surrogates become U+FFFD.
pub(crate) fn to_usv_string(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    let units = c.to_string_code_units(value)?;
    let text = String::from_utf16_lossy(&units);
    Ok(c.string_rooted(&text))
}

/// `setTraceSigInt(enable)`: the SIGINT trace hook has no native backend; the switch is
/// validated as Node validates it.
pub(crate) fn set_trace_sig_int(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let enable = argument(c, args, 0);
    if type_of(c, enable) != ValueType::Boolean {
        return Err(super::util_shared_vm::argument_type_error(c, enable, "enable", "boolean")?);
    }
    Ok(c.undefined())
}

/// Each codec export initializes independently. TextEncoder shares its original constructor
/// with the global even after a script replaces that public binding.
pub(crate) fn global_export(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let module = get(c, data, "module")?;
    let name = get(c, data, "name")?;
    let name = c.to_string(name)?;
    let value = if name == "TextEncoder" {
        super::text_encoder_shared_vm::constructor(c)?
    } else {
        let global = c.global_root()?;
        get(c, global, &name)?
    };
    put(c, module, &name, value)?;
    Ok(c.undefined())
}

/// `util.promisify(original)`.
pub(crate) fn promisify(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let original = argument(c, args, 0);
    validate_function(c, original, "original")?;
    let custom_key = inspect::primordial(c, "promisifyCustom")?;
    let custom = c.get_property_rooted(original, custom_key)?;
    if type_of(c, custom) != ValueType::Undefined {
        validate_function(c, custom, "util.promisify.custom")?;
        let descriptor = custom_descriptor(c, custom)?;
        c.define_property_descriptor_rooted(custom, custom_key, descriptor)?;
        return Ok(custom);
    }
    let names_key = inspect::primordial(c, "promisifyArgs")?;
    let names = c.get_property_rooted(original, names_key)?;
    let data = c.array_rooted(&[original, names])?;
    let promisified = c.host_function_named_with_data(crate::host::shared_vm::operation!("utilPromisified"), "fn", data)?;
    let prototype = c.prototype_rooted(original)?;
    c.set_prototype_rooted(promisified, prototype)?;
    let descriptor = custom_descriptor(c, promisified)?;
    c.define_property_descriptor_rooted(promisified, custom_key, descriptor)?;
    let null = c.null();
    for key in c.own_keys_rooted(original)? {
        let descriptor = c.own_property_descriptor_rooted(original, key)?;
        c.set_prototype_rooted(descriptor, null)?;
        c.define_property_descriptor_rooted(promisified, key, descriptor)?;
    }
    Ok(promisified)
}

fn custom_descriptor(c: &mut Context<'_>, value: RootId) -> Result<RootId, RootedError> {
    let descriptor = c.null_object_rooted()?;
    put(c, descriptor, "value", value)?;
    let no = c.boolean(false);
    let yes = c.boolean(true);
    put(c, descriptor, "enumerable", no)?;
    put(c, descriptor, "writable", no)?;
    put(c, descriptor, "configurable", yes)?;
    Ok(descriptor)
}

/// The promisified function: calls the original with a settling callback appended.
pub(crate) fn promisified(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let original = get(c, data, "0")?;
    let names = get(c, data, "1")?;
    let promise = c.pending_promise_rooted()?;
    let settle_data = c.array_rooted(&[promise, names])?;
    let settle = c.host_function_with_data(crate::host::shared_vm::operation!("utilPromisifySettle"), settle_data)?;
    let mut arguments = args.to_vec();
    arguments.push(settle);
    if let Err(error) = c.call_rooted(original, this, &arguments) {
        let Some(exception) = error.exception else { return Err(error) };
        c.reject_promise_rooted(promise, exception)?;
    }
    Ok(promise)
}

/// The promisified callback `(err, ...values)`.
pub(crate) fn promisify_settle(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let promise = get(c, data, "0")?;
    let names = get(c, data, "1")?;
    let error = argument(c, args, 0);
    if c.truthy_rooted(error)? {
        c.reject_promise_rooted(promise, error)?;
        return Ok(c.undefined());
    }
    let values = args.get(1..).unwrap_or_default();
    let value = if type_of(c, names) != ValueType::Undefined && values.len() > 1 {
        let object = c.object_rooted()?;
        let names = c.array_like_values_rooted(names)?;
        for (index, name) in names.into_iter().enumerate() {
            let value = values.get(index).copied().unwrap_or_else(|| c.undefined());
            c.set_property_rooted(object, name, value, object)?;
        }
        object
    } else {
        argument(c, values, 0)
    };
    c.resolve_promise_rooted(promise, value)?;
    Ok(c.undefined())
}

/// `promisify(target.close).call(target)`: the promise `close`'s callback settles, which
/// a resource's `[Symbol.asyncDispose]()` awaits.
pub(crate) fn close_promise(c: &mut Context<'_>, target: RootId) -> Result<RootId, RootedError> {
    let promise = c.pending_promise_rooted()?;
    let undefined = c.undefined();
    let data = c.array_rooted(&[promise, undefined])?;
    let settle = c.host_function_with_data(crate::host::shared_vm::operation!("utilPromisifySettle"), data)?;
    let close = get(c, target, "close")?;
    if let Err(error) = c.call_rooted(close, target, &[settle]) {
        let Some(exception) = error.exception else { return Err(error) };
        c.reject_promise_rooted(promise, exception)?;
    }
    Ok(promise)
}

/// The `[Symbol.asyncDispose]()` of a resource already released: a promise of undefined.
pub(crate) fn disposed_promise(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let promise = c.pending_promise_rooted()?;
    let undefined = c.undefined();
    c.resolve_promise_rooted(promise, undefined)?;
    Ok(promise)
}

/// `util.log(...args)`: `console.log('%s - %s', timestamp(), format(...args))`.
pub(crate) fn log(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    let mut local = unsafe { std::mem::zeroed::<libc::tm>() };
    unsafe { libc::localtime_r(&now, &mut local) };
    let stamp = format!(
        "{} {} {:02}:{:02}:{:02}",
        local.tm_mday, MONTHS[local.tm_mon as usize], local.tm_hour, local.tm_min, local.tm_sec
    );
    let message = inspect::format_with_options(c, None, args)?;
    let mut line = format!("{stamp} - ").encode_utf16().collect::<Vec<_>>();
    line.extend(message);
    let line = c.string_units_rooted(&line);
    let global = c.global_root()?;
    let console = get(c, global, "console")?;
    let log = get(c, console, "log")?;
    c.call_rooted(log, console, &[line])?;
    Ok(c.undefined())
}

/// `util.transferableAbortSignal(signal)`.
pub(crate) fn transferable_abort_signal(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let signal = argument(c, args, 0);
    let global = c.global_root()?;
    let class = get(c, global, "AbortSignal")?;
    if !c.is_object_rooted(signal)? || !c.instance_of_rooted(signal, class)? {
        return Err(super::util_shared_vm::argument_type_error(c, signal, "signal", "AbortSignal")?);
    }
    Ok(signal)
}

/// `util.transferableAbortController()`.
pub(crate) fn transferable_abort_controller(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let class = get(c, global, "AbortController")?;
    c.construct_rooted(class, class, &[])
}

/// `util.aborted(signal, resource)` (lib/internal/abort_controller.js, an async function):
/// a promise that resolves with the `abort` event once `signal` aborts, at once if it has;
/// an invalid argument rejects it.
pub(crate) fn aborted(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let promise = c.pending_promise_rooted()?;
    if let Err(error) = settle_aborted(c, promise, args) {
        let Some(exception) = error.exception else {
            return Err(error);
        };
        c.reject_promise_rooted(promise, exception)?;
    }
    Ok(promise)
}

fn settle_aborted(c: &mut Context<'_>, promise: RootId, args: &[RootId]) -> Result<(), RootedError> {
    let signal = argument(c, args, 0);
    let resource = argument(c, args, 1);
    if c.rooted_value(signal).is_some_and(|value| value.is_undefined()) {
        return Err(super::util_shared_vm::argument_type_error(c, signal, "signal", "AbortSignal")?);
    }
    super::util_shared_vm::validate_abort_signal(c, signal, "signal")?;
    // `validateObject(resource, 'resource', kValidateObjectAllowObjects)`: arrays and
    // functions pass.
    if !c.is_object_rooted(resource)? && !c.is_callable_rooted(resource)? {
        return Err(super::util_shared_vm::argument_type_error(c, resource, "resource", "Object")?);
    }
    let is_aborted = get(c, signal, "aborted")?;
    if c.truthy_rooted(is_aborted)? {
        let undefined = c.undefined();
        return c.resolve_promise_rooted(promise, undefined).map(drop);
    }
    let resolve = c.host_function_with_data(crate::host::shared_vm::operation!("utilAbortedResolve"), promise)?;
    let api = c
        .host_mut()
        .shared_state()
        .borrow()
        .event_targets_api
        .ok_or_else(|| RootedError::host("EventTarget API missing"))?;
    // `{ once: true, [kResistStopPropagation]: true }`.
    let options = get(c, api, "abortListenerOptions")?;
    let add = get(c, signal, "addEventListener")?;
    let kind = c.string_rooted("abort");
    c.call_rooted(add, signal, &[kind, resolve, options])?;
    Ok(())
}

pub(crate) fn aborted_resolve(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let promise = c.host_function_data()?;
    let event = argument(c, args, 0);
    c.resolve_promise_rooted(promise, event)?;
    Ok(c.undefined())
}
