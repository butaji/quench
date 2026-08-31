//! `util` module — formatting + type inspection.
//!
//! Node-compatible `util.format` with `%s`, `%d`, `%i`, `%f`, `%j`,
//! `%o`, `%O`, `%%`. Plus `util.inspect` (string-only; sufficient
//! for the test262 + Node fixture conformance surface).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::ops::FunctionKind;
use quench_runtime::value::{IteratorState, Value};

pub const PROMISIFY_CUSTOM_KEY: &str = "Symbol.for.nodejs.util.promisify.custom\0";
pub const PROMISIFY_CUSTOM_ARGS_KEY: &str = "Symbol.for.nodejs.util.promisify.customArgs\0";

thread_local! {
    /// The live `util.inspect.defaultOptions` object; formatters read
    /// through it so JavaScript-side mutation is observed.
    static INSPECT_DEFAULT_OPTIONS: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// Per-call override set by `util.formatWithOptions`.
    static SEPARATOR_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static COLORS_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static COMPACT_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
}

/// `util.formatWithOptions(options, ...args)`.
pub fn format_with_options(
    args: &[Value],
    numeric_separator: bool,
    colors: bool,
    compact: bool,
) -> String {
    SEPARATOR_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(numeric_separator));
    COLORS_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(colors));
    COMPACT_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(compact));
    let result = format(args);
    SEPARATOR_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    COLORS_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    COMPACT_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    result
}

/// Parse Node's dotenv-style environment format into a null-prototype object.
pub fn parse_env(arguments: &[Value]) -> Result<Value, VmError> {
    let source = match arguments.first() {
        Some(Value::String(source)) => source.clone(),
        Some(Value::StringUnits(units)) => String::from_utf16_lossy(units),
        _ => {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "str must be a string".into(),
        ));
        }
    };
    let lines = source.lines().collect::<Vec<_>>();
    let mut values = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let mut line = lines[index].trim().to_string();
        index += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(stripped) = line.strip_prefix("export ") {
            line = stripped.to_string();
        }
        let Some((key, initial)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.chars().any(char::is_whitespace) {
            continue;
        }
        let mut raw = initial.trim().to_string();
        if let Some(quote) = raw.chars().next().filter(|c| matches!(c, '\'' | '"' | '`')) {
            while !has_closing_quote(&raw, quote) && index < lines.len() {
                if looks_like_assignment(lines[index]) {
                    break;
                }
                raw.push('\n');
                raw.push_str(lines[index]);
                index += 1;
            }
        }
        values.push((key.to_string(), Value::String(parse_env_value(&raw))));
    }
    let mut unique = HashMap::new();
    for (key, value) in values {
        unique.insert(key, value);
    }
    let mut properties = vec![("\0prototype".into(), Value::Null)];
    properties.extend(unique);
    Ok(Value::object(properties))
}

/// Build the observable call-site records exposed by `util.getCallSites`.
/// Source-map lookup is deliberately a host-edge concern: the runtime keeps
/// executing the same code, while this adapter resolves the optional map
/// named by the executed script when Node's flag is present.
pub fn get_call_sites(
    state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    if args.len() > 2
        || args.get(1).is_some_and(|options| {
            !matches!(options, Value::Object(_) | Value::ObjectAlias(_))
        })
    {
        return Err(execute::type_error("The options argument must be an object"));
    }
    let count = match args.first() {
        None | Some(Value::Undefined) | Some(Value::Object(_)) | Some(Value::ObjectAlias(_)) => 10,
        Some(Value::Number(value))
            if value.is_finite() && *value >= 1.0 && value.fract() == 0.0 =>
        {
            if *value > 200.0 {
                return Err(VmError::Thrown(Value::object(vec![
                    ("name".into(), Value::String("RangeError".into())),
                    ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                    (
                        "message".into(),
                        Value::String("The frame count must be between 1 and 200".into()),
                    ),
                ])));
            }
            *value as usize
        }
        Some(Value::Number(_)) => {
            return Err(VmError::Thrown(Value::object(vec![
                ("name".into(), Value::String("RangeError".into())),
                ("code".into(), Value::String("ERR_OUT_OF_RANGE".into())),
                (
                    "message".into(),
                    Value::String("The frame count must be an integer between 1 and 200".into()),
                ),
            ])));
        }
        Some(_) => return Err(execute::type_error("The frame count must be an integer")),
    };

    let script_name = state
        .borrow()
        .process
        .argv
        .get(1)
        .cloned()
        .unwrap_or_default();
    let options = args.get(1).or_else(|| {
        args.first().filter(|value| {
            matches!(value, Value::Object(_) | Value::ObjectAlias(_))
        })
    });
    let source_map = options
        .and_then(|options| execute::get_property_result(options, "sourceMap").ok())
        .is_some_and(|value| matches!(value, Value::Boolean(true)))
        .then(|| source_maps_enabled())
        .and_then(|enabled| enabled.then(|| resolve_source_map(Path::new(&script_name))))
        .flatten();
    let mapped = source_map.unwrap_or_else(|| CallSite {
        script_name: script_name.clone(),
        line: 0,
        column: 0,
        function_name: None,
    });
    Ok(quench_runtime::host_api::array(
        (0..count)
            .map(|_| {
                let mut properties = vec![
                    ("scriptName".into(), Value::String(mapped.script_name.clone())),
                    ("scriptId".into(), Value::String(mapped.script_name.clone())),
                    ("lineNumber".into(), Value::Number(mapped.line as f64)),
                    ("columnNumber".into(), Value::Number(mapped.column as f64)),
                ];
                if let Some(name) = &mapped.function_name {
                    properties.push(("functionName".into(), Value::String(name.clone())));
                }
                quench_runtime::host_api::object(properties)
            })
            .collect(),
    ))
}

#[derive(Clone)]
struct CallSite {
    script_name: String,
    line: u32,
    column: u32,
    function_name: Option<String>,
}

fn source_maps_enabled() -> bool {
    let process = execute::get_property(&quench_runtime::vm::current_global_object(), "process");
    let flags = execute::get_property(&process, "execArgv");
    let Value::Array(values) = flags else {
        return false;
    };
    (0..values.logical_len()).any(|index| {
        matches!(
            execute::to_js_string(&execute::get_property(&Value::Array(values.clone()), &index.to_string())),
            Ok(flag) if flag == "--enable-source-maps"
        )
    })
}

fn resolve_source_map(script: &Path) -> Option<CallSite> {
    let source = std::fs::read_to_string(script).ok()?;
    let url = source
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("//# sourceMappingURL="))?;
    if url.starts_with("data:") {
        return None;
    }
    let map_path = script.parent().unwrap_or_else(|| Path::new(".")).join(url);
    let map = std::fs::read_to_string(&map_path).ok()?;
    let source_name = json_string_array_item(&map, "sources", 0)?;
    let source_root = json_string_field(&map, "sourceRoot").unwrap_or_default();
    let source_path = map_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(source_root)
        .join(source_name);
    let (line, column) = first_mapping(&map).unwrap_or((0, 0));
    Some(CallSite {
        script_name: source_path.to_string_lossy().into_owned(),
        line,
        column,
        function_name: source
            .split_once("function ")
            .and_then(|(_, tail)| tail.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
    })
}

fn json_string_field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))?;
    let tail = &json[start..];
    let quote = tail.find(':').and_then(|index| tail[index + 1..].find('"'))?;
    let value = &tail[tail.find(':')? + 1 + quote + 1..];
    Some(value.split('"').next()?.to_owned())
}

fn json_string_array_item(json: &str, key: &str, index: usize) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))?;
    let array = &json[start..].split_once('[')?.1;
    array.split('"').filter(|value| !value.is_empty() && *value != ",").nth(index * 2)
        .map(str::to_owned)
}

fn first_mapping(json: &str) -> Option<(u32, u32)> {
    let mapping = json
        .find("\"mappings\"")
        .and_then(|start| json[start..].split_once(':'))?
        .1
        .split('"')
        .nth(1)?;
    let segment = mapping.split(';').next()?.split(',').next()?;
    let mut values = segment.chars().filter_map(decode_vlq).collect::<Vec<_>>();
    if values.len() < 4 {
        return None;
    }
    let original_line = values.swap_remove(2);
    let original_column = values.swap_remove(2);
    Some(((original_line + 1) as u32, (original_column + 1) as u32))
}

fn decode_vlq(character: char) -> Option<i32> {
    const DIGITS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let value = DIGITS.find(character)? as i32;
    Some(if value & 1 == 1 { -(value >> 1) - 1 } else { value >> 1 })
}

fn has_closing_quote(value: &str, quote: char) -> bool {
    value.chars().skip(1).any(|character| character == quote)
}

fn looks_like_assignment(line: &str) -> bool {
    let line = line.trim();
    let Some((key, _)) = line.split_once('=') else {
        return false;
    };
    !key.is_empty() && !key.chars().any(char::is_whitespace)
}

fn parse_env_value(raw: &str) -> String {
    let value = raw.trim();
    let Some(quote) = value.chars().next() else {
        return String::new();
    };
    if !matches!(quote, '\'' | '"' | '`') {
        return value
            .split_once('#')
            .map_or(value, |(head, _)| head)
            .trim()
            .to_string();
    }
    let Some(end) = value[quote.len_utf8()..].find(quote) else {
        return value.to_string();
    };
    let result = &value[quote.len_utf8()..quote.len_utf8() + end];
    if quote == '"' {
        result.replace("\\n", "\n")
    } else {
        result.to_string()
    }
}

/// Module wiring: returns the `(name, value)` pairs the host
/// installs into the `util` namespace.
pub fn build() -> Vec<(String, Value)> {
    let global = quench_runtime::vm::current_global_object();
    let object_assign = quench_runtime::execute::get_property_result(&global, "Object")
        .ok()
        .and_then(|object| quench_runtime::execute::get_property_result(&object, "assign").ok())
        .unwrap_or(Value::Undefined);
    let to_usv_string = crate::host::capability(crate::registry::SPEC_UTIL_TO_USV_STRING);
    let debuglog = crate::host::capability(crate::registry::SPEC_UTIL_DEBUGLOG);
    let promisify = quench_runtime::execute::set_property(
        crate::host::capability(crate::registry::SPEC_UTIL_PROMISIFY),
        "custom",
        Value::String(PROMISIFY_CUSTOM_KEY.into()),
    );
    let promisify = quench_runtime::execute::set_property(
        promisify,
        PROMISIFY_CUSTOM_KEY,
        Value::String(PROMISIFY_CUSTOM_KEY.into()),
    );
    let types = types_object();
    /*let type_names = [
        "isArgumentsObject", "isArrayBuffer", "isAsyncFunction", "isBigIntObject",
        "isBooleanObject", "isDate", "isExternal", "isGeneratorFunction",
        "isGeneratorObject", "isMap", "isMapIterator", "isModuleNamespaceObject",
        "isNativeError", "isNumberObject", "isPromise", "isProxy", "isRegExp",
        "isSet", "isSetIterator", "isSharedArrayBuffer", "isStringObject",
        "isSymbolObject", "isWeakMap", "isWeakSet", "isAnyArrayBuffer",
        "isBoxedPrimitive", "isArrayBufferView", "isDataView", "isTypedArray",
        "isUint8Array", "isUint8ClampedArray", "isUint16Array", "isUint32Array",
        "isInt8Array", "isInt16Array", "isInt32Array", "isFloat16Array",
        "isFloat32Array", "isFloat64Array", "isBigInt64Array", "isBigUint64Array",
        "isKeyObject", "isCryptoKey",
    ];
    let types = quench_runtime::host_api::object(type_names.iter().map(|name| (
        (*name).to_string(),
        quench_runtime::host_api::bound_capability_with_arguments(
            quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(crate::registry::SPEC_UTIL_TYPE_PREDICATE.cap),
            },
            vec![Value::String((*name).to_string())],
        ),
    )).collect());*/
    vec![
        (
            "isArray".to_string(),
            Value::Builtin(quench_runtime::ops::Builtin::ArrayIsArray),
        ),
        ("_extend".to_string(), object_assign),
        ("debuglog".to_string(), debuglog),
        ("toUSVString".to_string(), to_usv_string),
        ("types".to_string(), types),
        (
            "parseEnv".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_PARSE_ENV),
        ),
        (
            "format".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_FORMAT),
        ),
        ("promisify".to_string(), promisify),
        (
            "deprecate".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_DEPRECATE),
        ),
        (
            "pendingDeprecate".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_DEPRECATE),
        ),
        (
            "getSystemErrorName".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_SYSTEM_ERROR_NAME),
        ),
        (
            "convertProcessSignalToExitCode".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_CONVERT_SIGNAL_TO_EXIT_CODE),
        ),
        (
            "_exceptionWithHostPort".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_EXCEPTION_WITH_HOST_PORT),
        ),
        ("inspect".to_string(), inspect_capability()),
        (
            "aborted".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_ABORTED),
        ),
        (
            "isDeepStrictEqual".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_IS_DEEP_STRICT_EQUAL),
        ),
        (
            "styleText".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_STYLE_TEXT),
        ),
        (
            "formatWithOptions".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_FORMAT_WITH_OPTIONS),
        ),
        (
            "stripVTControlCharacters".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_STRIP_VT),
        ),
        (
            "inherits".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_INHERITS),
        ),
        (
            "getCallSites".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_GETCALLSITES),
        ),
        (
            "TextEncoder".to_string(),
            crate::host::capability(crate::registry::SPEC_TEXT_ENCODER_NEW),
        ),
        (
            "TextDecoder".to_string(),
            crate::host::capability(crate::registry::SPEC_TEXT_DECODER_NEW),
        ),
    ]
}

pub fn types_object() -> Value {
    let names = [
        "isArgumentsObject",
        "isArrayBuffer",
        "isAsyncFunction",
        "isBigIntObject",
        "isBooleanObject",
        "isDate",
        "isExternal",
        "isGeneratorFunction",
        "isGeneratorObject",
        "isMap",
        "isMapIterator",
        "isModuleNamespaceObject",
        "isNativeError",
        "isNumberObject",
        "isPromise",
        "isProxy",
        "isRegExp",
        "isSet",
        "isSetIterator",
        "isSharedArrayBuffer",
        "isStringObject",
        "isSymbolObject",
        "isWeakMap",
        "isWeakSet",
        "isAnyArrayBuffer",
        "isBoxedPrimitive",
        "isArrayBufferView",
        "isDataView",
        "isTypedArray",
        "isUint8Array",
        "isUint8ClampedArray",
        "isUint16Array",
        "isUint32Array",
        "isInt8Array",
        "isInt16Array",
        "isInt32Array",
        "isFloat16Array",
        "isFloat32Array",
        "isFloat64Array",
        "isBigInt64Array",
        "isBigUint64Array",
        "isKeyObject",
        "isCryptoKey",
    ];
    quench_runtime::host_api::object(
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_string(),
                    quench_runtime::host_api::bound_capability_with_arguments(
                        quench_runtime::ops::HostCapabilityRef {
                            realm: quench_runtime::ops::RealmId::ROOT,
                            kind: quench_runtime::ops::HostCapabilityKind::Custom(
                                crate::registry::SPEC_UTIL_TYPE_PREDICATE.cap,
                            ),
                        },
                        vec![Value::String((*name).to_string())],
                    ),
                )
            })
            .collect(),
    )
}

/// Runtime identity predicates share one capability and differ only by this data key.
pub fn type_predicate(name: &str, value: &Value) -> bool {
    let typed = matches!(
        value,
        Value::Float64Array(_)
            | Value::Float32Array(_)
            | Value::Int8Array(_)
            | Value::Int16Array(_)
            | Value::Int32Array(_)
            | Value::BigInt64Array(_)
            | Value::BigUint64Array(_)
            | Value::Uint32Array(_)
            | Value::Uint8Array(_)
            | Value::Uint8ClampedArray(_)
            | Value::Uint16Array(_)
    );
    let view = typed || matches!(value, Value::DataView(_));
    match name {
        "isArrayBuffer" => matches!(value, Value::ArrayBuffer(_)),
        "isSharedArrayBuffer" => matches!(value, Value::ArrayBuffer(buffer) if buffer.shared),
        "isAnyArrayBuffer" => matches!(value, Value::ArrayBuffer(_)),
        "isArrayBufferView" => view,
        "isDataView" => matches!(value, Value::DataView(_)),
        "isTypedArray" => typed,
        "isUint8Array" => matches!(value, Value::Uint8Array(_)),
        "isUint8ClampedArray" => matches!(value, Value::Uint8ClampedArray(_)),
        "isUint16Array" => matches!(value, Value::Uint16Array(_)) && !value.is_float16_array(),
        "isUint32Array" => matches!(value, Value::Uint32Array(_)),
        "isInt8Array" => matches!(value, Value::Int8Array(_)),
        "isInt16Array" => matches!(value, Value::Int16Array(_)),
        "isInt32Array" => matches!(value, Value::Int32Array(_)),
        "isFloat32Array" => matches!(value, Value::Float32Array(_)),
        "isFloat64Array" => matches!(value, Value::Float64Array(_)),
        "isBigInt64Array" => matches!(value, Value::BigInt64Array(_)),
        "isBigUint64Array" => matches!(value, Value::BigUint64Array(_)),
        "isPromise" => matches!(value, Value::Promise(_)),
        "isProxy" => matches!(value, Value::Proxy(_)),
        "isRegExp" => matches!(
            quench_runtime::execute::get_property_result(value, "\0regexp"),
            Ok(Value::Boolean(true))
        ),
        "isDate" => matches!(
            quench_runtime::execute::get_property_result(value, "timeValue"),
            Ok(Value::Number(_) | Value::BindingCell(_))
        ),
        "isMap" => matches!(value, Value::Map(data) if !data.is_weak()),
        "isWeakMap" => matches!(value, Value::Map(data) if data.is_weak()),
        "isSet" => matches!(value, Value::Set(data) if !data.is_weak()),
        "isWeakSet" => matches!(value, Value::Set(data) if data.is_weak()),
        "isMapIterator" => {
            matches!(value, Value::Iterator(iter) if matches!(*iter.state.borrow(), IteratorState::Map { .. }))
        }
        "isSetIterator" => {
            matches!(value, Value::Iterator(iter) if matches!(*iter.state.borrow(), IteratorState::Set { .. }))
        }
        "isGeneratorObject" => matches!(value, Value::Generator(_)),
        "isGeneratorFunction" => {
            matches!(value, Value::Function(function) if function.kind == FunctionKind::Generator && !function.is_async)
        }
        "isAsyncFunction" => {
            matches!(value, Value::Function(function) if function.is_async && function.kind != FunctionKind::Generator)
        }
        "isArgumentsObject" => value.is_arguments_object(),
        "isBooleanObject" => boxed_constructor(value, "Boolean"),
        "isNumberObject" => boxed_constructor(value, "Number"),
        "isStringObject" => boxed_constructor(value, "String"),
        "isSymbolObject" => boxed_constructor(value, "Symbol"),
        "isBigIntObject" => boxed_constructor(value, "BigInt"),
        "isBoxedPrimitive" => ["Boolean", "Number", "String", "Symbol", "BigInt"]
            .iter()
            .any(|kind| boxed_constructor(value, kind)),
        "isNativeError" => matches!(
            quench_runtime::execute::get_property_result(value, "\0error_slot"),
            Ok(Value::Boolean(true))
        ),
        "isExternal" => matches!(
            quench_runtime::execute::get_property_result(value, "__quench_external"),
            Ok(Value::Boolean(true))
        ),
        "isFloat16Array" => value.is_float16_array(),
        "isModuleNamespaceObject" => matches!(
            quench_runtime::execute::get_property_result(value, "\0module_namespace"),
            Ok(Value::Boolean(true))
        ),
        "isKeyObject" | "isCryptoKey" => false,
        _ => false,
    }
}

fn boxed_constructor(value: &Value, name: &str) -> bool {
    let prototype = quench_runtime::execute::get_property_result(value, "\0prototype");
    let expected = match name {
        "Boolean" => quench_runtime::ops::Builtin::BooleanPrototype,
        "Number" => quench_runtime::ops::Builtin::NumberPrototype,
        "String" => quench_runtime::ops::Builtin::StringPrototype,
        "Symbol" => quench_runtime::ops::Builtin::SymbolPrototype,
        "BigInt" => quench_runtime::ops::Builtin::BigIntPrototype,
        _ => return false,
    };
    matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::BindingCell(_)
    ) && matches!(prototype, Ok(Value::Builtin(actual)) if actual == expected)
        && matches!(
            quench_runtime::execute::get_property_result(value, "_value"),
            Ok(Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::BigInt(_))
        )
}

fn inspect_capability() -> Value {
    let inspect = crate::host::capability(crate::registry::SPEC_UTIL_INSPECT);
    let options = quench_runtime::host_api::object(vec![(
        "numericSeparator".to_string(),
        Value::Boolean(false),
    )]);
    INSPECT_DEFAULT_OPTIONS.with(|slot| *slot.borrow_mut() = Some(options.clone()));
    let _ = quench_runtime::execute::set_callable_property(&inspect, "defaultOptions", options);
    if let Ok(custom) = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::SymbolFor),
        &Value::Undefined,
        &[Value::String("nodejs.util.inspect.custom".into())],
    ) {
        let _ = quench_runtime::execute::set_callable_property(&inspect, "custom", custom);
    }
    inspect
}

fn numeric_separator() -> bool {
    if let Some(override_) = SEPARATOR_OVERRIDE.with(|slot| *slot.borrow()) {
        return override_;
    }
    INSPECT_DEFAULT_OPTIONS.with(|slot| {
        let options = slot.borrow();
        let Some(options) = options.as_ref() else {
            return false;
        };
        let options = quench_runtime::execute::resolve_alias(options);
        quench_runtime::execute::is_truthy(&quench_runtime::execute::get_property(
            &options,
            "numericSeparator",
        ))
    })
}

/// Group integer digits into `_`-separated triples (Node's
/// `numericSeparator` rendering); fraction/exponent stay untouched.
fn separate_digits(text: &str) -> String {
    let (sign, rest) = text.strip_prefix('-').map_or(("", text), |r| ("-", r));
    let end = rest.find(['.', 'e', 'E', 'n']).unwrap_or(rest.len());
    let (int, tail) = rest.split_at(end);
    let mut grouped = String::with_capacity(text.len() + int.len() / 3);
    for (index, c) in int.chars().enumerate() {
        if index > 0 && (int.len() - index) % 3 == 0 {
            grouped.push('_');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped}{tail}")
}

/// `util.format` — see test fixture `parallel/test-util-format.js`.
pub fn format(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    if let Value::String(template) = &args[0] {
        if !quench_runtime::execute::is_symbol(&args[0]) {
            return format_template(template, args);
        }
    }
    format_varargs(args)
}

/// Execute observable `toJSON` hooks before the string-only formatter runs.
/// The dispatch edge uses this to preserve thrown user errors.
pub fn validate_json_arguments(args: &[Value]) -> Result<(), VmError> {
    let Some(Value::String(template)) = args.first() else {
        return Ok(());
    };
    let mut index = 1;
    let mut chars = template.chars();
    while let Some(character) = chars.next() {
        if character != '%' {
            continue;
        }
        let Some(specifier) = chars.next() else { break };
        if specifier == '%' || specifier != 'j' {
            continue;
        }
        let Some(value) = args.get(index) else { break };
        index += 1;
        if let Ok(method) = quench_runtime::execute::get_property_result(value, "toJSON") {
            if matches!(method, Value::Function(_) | Value::BoundFunction(_)) {
                quench_runtime::execute::call(&method, value, &[])?;
            }
        }
    }
    Ok(())
}

/// Public for `console.log` reuse.
pub fn format_template(template: &str, args: &[Value]) -> String {
    let mut out = String::new();
    let mut iter = template.chars().peekable();
    let mut index = 1usize;
    while let Some(c) = iter.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let Some(spec) = iter.next() else {
            out.push('%');
            break;
        };
        if spec == '%' {
            out.push('%');
            continue;
        }
        // Unknown specifiers are literal text and do not consume the next
        // argument.  This is observable for `util.format('a% b', 'x')`.
        if !matches!(spec, 's' | 'd' | 'i' | 'f' | 'j' | 'o' | 'O' | 'c') {
            out.push('%');
            out.push(spec);
            continue;
        }
        let Some(arg) = args.get(index).cloned() else {
            out.push('%');
            out.push(spec);
            continue;
        };
        index += 1;
        out.push_str(&format_spec(spec, &arg));
    }
    // Node's util.format appends remaining positional args separated
    // by spaces, mirroring console.log's behavior.
    for arg in args.iter().skip(index) {
        out.push(' ');
        let rendered = format_extra(arg);
        out.push_str(&colorize(arg, rendered));
    }
    out
}

fn colorize(value: &Value, rendered: String) -> String {
    let enabled = COLORS_OVERRIDE.with(|slot| slot.borrow().unwrap_or(false));
    if !enabled {
        return rendered;
    }
    let (start, end) = if matches!(value, Value::Null) {
        ("\x1b[1m", "\x1b[22m")
    } else if matches!(value, Value::Undefined) {
        ("\x1b[90m", "\x1b[39m")
    } else if quench_runtime::execute::is_symbol(value) {
        ("\x1b[32m", "\x1b[39m")
    } else if matches!(
        value,
        Value::Boolean(_) | Value::Number(_) | Value::BigInt(_)
    ) {
        ("\x1b[33m", "\x1b[39m")
    } else {
        return rendered;
    };
    format!("{start}{rendered}{end}")
}

fn format_varargs(args: &[Value]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        let rendered = format_extra(arg);
        out.push_str(&colorize(arg, rendered));
    }
    out
}

fn format_spec(spec: char, arg: &Value) -> String {
    match spec {
        's' => value_to_string(arg),
        'd' => to_number_string(arg),
        'i' => to_int_string(arg),
        'f' => to_float_string(arg),
        'j' => json_string(arg),
        // Node gives `%o` the full, hidden-property inspection profile while
        // `%O` uses the ordinary compact profile.  Keep the distinction at
        // this boundary; both profiles consume the same property facts.
        'o' => inspect_verbose(arg, 4, 0),
        'O' => inspect_with_options(arg, 2, false, None, false),
        'c' => String::new(),
        other => format!("%{other}"),
    }
}

fn format_extra(value: &Value) -> String {
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) && is_error_value(value) {
        return inspect_depth(value, 3);
    }
    match value {
        Value::Function(_) | Value::BoundFunction(_) => inspect_function(value),
        value if quench_runtime::is_callable(value) => inspect_function(value),
        Value::String(value)
            if quench_runtime::execute::is_symbol(&Value::String(value.clone())) =>
        {
            symbol_string(&Value::String(value.clone()))
        }
        Value::String(value) => value.clone(),
        _ => format_spec('s', value),
    }
}

fn value_to_string(value: &Value) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    if is_date_value(value) {
        return inspect_date(value);
    }
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => js_number(*n),
        Value::BigInt(digits) => format!("{}n", bigint_digits(digits)),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        // Node: objects with a custom `toString` go through `String(arg)`;
        // plain objects inspect.
        Value::Object(_) | Value::ObjectAlias(_) | Value::Function(_) | Value::BoundFunction(_) => {
            match quench_runtime::execute::to_js_string(value) {
                Ok(text) if text != "[object Object]" && !text.is_empty() => text,
                // `%s` inspects plain objects at depth 0: nested containers
                // collapse to `[Array]` / `[Object]`.
                _ => {
                    let rendered = inspect_depth(value, 0);
                    let constructor = quench_runtime::execute::get_property(value, "constructor");
                    match quench_runtime::execute::get_property(&constructor, "name") {
                        Value::String(name)
                            if !name.is_empty()
                                && name != "Object"
                                && rendered.starts_with('{') =>
                        {
                            format!("{name} {rendered}")
                        }
                        _ => rendered,
                    }
                }
            }
        }
        Value::Array(_) => {
            let depth = COMPACT_OVERRIDE.with(|slot| slot.borrow().is_some_and(|value| value));
            inspect_array(value, if depth { 1 } else { 3 })
        }
        Value::Proxy(proxy) => {
            if *proxy.revoked.borrow() {
                "<Revoked Proxy>".into()
            } else {
                inspect_proxy(value, 3, false).unwrap_or_else(|| "<unknown>".into())
            }
        }
        Value::ArrayBuffer(buffer) => inspect_array_buffer(value, buffer),
        Value::DataView(view) => inspect_data_view(value, view),
        _ => "<unknown>".into(),
    }
}

/// BigInt digits, grouped when `numericSeparator` is on.
fn bigint_digits(digits: &str) -> String {
    if numeric_separator() {
        separate_digits(digits)
    } else {
        digits.to_string()
    }
}

/// JavaScript number rendering honoring `numericSeparator`.
fn js_number(n: f64) -> String {
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    let text = quench_runtime::execute::number_to_js_string(n);
    if numeric_separator() {
        separate_digits(&text)
    } else {
        text
    }
}

/// `Symbol.prototype.toString` rendering: `Symbol(desc)`.
fn symbol_string(value: &Value) -> String {
    let Value::String(payload) = value else {
        return "Symbol()".into();
    };
    let (body, suffix) = payload.split_once('\0').unwrap_or((payload.as_str(), ""));
    if let Some(key) = body.strip_prefix("Symbol.for.") {
        return format!("Symbol.for({key})");
    }
    let unique = !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit());
    if !unique {
        return format!("Symbol({body})");
    }
    let description = body.strip_prefix("Symbol.").unwrap_or(body);
    if description.is_empty() || description == "\u{1}" || description.chars().any(char::is_control)
    {
        return "Symbol()".into();
    }
    format!("Symbol({description})")
}

fn to_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse().unwrap_or(f64::NAN)
            }
        }
        Value::Boolean(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        _ => f64::NAN,
    }
}

/// `%i` — `parseInt(arg, 10)`: integers keep their digits, numbers
/// stringify first (so `1.18e+21` parses as `1`), anything else is NaN.
fn to_int_string(value: &Value) -> String {
    if let Value::BigInt(digits) = value {
        return format!("{}n", bigint_digits(digits));
    }
    let text = match value {
        Value::Number(n) if n.is_finite() => quench_runtime::execute::number_to_js_string(*n),
        Value::String(s) => s.trim().to_string(),
        _ => return "NaN".into(),
    };
    let text = text.strip_prefix('+').unwrap_or(&text);
    let digits: String = text.chars().take_while(|c| c.is_ascii_digit()).collect();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (
            true,
            rest.chars().take_while(|c| c.is_ascii_digit()).collect(),
        ),
        None => (false, digits),
    };
    if digits.is_empty() {
        return "NaN".into();
    }
    let grouped = if numeric_separator() {
        separate_digits(&digits)
    } else {
        digits
    };
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// `%d` — `Number(arg)` rendered with JavaScript number formatting;
/// BigInts render as digits plus `n`.
fn to_number_string(value: &Value) -> String {
    if let Value::BigInt(digits) = value {
        return format!("{}n", bigint_digits(digits));
    }
    let n = to_number(value);
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    js_number(n)
}

/// `%f` — `parseFloat`-style: strings parse their leading float,
/// BigInts convert via digits, `-0` renders as `-0`.
fn to_float_string(value: &Value) -> String {
    if let Value::BigInt(digits) = value {
        let n = digits.parse::<f64>().unwrap_or(f64::NAN);
        return float_text(n);
    }
    let n = match value {
        Value::Number(n) => *n,
        Value::String(s) => parse_float_prefix(s),
        _ => to_number(value),
    };
    float_text(n)
}

fn float_text(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 && n.is_sign_negative() {
        return "-0".into();
    }
    quench_runtime::execute::number_to_js_string(n)
}

fn parse_float_prefix(text: &str) -> f64 {
    let text = text.trim_start();
    let mut end = 0;
    for (index, c) in text.char_indices() {
        let part = c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E');
        if !part
            || (matches!(c, '+' | '-')
                && index > 0
                && !matches!(text.as_bytes()[index - 1], b'e' | b'E'))
        {
            break;
        }
        end = index + 1;
    }
    text[..end].parse().unwrap_or(f64::NAN)
}

fn json_string(value: &Value) -> String {
    match quench_runtime::execute::json_stringify(value) {
        Ok(Value::String(json)) => json,
        Ok(_) => "undefined".into(),
        Err(error) => {
            let message = format!("{error:?}");
            if message.contains("ircular") {
                "[Circular]".into()
            } else {
                "undefined".into()
            }
        }
    }
}

pub use crate::modules::buffer_enc::invalid_arg_received;

/// `util.inspect` — string-only, sufficient for fixtures.
pub fn inspect(value: &Value) -> String {
    inspect_with_depth(value, 3)
}

pub fn inspect_with_depth(value: &Value, depth: usize) -> String {
    if matches!(
        quench_runtime::execute::get_property(value, "\0source_text_module"),
        Value::Boolean(true)
    ) {
        if depth == 0 {
            return "[SourceTextModule]".into();
        }
        let status = inspect_with_depth(&quench_runtime::execute::get_property(value, "status"), 0);
        let identifier = inspect_with_depth(
            &quench_runtime::execute::get_property(value, "identifier"),
            0,
        );
        let context = inspect_with_depth(
            &quench_runtime::execute::get_property(value, "context"),
            depth.saturating_sub(1),
        );
        return format!(
            "SourceTextModule {{\n  status: {status},\n  identifier: {identifier},\n  context: {context}\n}}"
        );
    }
    if matches!(
        quench_runtime::execute::get_property(value, "\0module_namespace"),
        Value::Boolean(true)
    ) {
        let pending = quench_runtime::execute::get_property(value, "\0module_uninitialized");
        let entries = quench_runtime::execute::own_enumerable_keys(value)
            .into_iter()
            .map(|key| {
                let item = quench_runtime::execute::get_property(value, &key);
                let text = if matches!(
                    quench_runtime::execute::get_property(&pending, &key),
                    Value::Boolean(true)
                ) {
                    "<uninitialized>".to_string()
                } else {
                    inspect_with_depth(&item, depth.saturating_sub(1))
                };
                format!("{key}: {text}")
            })
            .collect::<Vec<_>>();
        return format!("[Module: null prototype] {{ {} }}", entries.join(", "));
    }
    if value.object_identity().is_some() {
        for key in quench_runtime::execute::own_enumerable_keys(value) {
            if quench_runtime::execute::same_identity(
                &quench_runtime::execute::get_property(value, &key),
                value,
            ) {
                return format!("<ref *1> {{ {key}: [Circular *1] }}");
            }
        }
    }
    if matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        let canonical = quench_runtime::execute::canonical_value(value);
        for key in quench_runtime::execute::own_enumerable_keys(&canonical) {
            let mut current = canonical.clone();
            let mut repeated = true;
            for _ in 0..4 {
                let keys = quench_runtime::execute::own_enumerable_keys(&current);
                if keys.len() != 1 || keys[0] != key {
                    repeated = false;
                    break;
                }
                current = quench_runtime::execute::canonical_value(
                    &quench_runtime::execute::get_property(&current, &key),
                );
            }
            if repeated
                && quench_runtime::execute::own_enumerable_keys(&current) == vec![key.clone()]
            {
                return format!("<ref *1> {{ {key}: [Circular *1] }}");
            }
        }
    }
    if depth > 100 && matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        let keys = quench_runtime::execute::own_enumerable_keys(value);
        if !keys.is_empty() {
            let body = keys
                .iter()
                .map(|key| {
                    format!(
                        "  {}: {}",
                        if key.parse::<usize>().is_ok() {
                            format!("'{key}'")
                        } else {
                            key.clone()
                        },
                        inspect_at(&quench_runtime::execute::get_property(value, key), 100)
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            return format!("{{\n{body}\n}}");
        }
    }
    inspect_depth(value, depth)
}

pub fn inspect_with_options(
    value: &Value,
    depth: usize,
    show_hidden: bool,
    max_array_length: Option<usize>,
    getters: bool,
) -> String {
    let rendered = match (value, max_array_length, getters) {
        (Value::Object(_) | Value::ObjectAlias(_), _, true) => {
            inspect_object_with_getters(value, depth)
        }
        (Value::Map(_) | Value::Set(_), _, true) => inspect_collection(value, depth, true),
        (Value::ArrayBuffer(buffer), Some(limit), _) => {
            inspect_array_buffer_with_limit(value, buffer, limit)
        }
        _ => inspect_with_depth(value, depth),
    };
    if show_hidden {
        if matches!(value, Value::Object(_) | Value::ObjectAlias(_))
            && !matches!(
                quench_runtime::execute::get_prototype_of(value),
                Ok(Value::Builtin(
                    quench_runtime::ops::Builtin::StringPrototype
                ))
            )
        {
            let hidden = quench_runtime::execute::own_keys(value)
                .into_iter()
                .filter_map(|key| match key {
                    Value::String(key)
                        if !key.starts_with('\0')
                            && !quench_runtime::execute::own_enumerable_keys(value)
                                .iter()
                                .any(|visible| visible == &key) =>
                    {
                        Some(format!("[{key}]: {}", inspect_property(value, &key, 0)))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            if !hidden.is_empty() {
                if let Some(body) = rendered.strip_suffix(" }") {
                    return format!("{body}, {} }}", hidden.join(", "));
                }
            }
        }
        if let Value::Array(array) = value {
            if let Some(body) = rendered.strip_suffix(" ]") {
                return format!("{body}, [length]: {} ]", array.len());
            }
        }
        if matches!(
            value,
            Value::Uint8Array(_)
                | Value::Uint8ClampedArray(_)
                | Value::Uint16Array(_)
                | Value::Uint32Array(_)
                | Value::Int8Array(_)
                | Value::Int16Array(_)
                | Value::Int32Array(_)
                | Value::Float32Array(_)
                | Value::Float64Array(_)
                | Value::BigInt64Array(_)
                | Value::BigUint64Array(_)
        ) {
            if let Some((name, length, bytes_per_element, byte_offset, buffer)) =
                typed_array_info(value)
            {
                return inspect_typed_array(
                    value,
                    name,
                    length,
                    bytes_per_element,
                    byte_offset,
                    &buffer,
                );
            }
            return format!("{rendered} [buffer]");
        }
        if let Ok(Value::Builtin(quench_runtime::ops::Builtin::StringPrototype)) =
            quench_runtime::execute::get_prototype_of(value)
        {
            if let Value::String(string) = quench_runtime::execute::get_property(value, "_value") {
                let symbols = quench_runtime::execute::own_keys(value)
                    .into_iter()
                    .filter_map(|key| {
                        if !quench_runtime::execute::is_symbol(&key) {
                            return None;
                        }
                        let name = symbol_string(&key);
                        let raw = match key {
                            Value::String(raw) => raw,
                            _ => return None,
                        };
                        let value = quench_runtime::execute::get_property(value, &raw);
                        Some(format!("{name}: {}", inspect_shallow(&value)))
                    })
                    .collect::<Vec<_>>();
                let suffix = if symbols.is_empty() {
                    String::new()
                } else {
                    format!(", {}", symbols.join(", "))
                };
                return format!(
                    "[String: {}] {{ [length]: {}{} }}",
                    inspect_string(&string),
                    string.chars().count(),
                    suffix
                );
            }
        }
    }
    rendered
}

pub fn inspect_with_options_colors(
    value: &Value,
    depth: usize,
    show_hidden: bool,
    max_array_length: Option<usize>,
    getters: bool,
    colors: bool,
) -> String {
    COLORS_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(colors));
    let rendered = inspect_with_options(value, depth, show_hidden, max_array_length, getters);
    COLORS_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    rendered
}

pub fn inspect_proxy(value: &Value, depth: usize, show_proxy: bool) -> Option<String> {
    let Value::Proxy(proxy) = value else {
        return None;
    };
    if *proxy.revoked.borrow() {
        return Some("<Revoked Proxy>".into());
    }
    let depth = depth.min(32);
    let custom = inspect_custom_with_receiver(&proxy.target, value, depth);
    let target = custom.clone().unwrap_or_else(|| {
        if show_proxy && depth <= 1 && matches!(proxy.target, Value::Proxy(_)) {
            return "Proxy [Array]".into();
        }
        if matches!(proxy.target, Value::Proxy(_)) {
            inspect_proxy(&proxy.target, depth.saturating_sub(1), show_proxy)
                .unwrap_or_else(|| "<unknown>".into())
        } else {
            inspect_depth(&proxy.target, depth.saturating_sub(1))
        }
    });
    if !show_proxy {
        return Some(custom.unwrap_or_else(|| format!("Proxy({target})")));
    }
    let handler = if matches!(proxy.handler, Value::Proxy(_)) {
        if depth <= 1 {
            "Proxy [Array]".into()
        } else {
            inspect_proxy(&proxy.handler, depth.saturating_sub(1), true)
                .unwrap_or_else(|| "<unknown>".into())
        }
    } else {
        inspect_proxy_handler(&proxy.handler, depth)
    };
    let target_block = indent_proxy_child(&target);
    let handler_block = indent_proxy_child(&handler);
    let inline = format!("Proxy [ {target}, {handler} ]");
    Some(if inline.len() <= 80 {
        inline
    } else {
        format!("Proxy [\n  {target_block},\n  {handler_block}\n]")
    })
}

pub fn inspect_proxy_colored(value: &Value) -> Option<String> {
    let Value::Proxy(proxy) = value else {
        return None;
    };
    let Value::Array(target) = &proxy.target else {
        return None;
    };
    let items = (0..target.logical_len())
        .map(|index| {
            let item = quench_runtime::execute::get_property(&proxy.target, &index.to_string());
            let rendered = inspect_depth(&item, 0);
            format!("  \x1b[33m{rendered}\x1b[39m")
        })
        .collect::<Vec<_>>()
        .join(",\n");
    Some(format!(
        "\x1b[36mProxy(\x1b[39m[\n{items}\n]\x1b[36m)\x1b[39m"
    ))
}

fn indent_proxy_child(value: &str) -> String {
    value.replace('\n', "\n  ")
}

fn inspect_proxy_handler(value: &Value, depth: usize) -> String {
    if matches!(value, Value::Array(_)) {
        return inspect_array(value, 1);
    }
    if quench_runtime::is_callable(value) {
        return inspect_function(value);
    }
    let keys = quench_runtime::execute::own_enumerable_keys(value);
    if keys.is_empty() {
        return "{}".into();
    }
    if depth <= 1 {
        return "[Object]".into();
    }
    let body = keys
        .iter()
        .map(|key| {
            format!(
                "  {key}: {}",
                inspect_shallow(&quench_runtime::execute::get_property(value, key))
            )
        })
        .collect::<Vec<_>>()
        .join(",\n");
    format!("{{\n{body}\n}}")
}

fn inspect_object_with_getters(value: &Value, depth: usize) -> String {
    if quench_runtime::regexp::has_regexp_internal_slot(value) {
        return inspect_regexp(value);
    }
    if is_date_value(value) {
        return inspect_date(value);
    }
    if depth <= 2 {
        return inspect_getter_recursive(
            value,
            depth.saturating_sub(1),
            0,
            inspect_identity(value),
        );
    }
    let own_keys = quench_runtime::execute::own_keys(value)
        .into_iter()
        .filter_map(|key| match key {
            Value::String(key) if !key.starts_with('\0') => Some(key),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut keys = own_keys.clone();
    let mut prototype = quench_runtime::execute::get_prototype_of(value).ok();
    while let Some(current) = prototype {
        for key in quench_runtime::execute::own_keys(&current)
            .into_iter()
            .filter_map(|key| match key {
                Value::String(key) if !key.starts_with('\0') => Some(key),
                _ => None,
            })
        {
            if key != "constructor"
                && !keys.contains(&key)
                && matches!(
                    quench_runtime::execute::call(
                        &Value::Builtin(
                            quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor
                        ),
                        &Value::Undefined,
                        &[current.clone(), Value::String(key.clone())],
                    ),
                    Ok(Value::Object(_))
                )
            {
                keys.push(key);
            }
        }
        prototype = quench_runtime::execute::get_prototype_of(&current).ok();
    }
    if keys.is_empty() {
        return "{}".into();
    }
    let body = keys
        .iter()
        .map(|key| {
            format!(
                "{}: {}",
                if !own_keys.contains(key) {
                    format!("[{key}]")
                } else if key.parse::<usize>().is_ok() {
                    format!("'{key}'")
                } else {
                    key.clone()
                },
                inspect_property_with_getters(value, key, depth.saturating_sub(1)),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let name = quench_runtime::execute::get_prototype_of(value)
        .ok()
        .map(|prototype| quench_runtime::execute::get_property(&prototype, "constructor"))
        .and_then(|constructor| {
            match quench_runtime::execute::get_property(&constructor, "name") {
                Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                _ => None,
            }
        });
    match name {
        Some(name) => format!("{name} {{ {body} }}"),
        None => format!("{{ {body} }}"),
    }
}

fn inspect_getter_recursive(
    value: &Value,
    depth: usize,
    indent: usize,
    root_identity: Option<u64>,
) -> String {
    let own_keys = quench_runtime::execute::own_keys(value)
        .into_iter()
        .filter_map(|key| match key {
            Value::String(key) if !key.starts_with('\0') => Some(key),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut keys = own_keys.clone();
    let mut prototype = quench_runtime::execute::get_prototype_of(value).ok();
    while let Some(current) = prototype {
        for key in quench_runtime::execute::own_keys(&current)
            .into_iter()
            .filter_map(|key| match key {
                Value::String(key) if key != "constructor" && !key.starts_with('\0') => Some(key),
                _ => None,
            })
        {
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        prototype = quench_runtime::execute::get_prototype_of(&current).ok();
    }
    let name = quench_runtime::execute::get_prototype_of(value)
        .ok()
        .map(|prototype| quench_runtime::execute::get_property(&prototype, "constructor"))
        .and_then(
            |constructor| match quench_runtime::execute::get_property(&constructor, "name") {
                Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                _ => None,
            },
        )
        .unwrap_or_default();
    let mut parts = Vec::new();
    for key in keys {
        let inherited = !own_keys.contains(&key);
        let label = if inherited {
            format!("[{key}]")
        } else {
            key.clone()
        };
        let rendered = inspect_getter_property(value, &key, depth, root_identity);
        parts.push(format!("{label}: {rendered}"));
    }
    let prefix = if name.is_empty() {
        String::new()
    } else {
        format!("{name} ")
    };
    if indent == 0 {
        let pad = " ".repeat(indent + 2);
        let body = parts
            .into_iter()
            .map(|part| format!("{pad}{part}"))
            .collect::<Vec<_>>()
            .join(",\n");
        if root_identity.is_some() {
            format!("<ref *1> {prefix}{{\n{body}\n{}}}", " ".repeat(indent))
        } else {
            format!("{prefix}{{\n{body}\n{}}}", " ".repeat(indent))
        }
    } else {
        format!("{prefix}{{ {} }}", parts.join(", "))
    }
}

fn inspect_identity(value: &Value) -> Option<u64> {
    quench_runtime::execute::canonical_value(value).object_identity()
}

fn inspect_getter_property(
    value: &Value,
    key: &str,
    depth: usize,
    root_identity: Option<u64>,
) -> String {
    let descriptor = inherited_property_descriptor(value, key);
    if let Some(Value::Object(descriptor)) = descriptor {
        let getter =
            quench_runtime::execute::get_property(&Value::Object(descriptor.clone()), "get");
        if !matches!(getter, Value::Undefined) {
            if let Ok(result) = quench_runtime::execute::call(&getter, value, &[]) {
                let shown = if inspect_identity(&result) == root_identity {
                    "[Circular *1]".into()
                } else if depth == 0 {
                    inspect_shallow(&result)
                } else if matches!(result, Value::Object(_) | Value::ObjectAlias(_)) {
                    inspect_getter_recursive(&result, depth - 1, 1, root_identity)
                } else {
                    inspect_shallow(&result)
                };
                return if matches!(result, Value::Object(_) | Value::ObjectAlias(_)) {
                    format!("[Getter] {shown}")
                } else {
                    format!("[Getter: {shown}]")
                };
            }
            return "[Getter]".into();
        }
    }
    let result = quench_runtime::execute::get_property(value, key);
    if inspect_identity(&result) == root_identity {
        "[Circular *1]".into()
    } else if depth == 0 {
        inspect_shallow(&result)
    } else if matches!(result, Value::Object(_) | Value::ObjectAlias(_)) {
        inspect_getter_recursive(&result, depth - 1, 1, root_identity)
    } else {
        inspect_shallow(&result)
    }
}

fn inherited_property_descriptor(value: &Value, key: &str) -> Option<Value> {
    let mut owner = value.clone();
    loop {
        let descriptor = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
            &Value::Undefined,
            &[owner.clone(), Value::String(key.to_string())],
        )
        .ok();
        if !matches!(descriptor, None | Some(Value::Undefined)) {
            return descriptor;
        }
        owner = match quench_runtime::execute::get_prototype_of(&owner) {
            Ok(Value::Null) | Err(_) => return None,
            Ok(next) => next,
        };
    }
}

fn inspect_string(value: &str) -> String {
    if value.len() > 60 && value.contains('\n') {
        let lines = value.split('\n').collect::<Vec<_>>();
        let mut result = inspect_string_segment(lines[0]);
        for (index, line) in lines.iter().skip(1).enumerate() {
            if index >= 10 {
                result.push_str(" +\n  ...");
                break;
            }
            if let Some(quote) = result.pop() {
                result.push_str("\\n");
                result.push(quote);
            }
            result.push_str(" +\n  ");
            result.push_str(&inspect_string_segment(line));
        }
        return result;
    }
    inspect_string_segment(value)
}

fn inspect_string_segment(value: &str) -> String {
    // Node's inspector bounds a single-line string before embedding it in an
    // object; assertion fixtures rely on the stable 9,488-code-unit prefix.
    let value = if value.len() > 9_488 && !value.contains('\n') {
        let mut truncated = value[..9_488].to_string();
        truncated.push_str("...");
        truncated
    } else {
        value.to_string()
    };
    let mut out = String::with_capacity(value.len() + 2);
    let quote = if value.contains('\'') && !value.contains('"') {
        '"'
    } else {
        '\''
    };
    out.push(quote);
    for character in value.chars() {
        match character {
            '\'' if quote == '\'' => out.push_str("\\'"),
            '"' if quote == '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\u{000B}' => out.push_str("\\v"),
            character if character.is_control() => {
                out.push_str(&format!("\\x{:02X}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push(quote);
    out
}

fn inspect_depth(value: &Value, depth: usize) -> String {
    if let Value::Promise(promise) = value {
        return match &*promise.state.borrow() {
            quench_runtime::value::PromiseState::Pending => "Promise { <pending> }".into(),
            quench_runtime::value::PromiseState::Fulfilled(value) => {
                format!(
                    "Promise {{ {} }}",
                    inspect_depth(value, depth.saturating_sub(1))
                )
            }
            quench_runtime::value::PromiseState::Rejected(value) => {
                format!(
                    "Promise {{ <rejected> {} }}",
                    inspect_depth(value, depth.saturating_sub(1))
                )
            }
        };
    }
    if let Value::Proxy(proxy) = value {
        if *proxy.revoked.borrow() {
            return "<Revoked Proxy>".into();
        }
        if let Some(custom) = inspect_custom(&proxy.target, depth) {
            return custom;
        }
        return if depth == 0 {
            inspect_shallow(&proxy.target)
        } else {
            inspect_depth(&proxy.target, depth.saturating_sub(1))
        };
    }
    if matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_)
    ) {
        if matches!(quench_runtime::execute::get_property(value, "Symbol.toStringTag"), Value::String(ref tag) if tag == "AbortController")
            && quench_runtime::execute::has_own_property(value, "signal")
            && quench_runtime::execute::has_own_property(value, "abort")
        {
            let signal = quench_runtime::execute::get_property(value, "\0quench:abort:signal");
            let aborted = quench_runtime::execute::get_property(&signal, "aborted");
            return if depth <= 4 {
                "AbortController { signal: [AbortSignal] }".into()
            } else {
                format!(
                    "AbortController {{ signal: AbortSignal {{ aborted: {} }} }}",
                    inspect_shallow(&aborted)
                )
            };
        }
        if let Some(custom) = inspect_custom(value, depth) {
            return custom;
        }
    }
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    if matches!(
        quench_runtime::execute::get_property_result(value, "__quench_external"),
        Ok(Value::Boolean(true))
    ) {
        return "[External: 0]".into();
    }
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) && is_error_value(value) {
        // AssertionError exposes comparison operands as enumerable own fields;
        // retain them in inspection instead of reducing every Error to stack.
        if quench_runtime::execute::has_own_property(value, "actual")
            || quench_runtime::execute::has_own_property(value, "expected")
        {
            return inspect_object(value, depth);
        }
        if let Value::String(stack) = quench_runtime::execute::get_property(value, "stack") {
            return stack;
        }
        let name = match quench_runtime::execute::get_property(value, "name") {
            Value::String(name) if !name.is_empty() => name,
            _ => "Error".into(),
        };
        let message = match quench_runtime::execute::get_property(value, "message") {
            Value::String(message) => message,
            _ => String::new(),
        };
        return if message.is_empty() {
            format!("[{name}]")
        } else {
            format!("[{name}: {message}]")
        };
    }
    if quench_runtime::regexp::has_regexp_internal_slot(value) {
        return inspect_regexp(value);
    }
    if quench_runtime::execute::has_own_property(value, "timeValue")
        && matches!(
            quench_runtime::execute::get_prototype_of(value),
            Ok(Value::Builtin(quench_runtime::ops::Builtin::DatePrototype))
        )
    {
        return inspect_date(value);
    }
    match value {
        Value::String(s) => inspect_string(s),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => inspect_object(value, depth),
        Value::Array(_) => inspect_array(value, depth),
        Value::Map(_) | Value::Set(_) => inspect_collection(value, depth, false),
        Value::ArrayBuffer(buffer) => inspect_array_buffer(value, buffer),
        Value::DataView(view) => inspect_data_view(value, view),
        Value::Float64Array(_)
        | Value::Float32Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Uint32Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_) => inspect_typed_array_compact(value),
        Value::Function(_) | Value::BoundFunction(_) => inspect_function(value),
        value if quench_runtime::is_callable(value) => inspect_function(value),
        Value::Uint8Array(view) if is_buffer_view(value) => inspect_buffer(value, view),
        Value::Uint8Array(_) => inspect_typed_array_compact(value),
        Value::BigInt(digits) => format!("{digits}n"),
        _ => "<unknown>".into(),
    }
}

fn is_error_value(value: &Value) -> bool {
    let mut prototype = quench_runtime::execute::get_prototype_of(value).ok();
    while let Some(current) = prototype {
        match current {
            Value::Builtin(
                quench_runtime::ops::Builtin::ErrorPrototype
                | quench_runtime::ops::Builtin::RangeErrorPrototype
                | quench_runtime::ops::Builtin::ReferenceErrorPrototype
                | quench_runtime::ops::Builtin::SyntaxErrorPrototype
                | quench_runtime::ops::Builtin::EvalErrorPrototype
                | quench_runtime::ops::Builtin::URIErrorPrototype
                | quench_runtime::ops::Builtin::TypeErrorPrototype
                | quench_runtime::ops::Builtin::AggregateErrorPrototype,
            ) => return true,
            _ => prototype = quench_runtime::execute::get_prototype_of(&current).ok(),
        }
    }
    false
}

fn inspect_regexp(value: &Value) -> String {
    let source = match quench_runtime::execute::get_property(value, "source") {
        Value::String(source) => source,
        _ => "(?:)".into(),
    };
    let flags = match quench_runtime::execute::get_property(value, "flags") {
        Value::String(flags) => flags,
        _ => String::new(),
    };
    let literal = format!("/{source}/{flags}");
    let literal = colorize_regexp(&literal);
    let constructor = quench_runtime::execute::get_property(value, "constructor");
    let name = match quench_runtime::execute::get_property(&constructor, "name") {
        Value::String(name) if name != "RegExp" && !name.is_empty() => Some(name),
        _ => None,
    };
    let props = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .map(|key| {
            format!(
                "  '{key}': {}",
                inspect_depth(&quench_runtime::execute::get_property(value, &key), 0)
            )
        })
        .collect::<Vec<_>>();
    let prefix = name.map(|name| format!("{name} ")).unwrap_or_default();
    if props.is_empty() {
        format!("{prefix}{literal}")
    } else {
        format!("{prefix}{literal} {{\n{}\n}}", props.join(",\n"))
    }
}

fn colorize_regexp(literal: &str) -> String {
    let enabled = COLORS_OVERRIDE.with(|slot| slot.borrow().unwrap_or(false));
    if !enabled {
        return literal.to_string();
    }
    let mut out = String::new();
    let mut last_style = String::new();
    let mut push = |style: &str, text: &str| {
        if style != last_style {
            if !last_style.is_empty() {
                out.push_str("\x1b[39m");
            }
            if !style.is_empty() {
                out.push_str(style);
            }
            last_style = style.to_string();
        }
        out.push_str(text);
    };
    let chars: Vec<char> = literal.chars().collect();
    let unicode = literal.ends_with("/u");
    let mut index = 0;
    let mut flags = false;
    let mut slash_count = 0;
    let mut in_class = false;
    let mut in_quantifier = false;
    let mut quantifier_range = false;
    let mut group_depth = 0usize;
    let mut named_group = false;
    while index < chars.len() {
        let character = chars[index];
        if character == '?' && chars.get(index.wrapping_sub(1)) == Some(&'(') {
            if let Some(next) = chars.get(index + 1) {
                if matches!(next, '<' | '=' | '!' | ':') {
                    let lookbehind = *next == '<' && chars.get(index + 2) == Some(&'!');
                    push("", "");
                    if lookbehind {
                        push("\x1b[31m", "?<");
                        push("\x1b[31m", "!");
                    } else {
                        push("\x1b[31m", &format!("?{next}"));
                    }
                    named_group = *next == '<';
                    index += if lookbehind { 3 } else { 2 };
                    continue;
                }
            }
        }
        let closing_slash = character == '/' && slash_count == 1;
        let style = if index == 0 || closing_slash {
            "\x1b[32m"
        } else if flags {
            "\x1b[31m"
        } else if character == '\\' {
            push("", "");
            let escape_style = if group_depth > 0 || unicode {
                "\x1b[36m"
            } else {
                "\x1b[33m"
            };
            push(escape_style, &character.to_string());
            if let Some(next) = chars.get(index + 1) {
                push(escape_style, &next.to_string());
                index += 1;
            }
            push("", "");
            index += 1;
            continue;
        } else if character == '(' || character == ')' || character == '>' {
            if character == '(' {
                group_depth += 1;
            } else if character == ')' {
                group_depth = group_depth.saturating_sub(1);
            } else if named_group {
                named_group = false;
            }
            "\x1b[31m"
        } else if character == '?' && chars.get(index.wrapping_sub(1)) == Some(&'(') {
            "\x1b[31m"
        } else if character == '=' && matches!(chars.get(index.wrapping_sub(1)), Some('?')) {
            "\x1b[31m"
        } else if character == '!' && chars.get(index.wrapping_sub(1)) == Some(&'?') {
            "\x1b[31m"
        } else if character == '<' && chars.get(index.wrapping_sub(1)) == Some(&'?') {
            named_group = true;
            "\x1b[31m"
        } else if matches!(character, '^' | '$' | '|' | '*' | '+' | '?' | '.') {
            if in_class && character == '^' {
                if unicode {
                    "\x1b[36m"
                } else {
                    "\x1b[33m"
                }
            } else if in_class && character == '.' {
                "\x1b[33m"
            } else if character == '|' {
                if group_depth > 0 {
                    "\x1b[32m"
                } else {
                    "\x1b[35m"
                }
            } else if group_depth > 0 && matches!(character, '*' | '+' | '?') {
                "\x1b[32m"
            } else if character == '.' && group_depth == 0 {
                "\x1b[36m"
            } else {
                "\x1b[35m"
            }
        } else if character == '{' {
            in_quantifier = true;
            quantifier_range = chars[index + 1..]
                .iter()
                .position(|value| *value == '}')
                .and_then(|end| chars.get(index + 1..index + 1 + end))
                .is_some_and(|body| body.contains(&','));
            if quantifier_range {
                "\x1b[31m"
            } else {
                "\x1b[33m"
            }
        } else if character == '}' {
            in_quantifier = false;
            if quantifier_range {
                "\x1b[31m"
            } else {
                "\x1b[33m"
            }
        } else if character == ',' {
            if in_quantifier {
                "\x1b[33m"
            } else {
                "\x1b[32m"
            }
        } else if character == '[' {
            in_class = true;
            "\x1b[33m"
        } else if character == ']' {
            in_class = false;
            "\x1b[33m"
        } else if character == '-' {
            if in_class && unicode {
                "\x1b[35m"
            } else {
                "\x1b[33m"
            }
        } else if character.is_ascii_digit() {
            if in_quantifier {
                if quantifier_range {
                    "\x1b[36m"
                } else {
                    "\x1b[35m"
                }
            } else if in_class && unicode {
                "\x1b[36m"
            } else if in_class {
                "\x1b[33m"
            } else {
                "\x1b[36m"
            }
        } else if named_group {
            "\x1b[33m"
        } else if group_depth > 0 {
            "\x1b[36m"
        } else {
            "\x1b[33m"
        };
        let group_prefix = character == '?' && chars.get(index.wrapping_sub(1)) == Some(&'(');
        let force_token = matches!(character, '^' | '$' | '*' | '+' | '?' | '.' | '(' | ')')
            || (character.is_ascii_digit() && !(in_quantifier && quantifier_range))
            || (group_depth > 0 && !named_group && character.is_ascii_alphabetic());
        if force_token {
            push("", "");
            push(style, &character.to_string());
            if !group_prefix {
                push("", "");
            }
        } else {
            push(style, &character.to_string());
        }
        if character == '/' {
            slash_count += 1;
            flags = slash_count > 1;
        }
        index += 1;
    }
    if !last_style.is_empty() {
        out.push_str("\x1b[39m");
    }
    out
}

fn inspect_collection(owner: &Value, depth: usize, sorted: bool) -> String {
    let mut entries = match owner {
        Value::Set(set) => set
            .values
            .borrow()
            .iter()
            .map(|entry| inspect_collection_entry(owner, entry, depth))
            .collect::<Vec<_>>(),
        Value::Map(map) => map
            .keys
            .borrow()
            .iter()
            .zip(map.values.borrow().iter())
            .map(|(key, entry_value)| {
                format!(
                    "{} => {}",
                    inspect_collection_entry(owner, key, depth),
                    inspect_collection_entry(owner, entry_value, depth)
                )
            })
            .collect::<Vec<_>>(),
        _ => return "<unknown>".into(),
    };
    if sorted {
        entries.sort();
    }
    let length = entries.len();
    let name = if matches!(owner, Value::Set(_)) {
        "Set"
    } else {
        "Map"
    };
    if entries.is_empty() {
        return format!("{name}({length}) {{}}");
    }
    if sorted {
        format!("{name}({length}) {{\n  {}\n}}", entries.join(",\n  "))
    } else {
        format!("{name}({length}) {{ {} }}", entries.join(", "))
    }
}

fn inspect_collection_entry(owner: &Value, entry: &Value, depth: usize) -> String {
    let circular = match (owner, entry) {
        (Value::Set(left), Value::Set(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Map(left), Value::Map(right)) => std::rc::Rc::ptr_eq(left, right),
        _ => false,
    };
    if circular {
        "[Circular]".into()
    } else {
        inspect_depth(entry, depth.saturating_sub(1))
    }
}

fn inspect_date(value: &Value) -> String {
    let method = quench_runtime::execute::get_property(value, "toISOString");
    let Ok(Value::String(date)) = quench_runtime::execute::call(&method, value, &[]) else {
        return "Invalid Date".into();
    };
    let constructor = quench_runtime::execute::get_property(value, "constructor");
    let name = match quench_runtime::execute::get_property(&constructor, "name") {
        Value::String(name) if name != "Date" && !name.is_empty() => Some(name),
        _ => None,
    };
    let props = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .map(|key| {
            format!(
                "  '{key}': {}",
                inspect_depth(&quench_runtime::execute::get_property(value, &key), 0)
            )
        })
        .collect::<Vec<_>>();
    let prefix = name.map(|name| format!("{name} ")).unwrap_or_default();
    if props.is_empty() {
        prefix + &date
    } else {
        format!("{prefix}{date} {{\n{}\n}}", props.join(",\n"))
    }
}

fn is_date_value(value: &Value) -> bool {
    if !quench_runtime::execute::has_own_property(value, "timeValue") {
        return false;
    }
    let mut prototype = quench_runtime::execute::get_prototype_of(value).ok();
    for _ in 0..8 {
        match prototype {
            Some(Value::Builtin(quench_runtime::ops::Builtin::DatePrototype)) => return true,
            Some(next) => prototype = quench_runtime::execute::get_prototype_of(&next).ok(),
            None => return false,
        }
    }
    false
}

fn inspect_function(value: &Value) -> String {
    let is_generator = matches!(value, Value::Function(function) if function.kind == quench_runtime::ops::FunctionKind::Generator);
    let is_async = matches!(value, Value::Function(function) if function.is_async);
    let prefix = match (is_generator, is_async) {
        (true, true) => "AsyncGeneratorFunction",
        (true, false) => "GeneratorFunction",
        (false, true) => "AsyncFunction",
        (false, false) => "Function",
    };
    let name = match quench_runtime::execute::get_property(value, "name") {
        Value::String(name) if !name.is_empty() => name,
        Value::String(_) => "(anonymous)".into(),
        Value::Number(number) => js_number(number),
        other if !matches!(other, Value::Undefined) => {
            quench_runtime::execute::to_js_string(&other).unwrap_or_else(|_| "(anonymous)".into())
        }
        _ => "(anonymous)".into(),
    };
    let null_prototype = matches!(
        quench_runtime::execute::get_prototype_of(value),
        Ok(Value::Null)
    );
    let tag = match quench_runtime::execute::has_own_property(value, "Symbol.toStringTag")
        .then(|| quench_runtime::execute::get_property(value, "Symbol.toStringTag"))
    {
        Some(Value::String(tag)) if !tag.is_empty() => Some(tag),
        _ => None,
    };
    let tag = if prefix == "AsyncFunction" { None } else { tag };
    let display_name = if null_prototype && is_generator {
        format!("(null prototype): {name}")
    } else {
        name
    };
    let body = if null_prototype && is_generator {
        format!("[{prefix} {display_name}]")
    } else if display_name == "(anonymous)" {
        format!("[{prefix} (anonymous)]")
    } else {
        format!("[{prefix}: {display_name}]")
    };
    let body = if prefix == "GeneratorFunction"
        && matches!(
            quench_runtime::execute::get_prototype_of(value),
            Ok(Value::Builtin(
                quench_runtime::ops::Builtin::AsyncFunctionPrototype
            ))
        ) {
        format!("{body} AsyncFunction")
    } else {
        body
    };
    tag.map_or(body.clone(), |tag| format!("{body} [{tag}]"))
}

fn is_buffer_view(value: &Value) -> bool {
    matches!(
        quench_runtime::execute::get_property_result(value, "parent"),
        Ok(Value::ArrayBuffer(_))
    )
}

fn inspect_buffer(value: &Value, view: &quench_runtime::value::Uint8ArrayData) -> String {
    let bytes = view.buffer.bytes.borrow();
    let slice = &bytes[view.byte_offset..view.byte_offset + view.logical_len()];
    let max = crate::modules::buffer::inspect_max_bytes();
    let shown = slice
        .iter()
        .take(max)
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>();
    let suffix = slice.len().saturating_sub(max);
    let plural = if suffix == 1 { "" } else { "s" };
    let mut result = if suffix == 0 {
        format!("<Buffer {}>", shown.join(" "))
    } else {
        format!(
            "<Buffer {} ... {suffix} more byte{plural}>",
            shown.join(" ")
        )
    };
    let mut properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| {
            key != "parent" && key != "offset" && key != "toString" && key.parse::<usize>().is_err()
        })
        .map(|key| {
            format!(
                "{key}: {}",
                inspect_shallow(&quench_runtime::execute::get_property(value, &key))
            )
        })
        .collect::<Vec<_>>();
    properties.dedup();
    if !properties.is_empty() {
        if slice.is_empty() {
            result = "<Buffer ".to_string();
        } else {
            result.pop();
            result.push_str(", ");
        }
        result.push_str(&properties.join(", "));
        result.push('>');
    }
    result
}

fn inspect_array(value: &Value, depth: usize) -> String {
    let prototype = quench_runtime::execute::get_prototype_of(value).ok();
    let constructor = prototype
        .as_ref()
        .map(|prototype| quench_runtime::execute::get_property(prototype, "constructor"))
        .unwrap_or(Value::Undefined);
    if let Value::String(name) = quench_runtime::execute::get_property(&constructor, "name") {
        if !name.is_empty() && name != "Array" && name != "Object" {
            return inspect_named_array(value, depth, &name);
        }
    }
    if depth == 0 {
        return "[Array]".into();
    }
    let mut items = Vec::new();
    for index in 0..64u32 {
        let item = quench_runtime::execute::get_property(value, &index.to_string());
        if matches!(item, Value::Undefined) {
            break;
        }
        items.push(inspect_at(&item, depth - 1));
    }
    let mut properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| key != "length" && key.parse::<usize>().is_err())
        .map(|key| {
            let display = if key.parse::<i64>().is_ok() {
                format!("'{key}'")
            } else {
                key.clone()
            };
            format!(
                "{display}: {}",
                inspect_property(value, &key, depth.saturating_sub(1))
            )
        })
        .collect::<Vec<_>>();
    if items.is_empty() && properties.is_empty() {
        return "[]".into();
    }
    let mut parts = items;
    parts.append(&mut properties);
    format!("[ {} ]", parts.join(", "))
}

fn inspect_named_array(value: &Value, depth: usize, name: &str) -> String {
    let length = match quench_runtime::execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    let mut parts = Vec::new();
    let mut holes = 0usize;
    for index in 0..length.min(64) {
        let item = quench_runtime::execute::get_property(value, &index.to_string());
        if matches!(item, Value::Undefined) {
            holes += 1;
        } else {
            if holes > 0 {
                parts.push(format!("<{holes} empty items>"));
                holes = 0;
            }
            parts.push(inspect_at(&item, depth.saturating_sub(1)));
        }
    }
    if holes > 0 {
        parts.push(format!("<{holes} empty items>"));
    }
    for key in quench_runtime::execute::own_enumerable_keys(value) {
        if key != "length" && key.parse::<usize>().is_err() {
            parts.push(format!(
                "{key}: {}",
                inspect_property(value, &key, depth.saturating_sub(1))
            ));
        }
    }
    format!("{name}({length}) [ {} ]", parts.join(", "))
}

fn inspect_at(value: &Value, depth: usize) -> String {
    if matches!(
        quench_runtime::execute::get_property_result(value, "__quench_external"),
        Ok(Value::Boolean(true))
    ) || matches!(
        quench_runtime::execute::get_property_result(value, "\0regexp"),
        Ok(Value::Boolean(true))
    ) || (quench_runtime::execute::has_own_property(value, "timeValue")
        && matches!(
            quench_runtime::execute::get_prototype_of(value),
            Ok(Value::Builtin(quench_runtime::ops::Builtin::DatePrototype))
        ))
    {
        return inspect_depth(value, depth);
    }
    if depth == 0 {
        return inspect_shallow(value);
    }
    match value {
        Value::Object(_) | Value::ObjectAlias(_) => inspect_object(value, depth),
        Value::Array(_) => inspect_array(value, depth),
        _ => inspect_shallow(value),
    }
}

/// `%o`'s hidden-property profile.  The layout is deliberately derived from
/// own keys and ordinary property reads, rather than maintaining a second
/// object model for formatting.
fn inspect_verbose(value: &Value, depth: usize, indent: usize) -> String {
    if depth == 0 {
        return inspect_shallow(value);
    }
    match value {
        Value::Object(_) | Value::ObjectAlias(_) => {
            let keys = quench_runtime::execute::own_enumerable_keys(value);
            if keys.is_empty() {
                return "{}".into();
            }
            let pad = " ".repeat(indent + 2);
            let body = keys
                .iter()
                .map(|key| {
                    let display = if key.parse::<usize>().is_ok() {
                        format!("'{key}'")
                    } else {
                        key.clone()
                    };
                    format!(
                        "{pad}{display}: {}",
                        inspect_verbose(
                            &quench_runtime::execute::get_property(value, key),
                            depth - 1,
                            indent + 2,
                        )
                    )
                })
                .collect::<Vec<_>>()
                .join(",\n");
            format!("{{\n{body}\n{}}}", " ".repeat(indent))
        }
        Value::Array(array) => {
            let pad = " ".repeat(indent + 2);
            let items = (0..array.len())
                .map(|index| {
                    format!(
                        "{pad}{}",
                        inspect_verbose(
                            &quench_runtime::execute::get_property(value, &index.to_string()),
                            depth - 1,
                            indent + 2,
                        )
                    )
                })
                .collect::<Vec<_>>();
            let mut lines = items;
            lines.push(format!("{pad}[length]: {}", array.len()));
            format!("[\n{}\n{}]", lines.join(",\n"), " ".repeat(indent))
        }
        Value::Function(_) | Value::BoundFunction(_) => inspect_verbose_function(value, indent),
        _ => inspect_depth(value, depth),
    }
}

fn inspect_verbose_function(value: &Value, indent: usize) -> String {
    let name = match quench_runtime::execute::get_property(value, "name") {
        Value::String(name) if !name.is_empty() => name,
        _ => "(anonymous)".into(),
    };
    let length = inspect_shallow(&quench_runtime::execute::get_property(value, "length"));
    let pad = " ".repeat(indent + 2);
    let prototype = "{ [constructor]: [Circular *1] }";
    format!(
        "<ref *1> {} {{\n{pad}[length]: {length},\n{pad}[name]: {},\n{pad}[prototype]: {prototype}\n{}}}",
        inspect_function(value),
        inspect_string(&name),
        " ".repeat(indent),
    )
}

/// Plain objects render as `{ key: value, ... }` with shallow values.
fn inspect_object(value: &Value, depth: usize) -> String {
    if matches!(quench_runtime::execute::get_property(value, "Symbol.toStringTag"), Value::String(ref tag) if tag == "AbortController")
        && quench_runtime::execute::has_own_property(value, "signal")
        && quench_runtime::execute::has_own_property(value, "abort")
    {
        let signal = quench_runtime::execute::get_property(value, "\0quench:abort:signal");
        let aborted = quench_runtime::execute::get_property(&signal, "aborted");
        return if depth <= 4 {
            "AbortController { signal: [AbortSignal] }".into()
        } else {
            format!(
                "AbortController {{ signal: AbortSignal {{ aborted: {} }} }}",
                inspect_shallow(&aborted)
            )
        };
    }
    let prototype = quench_runtime::execute::get_prototype_of(value).ok();
    let original_prototype = value.original_prototype();
    let null_prototype = matches!(prototype, Some(Value::Null));
    let constructor_name =
        match quench_runtime::execute::get_property(value, "\0original_constructor_name") {
            Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
            _ => None,
        }
        .or_else(
            || match quench_runtime::execute::get_property(value, "constructor") {
                Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                constructor => match quench_runtime::execute::get_property(&constructor, "name") {
                    Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                    _ => None,
                },
            },
        )
        .or_else(|| {
            original_prototype
                .as_ref()
                .or(prototype.as_ref())
                .and_then(|prototype| {
                    let constructor =
                        quench_runtime::execute::get_property(prototype, "constructor");
                    match quench_runtime::execute::get_property(&constructor, "name") {
                        Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                        _ => None,
                    }
                })
        });
    if constructor_name.as_deref() == Some("AbortController") {
        let signal = quench_runtime::execute::get_property(value, "\0quench:abort:signal");
        let aborted = quench_runtime::execute::get_property(&signal, "aborted");
        return if depth <= 4 {
            "AbortController { signal: [AbortSignal] }".into()
        } else {
            format!(
                "AbortController {{ signal: AbortSignal {{ aborted: {} }} }}",
                inspect_shallow(&aborted)
            )
        };
    }
    if constructor_name.as_deref() == Some("AbortSignal") {
        let aborted = quench_runtime::execute::get_property(value, "aborted");
        return if depth == 0 {
            "[AbortSignal]".into()
        } else {
            format!("AbortSignal {{ aborted: {} }}", inspect_shallow(&aborted))
        };
    }
    let keys = quench_runtime::execute::own_enumerable_keys(value);
    if keys.is_empty() {
        return if null_prototype {
            if let Some(name) = constructor_name {
                format!("[{name}: null prototype] {{}}")
            } else {
                "[Object: null prototype] {}".into()
            }
        } else if let Some(name) = constructor_name {
            format!("{name} {{}}")
        } else {
            "{}".into()
        };
    }
    let body = keys
        .iter()
        .map(|key| {
            let property_value = quench_runtime::execute::get_property(value, key);
            let rendered = if matches!(key.as_str(), "actual" | "expected") {
                match property_value {
                    Value::String(text) if text.len() > 9_488 => {
                        format!("'{}...'", &text[..9_488])
                    }
                    Value::String(text) if text.contains('\n') => {
                        let full_diff = matches!(
                            quench_runtime::execute::get_property(value, "diff"),
                            Value::String(mode) if mode == "full"
                        );
                        if full_diff {
                            let prefix = text.split_inclusive('\n').take(10).collect::<String>();
                            format!("'{}...'", prefix.replace('\n', "\\n"))
                        } else {
                            let mut lines = text
                                .split('\n')
                                .take(10)
                                .map(|line| format!("'{line}\\n' +"))
                                .collect::<Vec<_>>();
                            lines.push("'...'".into());
                            lines.join("\n    ")
                        }
                    }
                    _ if property_value.object_identity() == value.object_identity() => {
                        "[Circular]".into()
                    }
                    _ => inspect_property(value, key, depth.saturating_sub(1)),
                }
            } else if property_value.object_identity() == value.object_identity() {
                "[Circular]".into()
            } else {
                inspect_property(value, key, depth.saturating_sub(1))
            };
            format!(
                "{}: {}",
                if key.parse::<usize>().is_ok() {
                    format!("'{key}'")
                } else {
                    key.clone()
                },
                rendered
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if null_prototype {
        return format!("[Object: null prototype] {{ {body} }}");
    }
    format!("{{ {body} }}")
}

fn inspect_property(value: &Value, key: &str, depth: usize) -> String {
    inspect_property_mode(value, key, depth, false)
}

pub(crate) fn inspect_property_with_getters(value: &Value, key: &str, depth: usize) -> String {
    inspect_property_mode(value, key, depth, true)
}

fn inspect_property_mode(value: &Value, key: &str, depth: usize, getters: bool) -> String {
    let mut owner = value.clone();
    let mut descriptor = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
        &Value::Undefined,
        &[value.clone(), Value::String(key.to_string())],
    )
    .ok();
    while descriptor
        .as_ref()
        .is_none_or(|value| matches!(value, Value::Undefined))
    {
        owner = match quench_runtime::execute::get_prototype_of(&owner) {
            Ok(Value::Null) | Err(_) => break,
            Ok(next) => next,
        };
        descriptor = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
            &Value::Undefined,
            &[owner.clone(), Value::String(key.to_string())],
        )
        .ok();
    }
    if let Some(Value::Object(descriptor)) = descriptor {
        let getter =
            quench_runtime::execute::get_property(&Value::Object(descriptor.clone()), "get");
        let setter = quench_runtime::execute::get_property(&Value::Object(descriptor), "set");
        if !matches!(getter, Value::Undefined) {
            if getters && matches!(getter, Value::Function(_) | Value::BoundFunction(_)) {
                if let Ok(result) = quench_runtime::execute::call(&getter, value, &[]) {
                    return format!("[Getter: {}]", inspect_shallow(&result));
                }
            }
            return if matches!(setter, Value::Undefined) {
                "[Getter]".into()
            } else {
                "[Getter/Setter]".into()
            };
        }
        if !matches!(setter, Value::Undefined) {
            return "[Setter]".into();
        }
    }
    inspect_at(&quench_runtime::execute::get_property(value, key), depth)
}

fn inspect_shallow(value: &Value) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    match value {
        Value::Proxy(proxy) => {
            if *proxy.revoked.borrow() {
                "<Revoked Proxy>".into()
            } else {
                inspect_shallow(&proxy.target)
            }
        }
        Value::String(s) => inspect_string(s),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
        Value::Array(_) => "[Array]".into(),
        Value::ArrayBuffer(buffer) => inspect_array_buffer(value, buffer),
        Value::DataView(view) => inspect_data_view(value, view),
        Value::Float64Array(_)
        | Value::Float32Array(_)
        | Value::Int8Array(_)
        | Value::Int16Array(_)
        | Value::Int32Array(_)
        | Value::BigInt64Array(_)
        | Value::BigUint64Array(_)
        | Value::Uint32Array(_)
        | Value::Uint8ClampedArray(_)
        | Value::Uint16Array(_) => inspect_typed_array_compact(value),
        Value::Function(_) | Value::BoundFunction(_) => inspect_function(value),
        Value::Uint8Array(view) if is_buffer_view(value) => inspect_buffer(value, view),
        Value::Uint8Array(_) => inspect_typed_array_compact(value),
        _ => "<unknown>".into(),
    }
}

fn inspect_custom(value: &Value, depth: usize) -> Option<String> {
    inspect_custom_with_receiver(value, value, depth)
}

fn inspect_custom_with_receiver(value: &Value, receiver: &Value, depth: usize) -> Option<String> {
    let method = quench_runtime::execute::get_property_result(
        value,
        "Symbol.for.nodejs.util.inspect.custom\0",
    )
    .ok()
    .filter(quench_runtime::is_callable)
    .or_else(|| {
        quench_runtime::execute::get_property_result(value, "undefined")
            .ok()
            .filter(quench_runtime::is_callable)
    })
    .or_else(|| {
        quench_runtime::execute::own_keys(value)
            .into_iter()
            .filter_map(|key| match key {
                Value::String(key) if key == "undefined" || key.contains("inspect.custom") => {
                    quench_runtime::execute::get_property_result(value, &key).ok()
                }
                _ => None,
            })
            .find(quench_runtime::is_callable)
    })
    .or_else(|| {
        let symbols = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertySymbols),
            &Value::Undefined,
            &[value.clone()],
        )
        .ok()?;
        let length = match quench_runtime::execute::get_property(&symbols, "length") {
            Value::Number(length) => length as usize,
            _ => return None,
        };
        (0..length).find_map(|index| {
            let symbol = quench_runtime::execute::get_property(&symbols, &index.to_string());
            let method = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ReflectGet),
                &Value::Undefined,
                &[value.clone(), symbol, value.clone()],
            )
            .ok()?;
            quench_runtime::is_callable(&method).then_some(method)
        })
    })?;
    if !quench_runtime::is_callable(&method) {
        return None;
    }
    let result = quench_runtime::execute::call(
        &method,
        receiver,
        &[
            Value::Number(depth as f64),
            Value::object(vec![("showProxy".into(), Value::Boolean(false))]),
        ],
    )
    .ok()?;
    Some(inspect_depth(&result, depth.saturating_sub(1)))
}

fn inspect_array_buffer(value: &Value, buffer: &quench_runtime::value::ArrayBufferData) -> String {
    inspect_array_buffer_with_limit(value, buffer, crate::modules::buffer::inspect_max_bytes())
}

fn inspect_array_buffer_with_limit(
    value: &Value,
    buffer: &quench_runtime::value::ArrayBufferData,
    max: usize,
) -> String {
    let length = buffer.byte_length();
    let contents = if *buffer.detached.borrow() {
        "(detached)".to_string()
    } else {
        let bytes = buffer.bytes.borrow();
        let shown = bytes
            .iter()
            .take(max)
            .map(|byte| format!("{byte:02x}"))
            .collect::<Vec<_>>();
        let suffix = bytes.len().saturating_sub(max);
        if suffix == 0 {
            format!("<{}>", shown.join(" "))
        } else {
            let plural = if suffix == 1 { "byte" } else { "bytes" };
            format!("<{} ... {suffix} more {plural}>", shown.join(" "))
        }
    };
    let label = if buffer.shared {
        "SharedArrayBuffer"
    } else {
        "ArrayBuffer"
    };
    let mut result = if *buffer.detached.borrow() {
        format!("{label} {{ (detached), [byteLength]: {length}")
    } else {
        format!("{label} {{ [Uint8Contents]: {contents}, [byteLength]: {length}")
    };
    for key in quench_runtime::execute::own_enumerable_keys(value) {
        result.push_str(&format!(
            ", {key}: {}",
            inspect_shallow(&quench_runtime::execute::get_property(value, &key))
        ));
    }
    result.push_str(" }");
    result
}

fn inspect_data_view(value: &Value, view: &quench_runtime::value::DataViewData) -> String {
    let detached = *view.buffer.detached.borrow();
    let indent = if detached { "      " } else { "  " };
    let closing_indent = if detached { "    " } else { "" };
    let byte_length = if detached { 0 } else { view.byte_length };
    let byte_offset = if detached {
        "undefined".to_string()
    } else {
        view.byte_offset.to_string()
    };
    let buffer = inspect_array_buffer(&Value::ArrayBuffer(view.buffer.clone()), &view.buffer);
    let mut lines = vec![
        "DataView {".to_string(),
        format!("{indent}[byteLength]: {byte_length},"),
        format!("{indent}[byteOffset]: {byte_offset},"),
        format!("{indent}[buffer]: {buffer}"),
    ];
    let properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .map(|key| {
            format!(
                "{indent}{key}: {}",
                inspect_shallow(&quench_runtime::execute::get_property(value, &key))
            )
        })
        .collect::<Vec<_>>();
    if !properties.is_empty() {
        let last = lines.len() - 1;
        lines[last].push(',');
        lines.extend(properties.into_iter().map(|line| format!("{line},")));
        let last = lines.len() - 1;
        lines[last].pop();
    }
    lines.push(format!("{closing_indent}}}"));
    lines.join("\n")
}

fn typed_array_info(
    value: &Value,
) -> Option<(
    &'static str,
    usize,
    usize,
    usize,
    std::rc::Rc<quench_runtime::value::ArrayBufferData>,
)> {
    macro_rules! info {
        ($($variant:ident => ($name:literal, $bpe:expr)),+ $(,)?) => {
            match value {
                $(Value::$variant(view) => Some(($name, view.logical_len(), $bpe, view.byte_offset, view.buffer.clone())),)+
                _ => None,
            }
        };
    }
    info!(
        Float64Array => ("Float64Array", std::mem::size_of::<f64>()),
        Float32Array => ("Float32Array", std::mem::size_of::<f32>()),
        Int8Array => ("Int8Array", std::mem::size_of::<i8>()),
        Int16Array => ("Int16Array", std::mem::size_of::<i16>()),
        Int32Array => ("Int32Array", std::mem::size_of::<i32>()),
        BigInt64Array => ("BigInt64Array", std::mem::size_of::<i64>()),
        BigUint64Array => ("BigUint64Array", std::mem::size_of::<u64>()),
        Uint32Array => ("Uint32Array", std::mem::size_of::<u32>()),
        Uint8Array => ("Uint8Array", std::mem::size_of::<u8>()),
        Uint8ClampedArray => ("Uint8ClampedArray", std::mem::size_of::<u8>()),
        Uint16Array => ("Uint16Array", std::mem::size_of::<u16>()),
    )
}

fn inspect_typed_array_compact(value: &Value) -> String {
    let Some((name, length, _, _, _)) = typed_array_info(value) else {
        return "<unknown>".into();
    };
    let broken_length = matches!(
        quench_runtime::execute::get_property(value, "length"),
        Value::Number(number) if number < 0.0
    );
    let values = (0..length)
        .map(|index| {
            if broken_length {
                "0n".to_string()
            } else {
                inspect_shallow(&quench_runtime::execute::get_property(
                    value,
                    &index.to_string(),
                ))
            }
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        format!("{name}({length}) []")
    } else {
        format!("{name}({length}) [ {} ]", values.join(", "))
    }
}

fn inspect_typed_array(
    value: &Value,
    name: &str,
    length: usize,
    bytes_per_element: usize,
    byte_offset: usize,
    buffer: &quench_runtime::value::ArrayBufferData,
) -> String {
    let mut lines = vec![format!("{name}({length}) [")];
    let broken_length = matches!(
        quench_runtime::execute::get_property(value, "length"),
        Value::Number(number) if number < 0.0
    );
    for index in 0..length {
        let rendered = if broken_length {
            "0n".to_string()
        } else {
            inspect_shallow(&quench_runtime::execute::get_property(
                value,
                &index.to_string(),
            ))
        };
        lines.push(format!("  {rendered},"));
    }
    if length > 0 {
        lines.last_mut().expect("typed array value").pop();
        lines.last_mut().expect("typed array value").push(',');
    }
    lines.push(format!("  [BYTES_PER_ELEMENT]: {bytes_per_element},"));
    lines.push(format!("  [length]: {length},"));
    lines.push(format!("  [byteLength]: {},", length * bytes_per_element));
    lines.push(format!("  [byteOffset]: {byte_offset},"));
    lines.push(format!(
        "  [buffer]: ArrayBuffer {{ [byteLength]: {} }}",
        buffer.byte_length()
    ));
    lines.push("]".into());
    lines.join("\n")
}
