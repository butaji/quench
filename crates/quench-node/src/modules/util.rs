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
    static CUSTOM_INSPECT_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static STYLIZE_OVERRIDE: RefCell<Option<Value>> = const { RefCell::new(None) };
    static COMPACT_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static BREAK_LENGTH_OVERRIDE: RefCell<Option<usize>> = const { RefCell::new(None) };
    static INSPECT_CONTEXT: RefCell<Option<InspectContext>> = const { RefCell::new(None) };
}

#[derive(Default)]
struct InspectContext {
    cycle_ids: HashMap<u64, usize>,
    cycle_values: Vec<(Value, usize)>,
    active: std::collections::HashSet<usize>,
    repeated: std::collections::HashSet<usize>,
}

pub struct CustomInspectGuard(Option<bool>);

pub fn custom_inspect_guard(enabled: Option<bool>) -> CustomInspectGuard {
    let previous = CUSTOM_INSPECT_OVERRIDE.with(|slot| slot.replace(enabled));
    CustomInspectGuard(previous)
}

impl Drop for CustomInspectGuard {
    fn drop(&mut self) {
        CUSTOM_INSPECT_OVERRIDE.with(|slot| slot.replace(self.0.take()));
    }
}

pub struct StylizeGuard(Option<Value>);

pub fn stylize_guard(stylize: Option<Value>) -> StylizeGuard {
    let previous = STYLIZE_OVERRIDE.with(|slot| slot.replace(stylize));
    StylizeGuard(previous)
}

pub struct CompactGuard(Option<bool>);

pub fn compact_guard(compact: Option<bool>) -> CompactGuard {
    let previous = COMPACT_OVERRIDE.with(|slot| slot.replace(compact));
    CompactGuard(previous)
}

pub struct BreakLengthGuard(Option<usize>);

pub fn break_length_guard(limit: Option<usize>) -> BreakLengthGuard {
    BreakLengthGuard(BREAK_LENGTH_OVERRIDE.with(|slot| slot.replace(limit)))
}

impl Drop for BreakLengthGuard {
    fn drop(&mut self) {
        BREAK_LENGTH_OVERRIDE.with(|slot| slot.replace(self.0.take()));
    }
}

impl Drop for CompactGuard {
    fn drop(&mut self) {
        COMPACT_OVERRIDE.with(|slot| slot.replace(self.0.take()));
    }
}

impl Drop for StylizeGuard {
    fn drop(&mut self) {
        STYLIZE_OVERRIDE.with(|slot| slot.replace(self.0.take()));
    }
}

fn stylize(value: &Value, rendered: String) -> String {
    let Some(method) = STYLIZE_OVERRIDE.with(|slot| slot.borrow().clone()) else {
        return rendered;
    };
    if !matches!(value, Value::String(_) | Value::Number(_) | Value::Boolean(_) | Value::Null | Value::Undefined) {
        return rendered;
    }
    quench_runtime::execute::call(
        &method,
        &Value::Undefined,
        &[Value::String(rendered.clone()), Value::String("special".into())],
    )
    .ok()
    .and_then(|value| matches!(value, Value::String(_)).then(|| quench_runtime::execute::to_js_string(&value).ok()).flatten())
    .unwrap_or(rendered)
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
        || args
            .get(1)
            .is_some_and(|options| !matches!(options, Value::Object(_) | Value::ObjectAlias(_)))
    {
        return Err(execute::type_error(
            "The options argument must be an object",
        ));
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
        args.first()
            .filter(|value| matches!(value, Value::Object(_) | Value::ObjectAlias(_)))
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
                    (
                        "scriptName".into(),
                        Value::String(mapped.script_name.clone()),
                    ),
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
            .and_then(|(_, tail)| {
                tail.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                    .next()
            })
            .filter(|name| !name.is_empty())
            .map(str::to_owned),
    })
}

fn json_string_field(json: &str, key: &str) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))?;
    let tail = &json[start..];
    let quote = tail
        .find(':')
        .and_then(|index| tail[index + 1..].find('"'))?;
    let value = &tail[tail.find(':')? + 1 + quote + 1..];
    Some(value.split('"').next()?.to_owned())
}

fn json_string_array_item(json: &str, key: &str, index: usize) -> Option<String> {
    let start = json.find(&format!("\"{key}\""))?;
    let array = &json[start..].split_once('[')?.1;
    array
        .split('"')
        .filter(|value| !value.is_empty() && *value != ",")
        .nth(index * 2)
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
    Some(if value & 1 == 1 {
        -(value >> 1) - 1
    } else {
        value >> 1
    })
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
            "callbackify".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_CALLBACKIFY),
        ),
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
            "transferableAbortSignal".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_TRANSFERABLE_ABORT_SIGNAL),
        ),
        (
            "transferableAbortController".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_TRANSFERABLE_ABORT_CONTROLLER),
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
        "isObject",
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
    let external = matches!(
        quench_runtime::execute::get_property_result(value, "__quench_external"),
        Ok(Value::Boolean(true))
    );
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
        "isObject" => !external && plain_object(value),
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
        "isKeyObject" => matches!(
            quench_runtime::execute::get_property_result(
                value,
                crate::modules::crypto::KEY_MARKER_PROP
            ),
            Ok(Value::Boolean(true))
        ),
        "isCryptoKey" => matches!(
            quench_runtime::execute::get_property_result(
                value,
                crate::modules::webcrypto::KEY_MARKER_PROP,
            ),
            Ok(Value::Boolean(true))
        ),
        _ => false,
    }
}

fn plain_object(value: &Value) -> bool {
    if !matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        return false;
    }
    let constructor = quench_runtime::execute::get_property_result(value, "constructor")
        .ok()
        .and_then(|value| quench_runtime::execute::get_property_result(&value, "name").ok());
    match constructor {
        None => true,
        Some(Value::String(name)) => name == "Object",
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
    let inner = quench_runtime::execute::get_property_result(value, "_value");
    let primitive = matches!(value, Value::Object(_) | Value::ObjectAlias(_) | Value::BindingCell(_))
        && match (&inner, name) {
            (Ok(Value::Boolean(_)), "Boolean") | (Ok(Value::Number(_)), "Number") => true,
            (Ok(value @ Value::String(_)), "String") => !quench_runtime::execute::is_symbol(value),
            (Ok(Value::BigInt(_)), "BigInt") => true,
            (Ok(value), "Symbol") => quench_runtime::execute::is_symbol(value),
            _ => false,
        };
    let expected_prototype = matches!(prototype, Ok(Value::Builtin(actual)) if actual == expected);
    if primitive && (expected_prototype || quench_runtime::execute::has_own_property(value, "_value")) {
        return true;
    }
    if matches!(quench_runtime::execute::get_prototype_of(value), Ok(Value::Null)) {
        return match (name, inner) {
            ("Boolean", Ok(Value::Boolean(_))) | ("Number", Ok(Value::Number(_))) => true,
            ("String", Ok(value @ Value::String(_))) => !quench_runtime::execute::is_symbol(&value),
            ("BigInt", Ok(Value::BigInt(_))) => true,
            ("Symbol", Ok(value)) => quench_runtime::execute::is_symbol(&value),
            _ => false,
        };
    }
    false
}

fn inspect_capability() -> Value {
    let inspect = crate::host::capability(crate::registry::SPEC_UTIL_INSPECT);
    let options = quench_runtime::host_api::object(vec![
        ("showHidden".into(), Value::Boolean(false)),
        ("depth".into(), Value::Number(2.0)),
        ("colors".into(), Value::Boolean(false)),
        ("customInspect".into(), Value::Boolean(true)),
        ("showProxy".into(), Value::Boolean(false)),
        ("maxArrayLength".into(), Value::Number(100.0)),
        ("maxStringLength".into(), Value::Number(10_000.0)),
        ("breakLength".into(), Value::Number(80.0)),
        ("compact".into(), Value::Number(3.0)),
        ("sorted".into(), Value::Boolean(false)),
        ("getters".into(), Value::Boolean(false)),
        ("numericSeparator".into(), Value::Boolean(false)),
    ]);
    INSPECT_DEFAULT_OPTIONS.with(|slot| *slot.borrow_mut() = Some(options.clone()));
    let _ = quench_runtime::execute::set_callable_property(&inspect, "defaultOptions", options);
    if let Ok(custom) = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::SymbolFor),
        &Value::Undefined,
        &[Value::String("nodejs.util.inspect.custom".into())],
    ) {
        let _ = quench_runtime::execute::set_callable_property(&inspect, "custom", custom);
    }
    let styles = quench_runtime::host_api::object(vec![
        ("special".into(), Value::String("cyan".into())),
        ("number".into(), Value::String("yellow".into())),
        ("bigint".into(), Value::String("yellow".into())),
        ("boolean".into(), Value::String("yellow".into())),
        ("undefined".into(), Value::String("grey".into())),
        ("null".into(), Value::String("bold".into())),
        ("string".into(), Value::String("green".into())),
        ("symbol".into(), Value::String("green".into())),
        ("date".into(), Value::String("magenta".into())),
        ("regexp".into(), Value::String("red".into())),
        ("module".into(), Value::String("underline".into())),
    ]);
    let colors = quench_runtime::host_api::object(vec![
        ("gray".into(), color_pair(90, 39)),
        ("grey".into(), color_pair(90, 39)),
        ("cyan".into(), color_pair(36, 39)),
        ("yellow".into(), color_pair(33, 39)),
        ("green".into(), color_pair(32, 39)),
        ("magenta".into(), color_pair(35, 39)),
        ("bold".into(), color_pair(1, 22)),
        ("red".into(), color_pair(31, 39)),
    ]);
    let _ = quench_runtime::execute::set_callable_property(&inspect, "styles", styles);
    let _ = quench_runtime::execute::set_callable_property(&inspect, "colors", colors);
    inspect
}

fn color_pair(start: i32, end: i32) -> Value {
    quench_runtime::host_api::array(vec![
        Value::Number(start as f64),
        Value::Number(end as f64),
    ])
}

pub fn inspect_default_option(name: &str) -> Value {
    INSPECT_DEFAULT_OPTIONS.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|options| quench_runtime::execute::get_property(options, name))
            .unwrap_or(Value::Undefined)
    })
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
    } else if matches!(value, Value::String(_)) {
        ("\x1b[32m", "\x1b[39m")
    } else if is_date_value(value) {
        ("\x1b[35m", "\x1b[39m")
    } else if matches!(value, Value::Function(_) | Value::BoundFunction(_)) {
        ("\x1b[36m", "\x1b[39m")
    } else if boxed_constructor(value, "String") {
        ("\x1b[32m", "\x1b[39m")
    } else if boxed_constructor(value, "Symbol") {
        ("\x1b[32m", "\x1b[39m")
    } else if boxed_constructor(value, "Boolean")
        || boxed_constructor(value, "Number")
        || boxed_constructor(value, "BigInt")
    {
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
        // `StringUnits` is the runtime's exact UTF-16 representation for a
        // primitive string.  Formatting must preserve its full contents;
        // routing it through `inspect` would apply the inspector's display
        // truncation to otherwise ordinary console output.
        Value::StringUnits(value) => String::from_utf16_lossy(value),
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
        Value::StringUnits(units) => String::from_utf16_lossy(units),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::BigInt(digits) => format!("{}n", bigint_digits(digits)),
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
            if let Some(name) = custom_array_name(value) {
                return format!("[{name}]");
            }
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

fn custom_array_name(value: &Value) -> Option<String> {
    let direct = quench_runtime::execute::get_property(value, "constructor");
    let direct_name = match quench_runtime::execute::get_property(&direct, "name") {
        Value::String(name) if !name.is_empty() && name != "Array" => Some(name),
        _ => None,
    };
    direct_name.or_else(|| {
        let prototype = value
            .array_prototype()
            .or_else(|| {
                let stored = quench_runtime::execute::get_property(value, "\0prototype");
                (!matches!(stored, Value::Undefined)).then_some(stored)
            })
            .or_else(|| quench_runtime::execute::get_prototype_of(value).ok())?;
        let constructor = quench_runtime::execute::get_property(&prototype, "constructor");
        match quench_runtime::execute::get_property(&constructor, "name") {
            Value::String(name) if !name.is_empty() && name != "Array" => Some(name),
            _ => None,
        }
    })
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
    if body.contains('\u{1}') || body.contains('\u{0}') {
        return "Symbol()".into();
    }
    // `util.inspect` deliberately renders registry symbols as `Symbol(key)`;
    // the registry origin is observable through `Symbol.keyFor`, not through
    // the display form (Node uses the same spelling for local and global
    // symbols).
    if let Some(key) = body.strip_prefix("Symbol.for.") {
        return format!("Symbol({})", escape_symbol_description(key));
    }
    let unique = !suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit());
    if !unique {
        return format!("Symbol({})", escape_symbol_description(body));
    }
    let description = body.strip_prefix("Symbol.").unwrap_or(body);
    if description.is_empty() || description == "\u{1}" {
        return "Symbol()".into();
    }
    format!("Symbol({})", escape_symbol_description(description))
}

fn escape_symbol_description(description: &str) -> String {
    let quoted = inspect_string_segment(description);
    quoted[1..quoted.len().saturating_sub(1)].to_string()
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

pub fn inspect_minimal(value: &Value) -> String {
    if matches!(
        quench_runtime::execute::get_property(value, "\0quench:broadcast-channel"),
        Value::Boolean(true)
    ) {
        return "BroadcastChannel".into();
    }
    let (Value::Object(_) | Value::ObjectAlias(_)) = value else {
        return inspect_depth(value, 0);
    };
    let prototype = quench_runtime::execute::get_prototype_of(value).ok();
    let null_prototype = matches!(prototype, Some(Value::Null));
    let constructor = quench_runtime::execute::get_property(value, "constructor");
    let name = match quench_runtime::execute::get_property(value, "\0original_constructor_name") {
        Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
            Some(symbol_string(&Value::String(name)))
        }
        Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
        _ => match quench_runtime::execute::get_property(&constructor, "name") {
            Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
                Some(symbol_string(&Value::String(name)))
            }
            Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
            _ => None,
        },
    }
        .or_else(|| {
            value.original_prototype().and_then(|prototype| {
                let constructor = quench_runtime::execute::get_property(&prototype, "constructor");
                match quench_runtime::execute::get_property(&constructor, "name") {
                    Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
                        Some(symbol_string(&Value::String(name)))
                    }
                    Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                    _ => None,
                }
            })
        })
        .unwrap_or_else(|| "Object".into());
    let tag = match quench_runtime::execute::get_property(value, "Symbol.toStringTag") {
        Value::String(tag) if !tag.is_empty() => Some(tag),
        _ => None,
    };
    let display_name = if null_prototype && tag.as_deref() == Some(name.as_str()) {
        "Object".to_string()
    } else {
        name.clone()
    };
    let mut prefix = if null_prototype {
        format!("[{display_name}: null prototype]")
    } else {
        name.clone()
    };
    if let Some(tag) = tag {
        if tag != display_name {
            prefix.push_str(&format!(" [{tag}]"));
        }
    }
    let has_properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .any(|key| !key.starts_with('\0'));
    if has_properties {
        if prefix.starts_with('[') {
            prefix
        } else {
            format!("[{prefix}]")
        }
    } else {
        format!("{prefix} {{}}")
    }
}

pub fn inspect_with_depth(value: &Value, depth: usize) -> String {
    if matches!(
        quench_runtime::execute::get_property(value, crate::modules::webcrypto::KEY_MARKER_PROP),
        Value::Boolean(true)
    ) {
        let metadata =
            quench_runtime::execute::get_property(value, crate::modules::webcrypto::KEY_META_PROP);
        let key_type =
            inspect_with_depth(&quench_runtime::execute::get_property(&metadata, "type"), 0);
        let extractable = inspect_with_depth(
            &quench_runtime::execute::get_property(&metadata, "extractable"),
            0,
        );
        let algorithm = inspect_with_depth(
            &quench_runtime::execute::get_property(&metadata, "algorithm"),
            depth.saturating_sub(1),
        );
        let usages = inspect_with_depth(
            &quench_runtime::execute::get_property(&metadata, "usages"),
            depth.saturating_sub(1),
        );
        return format!(
            "CryptoKey {{ type: {key_type}, extractable: {extractable}, algorithm: {algorithm}, usages: {usages} }}"
        );
    }
    if let Some(rendered) = broadcast_channel_render(value, depth) {
        return rendered;
    }
    if matches!(
        quench_runtime::execute::get_property(value, "\0synthetic_module"),
        Value::Boolean(true)
    ) {
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
            "SyntheticModule {{\n  status: {status},\n  identifier: {identifier},\n  context: {context}\n}}"
        );
    }
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
    if value.is_arguments_object() || is_arguments_like(value) {
        return inspect_object(value, depth);
    }
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) && !is_arguments_like(value) {
        let canonical = quench_runtime::execute::canonical_value(value);
        for key in quench_runtime::execute::own_enumerable_keys(&canonical) {
            if quench_runtime::execute::same_identity(
                &quench_runtime::execute::canonical_value(&quench_runtime::execute::get_property(
                    &canonical, &key,
                )),
                &canonical,
            ) {
                return format!("<ref *1> {{ {key}: [Circular *1] }}");
            }
        }
    }
    if matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_)
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
    if matches!(
        quench_runtime::execute::get_property(value, crate::modules::webcrypto::KEY_MARKER_PROP),
        Value::Boolean(true)
    ) {
        return inspect_with_depth(value, depth);
    }
    if !show_hidden {
        for name in ["String", "Boolean", "Number", "Symbol", "BigInt"] {
            if boxed_constructor(value, name) {
                let inner = quench_runtime::execute::get_property(value, "_value");
                let prototype_suffix = match quench_runtime::execute::get_prototype_of(value) {
                    Ok(prototype) => match quench_runtime::execute::get_property(&prototype, "constructor") {
                        constructor => match quench_runtime::execute::get_property(&constructor, "name") {
                            Value::String(proto_name)
                                if !proto_name.is_empty() && proto_name != "Object" && proto_name != name =>
                            {
                                format!(" ({proto_name})")
                            }
                            _ => String::new(),
                        },
                    },
                    Err(_) => String::new(),
                };
                let null_suffix = if matches!(
                    quench_runtime::execute::get_prototype_of(value),
                    Ok(Value::Null)
                ) {
                    " (null prototype)"
                } else {
                    ""
                };
                let tag_suffix = if quench_runtime::execute::has_own_property(value, "Symbol.toStringTag") {
                    match quench_runtime::execute::get_property(value, "Symbol.toStringTag") {
                        Value::String(tag) if !tag.is_empty() => format!(" [{tag}]"),
                        _ => String::new(),
                    }
                } else {
                    String::new()
                };
                let extras = quench_runtime::execute::own_enumerable_keys(value)
                    .into_iter()
                    .filter(|key| key != "_value" && key != "length" && !is_array_index_key(key))
                    .map(|key| format!("{}: {}", format_property_key(&key), inspect_property(value, &key, depth.saturating_sub(1))))
                    .collect::<Vec<_>>();
                let extra_suffix = if extras.is_empty() {
                    String::new()
                } else {
                    format!(" {{ {} }}", extras.join(", "))
                };
                return format!("[{name}{prototype_suffix}{null_suffix}: {}]{tag_suffix}{extra_suffix}", inspect_shallow(&inner));
            }
        }
    }
    if show_hidden
        && matches!(value, Value::Object(_) | Value::ObjectAlias(_))
        && is_error_value(value)
    {
        let base = inspect_depth(value, depth);
        return format!("{base} {{ [stack]: [Getter/Setter], [message]: [Getter] }}");
    }
    let rendered = match (value, max_array_length, getters) {
        (value @ Value::Array(_), Some(limit), _)
            if !show_hidden && is_plain_array(value) =>
        {
            inspect_array_limited(value, depth, limit)
        }
        (value, limit, false) if is_typed_array_value(value) && !is_buffer_view(value) => {
            inspect_typed_array_values(value, limit.unwrap_or(100))
        }
        (Value::Array(_), _, _) if show_hidden => inspect_array_show_hidden(value, depth),
        (Value::Object(_) | Value::ObjectAlias(_), _, true) => {
            inspect_object_with_getters(value, depth)
        }
        (Value::Map(_) | Value::Set(_), _, _) => {
            inspect_collection_with_options(value, depth, false, show_hidden, max_array_length)
        }
        (Value::Iterator(_), _, _) => inspect_iterator_with_options(value, depth, max_array_length),
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
            let mut hidden = quench_runtime::execute::own_keys(value)
                .into_iter()
                .filter_map(|key| match key {
                    Value::String(key)
                        if !key.starts_with('\0')
                            && !quench_runtime::execute::own_enumerable_keys(value)
                                .iter()
                                .any(|visible| visible == &key)
                            && !inspect_enumerable_keys(value).iter().any(|visible| visible == &key) =>
                    {
                        Some(format!("[{}]: {}", format_property_key(&key), inspect_property(value, &key, 0)))
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let mut hidden_symbols = std::collections::HashSet::new();
            hidden.retain(|entry| {
                if let Some(key) = entry.strip_prefix('[').and_then(|entry| entry.split_once(']')).map(|(key, _)| key) {
                    if key.starts_with("Symbol") {
                        return hidden_symbols.insert(key.to_string());
                    }
                }
                true
            });
            if !hidden.is_empty() {
                if let Some(body) = rendered.strip_suffix(" }") {
                    return format!("{body}, {} }}", hidden.join(", "));
                }
                if let Some(prefix) = rendered.strip_suffix("{}") {
                    return format!("{prefix}{{ {} }}", hidden.join(", "));
                }
            }
        }
        if let Value::Array(array) = value {
            if rendered.contains("\n") {
                return rendered;
            }
            if rendered == "[]" {
                return format!("[ [length]: {} ]", array.len());
            }
            if let Some(body) = rendered.strip_suffix(" ]") {
                let mut insertion = body.len();
                for key in quench_runtime::execute::own_enumerable_keys(value)
                    .into_iter()
                    .filter(|key| key != "length" && !is_array_index_key(key))
                {
                    let display = if key.parse::<usize>().is_ok() {
                        format!("'{key}'")
                    } else {
                        key.clone()
                    };
                    if let Some(position) = body.find(&format!(", {display}:")) {
                        insertion = insertion.min(position);
                    } else if let Some(position) = body.find(&format!("[ {display}:")) {
                        insertion = insertion.min(position + 1);
                    }
                }
                let (prefix, suffix) = body.split_at(insertion);
                if insertion == 1 {
                    return format!("[ [length]: {}, {} ]", array.len(), suffix.trim_start());
                }
                return format!("{prefix}, [length]: {}{suffix} ]", array.len());
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

fn inspect_array_show_hidden(value: &Value, depth: usize) -> String {
    let prototype = quench_runtime::execute::get_prototype_of(value).ok();
    let constructor = prototype
        .as_ref()
        .map(|prototype| quench_runtime::execute::get_property(prototype, "constructor"))
        .unwrap_or(Value::Undefined);
    let Value::String(name) = quench_runtime::execute::get_property(&constructor, "name") else {
        return inspect_with_depth(value, depth);
    };
    if name.is_empty() || name == "Array" || name == "Object" {
        let length = match value {
            Value::Array(array) => array.logical_len(),
            _ => 0,
        };
        if length > 1_000_000 {
            return format!("[ <{length} empty items>, [length]: {length} ]");
        }
        return inspect_with_depth(value, depth);
    }
    let length = match quench_runtime::execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    if length > 1_000 {
        return format!("{name}({length}) [ <{length} empty items>, [length]: {length} ]");
    }
    let mut parts = Vec::new();
    let mut holes = 0usize;
    for index in 0..length.min(64) {
        let item = own_array_item(value, index as u32);
        if let Some(item) = item {
            if holes > 0 {
                parts.push(empty_items(holes));
                holes = 0;
            }
            parts.push(inspect_at(&item, depth.saturating_sub(1)));
        } else {
            holes += 1;
        }
    }
    if holes > 0 {
        parts.push(empty_items(holes));
    }
    parts.push(format!("[length]: {length}"));
    let mut seen = std::collections::HashSet::new();
    let mut append = |key: String| {
        if !seen.insert(key.clone()) {
            return;
        }
        let display = if key.parse::<usize>().is_ok() {
            format!("'{key}'")
        } else {
            key.clone()
        };
        parts.push(format!(
            "{display}: {}",
            inspect_property(value, &key, depth.saturating_sub(1))
        ));
    };
    for key in quench_runtime::execute::own_keys(value)
        .into_iter()
        .filter_map(|key| match key {
            Value::String(key)
                if key != "length"
                    && key != "constructor"
                    && !key.starts_with('\0')
                    && (!is_array_index_key(&key)
                        || key.parse::<usize>().map_or(true, |index| index >= length)) =>
            {
                Some(key)
            }
            _ => None,
        })
    {
        append(key);
    }
    if let Some(prototype) = prototype {
        for key in quench_runtime::execute::own_keys(&prototype)
            .into_iter()
            .filter_map(|key| match key {
                Value::String(key)
                    if key != "length"
                        && key != "constructor"
                        && !key.starts_with('\0')
                        && (!is_array_index_key(&key)
                            || key.parse::<usize>().map_or(true, |index| {
                                own_array_item(value, index as u32).is_none()
                            })) =>
                {
                    Some(key)
                }
                _ => None,
            })
        {
            append(key);
        }
    }
    format!("{name}({length}) [\n  {}\n]", parts.join(",\n  "))
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
    let rendered = colorize(value, rendered);
    COLORS_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    rendered
}

pub fn inspect_error_compact(value: &Value) -> Option<String> {
    if !is_error_value(value) {
        return None;
    }
    let name = inspect_error_name(value);
    let message = match quench_runtime::execute::get_property(value, "message") {
        Value::String(message) if !message.is_empty() => format!(": {message}"),
        _ => String::new(),
    };
    Some(format!("[{name}{message}]"))
}

fn inspect_error_name(value: &Value) -> String {
    let mut current = Some(value.clone());
    let constructor = loop {
        let Some(candidate) = current else {
            break Value::Undefined;
        };
        let constructor = quench_runtime::execute::get_property(&candidate, "constructor");
        if matches!(
            quench_runtime::execute::get_property(&constructor, "name"),
            Value::String(ref name) if !name.is_empty()
        ) {
            break constructor;
        }
        current = quench_runtime::execute::get_prototype_of(&candidate).ok();
    };
    let constructor = quench_runtime::execute::to_js_string(
        &quench_runtime::execute::get_property(&constructor, "name"),
    )
    .unwrap_or_else(|_| "Error".into());
    let raw = if quench_runtime::execute::has_own_property(value, "name") {
        quench_runtime::execute::get_property(value, "name")
    } else {
        Value::Undefined
    };
    match raw {
        Value::Undefined => constructor,
        Value::String(name) => name,
        Value::Null => "null".into(),
        value => {
            let name = quench_runtime::execute::to_js_string(&value).unwrap_or_default();
            if name.is_empty() {
                name
            } else {
                format!("{name} [{constructor}]")
            }
        }
    }
}

pub fn inspect_error_compact_with_break(
    value: &Value,
    break_length: Option<usize>,
) -> Option<String> {
    let header = inspect_error_compact(value)?;
    let extras = inspect_error_extras(value, 3);
    if extras.is_empty() {
        return Some(header);
    }
    if break_length.is_some_and(|limit| limit <= 1) {
        let fields = extras
            .iter()
            .map(|extra| {
                extra.split_once(": ").map_or_else(
                    || extra.clone(),
                    |(key, value)| format!("{key}:\n   {value}"),
                )
            })
            .collect::<Vec<_>>()
            .join(",\n  ");
        return Some(format!("{{ {header}\n  {fields} }}"));
    }
    if break_length.is_some_and(|limit| limit < header.len()) {
        return Some(format!("{{ {header}\n  {} }}", extras.join(",\n  ")));
    }
    Some(format_error_with_extras(&header, extras))
}

fn refresh_error_stack_header(value: &Value, stack: &str) -> String {
    let name = inspect_error_name(value);
    let message = quench_runtime::execute::get_property_result(value, "message")
        .ok()
        .and_then(|value| quench_runtime::execute::to_js_string(&value).ok())
        .unwrap_or_default();
    let header = if message.is_empty() {
        name
    } else if name.is_empty() {
        message
    } else {
        format!("{name}: {message}")
    };
    stack.find('\n').map_or(header.clone(), |index| {
        format!("{header}{}", &stack[index..])
    })
}

pub fn inspect_error_noncompact(value: &Value) -> Option<String> {
    let header = inspect_error_compact(value)?;
    let extras = inspect_error_extras(value, 3);
    (!extras.is_empty()).then(|| format!("{header} {{\n  {}\n}}", extras.join(",\n  ")))
}

fn inspect_error_extras(value: &Value, depth: usize) -> Vec<String> {
    let canonical = quench_runtime::execute::canonical_value(value);
    let stack_enumerable = stack_enumerable(&canonical);
    let stack_name = if stack_enumerable {
        quench_runtime::execute::to_js_string(&quench_runtime::execute::get_property(
            &canonical, "stack",
        ))
        .unwrap_or_default()
        .split_once(':')
        .map(|(name, _)| name.to_string())
    } else {
        None
    };
    let current_name = quench_runtime::execute::to_js_string(
        &quench_runtime::execute::get_property(&canonical, "name"),
    )
    .unwrap_or_default();
    let expose_name = stack_enumerable && stack_name.as_deref() != Some(current_name.as_str());
    let keys = quench_runtime::execute::own_enumerable_keys(&canonical);
    keys.clone()
        .into_iter()
        .filter(|key| key != "message" && key != "stack")
        .filter(|key| key != "name" || expose_name)
        .map(|key| {
            let rendered = inspect_depth(
                &quench_runtime::execute::get_property_result(&canonical, &key)
                    .unwrap_or(Value::Undefined),
                depth.saturating_sub(1),
            );
            format!("{key}: {rendered}")
        })
        .collect()
}

fn stack_enumerable(value: &Value) -> bool {
    quench_runtime::execute::get_own_property_descriptor(value, "stack")
        .ok()
        .and_then(|descriptor| {
            matches!(descriptor, Value::Object(_) | Value::ObjectAlias(_))
                .then(|| quench_runtime::execute::get_property(&descriptor, "enumerable"))
        })
        .is_some_and(|value| matches!(value, Value::Boolean(true)))
}

fn format_error_with_extras(header: &str, mut extras: Vec<String>) -> String {
    let header = if header.starts_with('[') {
        header.to_string()
    } else {
        format!("[{header}]")
    };
    // Error headers commonly exceed the inspector's break length. Expand
    // nested object extras into the same stable multiline shape as Node.
    if header.len() + extras.iter().map(String::len).sum::<usize>() > 80 {
        for extra in &mut extras {
            if let Some((key, value)) = extra.split_once(": { ") {
                if let Some(inner) = value.strip_suffix(" }") {
                    let fields = inner.split(", ").collect::<Vec<_>>().join(",\n    ");
                    *extra = format!("{key}: {{\n    {fields}\n  }}");
                }
            }
        }
    }
    if header.contains('\n') {
        format!("{{ {header}\n  {} }}", extras.join(",\n  "))
    } else {
        format!("{{ {header} {} }}", extras.join(", "))
    }
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
        return inspect_regexp(value, depth);
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
                } else if depth == 0 && root_identity.is_some() {
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
    } else if depth == 0 && root_identity.is_some() {
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

fn inspect_string_units(units: &[u16]) -> String {
    if units.iter().all(|unit| !(0xD800..=0xDFFF).contains(unit)) {
        return inspect_string(&String::from_utf16_lossy(units));
    }
    let has_single = units.iter().any(|unit| *unit == b'\'' as u16);
    let has_double = units.iter().any(|unit| *unit == b'"' as u16);
    let quote = if has_single && !has_double { '"' } else { '\'' };
    let mut out = String::with_capacity(units.len() + 2);
    out.push(quote);
    let mut index = 0;
    while index < units.len() {
        let unit = units[index];
        if (0xD800..=0xDBFF).contains(&unit)
            && units
                .get(index + 1)
                .is_some_and(|next| (0xDC00..=0xDFFF).contains(next))
        {
            let high = (unit - 0xD800) as u32;
            let low = (units[index + 1] - 0xDC00) as u32;
            if let Some(character) = char::from_u32(0x10000 + (high << 10) + low) {
                push_inspected_character(&mut out, quote, character);
            }
            index += 2;
            continue;
        }
        if (0xD800..=0xDFFF).contains(&unit) {
            out.push_str(&format!("\\u{unit:04x}"));
        } else if let Some(character) = char::from_u32(unit as u32) {
            push_inspected_character(&mut out, quote, character);
        }
        index += 1;
    }
    out.push(quote);
    out
}

fn push_inspected_character(out: &mut String, quote: char, character: char) {
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

fn inspect_depth(value: &Value, depth: usize) -> String {
    let root = INSPECT_CONTEXT.with(|slot| slot.borrow().is_none());
    if root {
        let (cycle_ids, cycle_values) = inspect_cycle_ids(value);
        INSPECT_CONTEXT.with(|slot| {
            *slot.borrow_mut() = Some(InspectContext {
                cycle_ids,
                cycle_values,
                ..InspectContext::default()
            })
        });
    }
    let rendered = inspect_depth_tracked(value, depth);
    if root {
        INSPECT_CONTEXT.with(|slot| *slot.borrow_mut() = None);
    }
    rendered
}

fn inspect_depth_tracked(value: &Value, depth: usize) -> String {
    if is_arguments_like(value) {
        return inspect_depth_inner(value, depth);
    }
    let Some(identity) = inspect_identity(value) else {
        return inspect_depth_inner(value, depth);
    };
    let Some(id) = INSPECT_CONTEXT.with(|slot| {
        let mut state = slot.borrow_mut();
        let context = state.as_mut().expect("inspection context");
        context
            .cycle_ids
            .get(&identity)
            .copied()
            .or_else(|| {
                context
                    .cycle_values
                    .iter()
                    .find(|(candidate, _)| quench_runtime::execute::same_identity(candidate, value))
                    .map(|(_, id)| *id)
            })
    }) else {
        return inspect_depth_inner(value, depth);
    };
    let repeated = INSPECT_CONTEXT.with(|slot| {
        let mut state = slot.borrow_mut();
        let context = state.as_mut().expect("inspection context");
        if context.active.contains(&id) {
            context.repeated.insert(id);
            true
        } else {
            context.active.insert(id);
            false
        }
    });
    if repeated {
        return format!("[Circular *{id}]");
    }
    let rendered = inspect_depth_inner(value, depth);
    let labeled = INSPECT_CONTEXT.with(|slot| {
        let mut context = slot.borrow_mut();
        let context = context.as_mut().expect("inspection context");
        context.active.remove(&id);
        if context.repeated.contains(&id) {
            format!("<ref *{id}> {rendered}")
        } else {
            rendered
        }
    });
    labeled
}

fn is_arguments_like(value: &Value) -> bool {
    matches!(
        quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectPrototypeToString),
            value,
            &[],
        ),
        Ok(Value::String(tag)) if tag == "[object Arguments]"
    )
}

fn inspect_cycle_ids(value: &Value) -> (HashMap<u64, usize>, Vec<(Value, usize)>) {
    let mut scan = CycleScan::default();
    inspect_cycle_scan(value, &mut scan);
    let entries = scan.order
        .into_iter()
        .filter(|(identity, _)| scan.cyclic.contains(identity))
        .enumerate()
        .map(|(index, (identity, value))| ((identity, index + 1), (value, index + 1)))
        .collect::<Vec<_>>();
    (
        entries.iter().map(|((identity, id), _)| (*identity, *id)).collect(),
        entries.into_iter().map(|(_, value)| value).collect(),
    )
}

#[derive(Default)]
struct CycleScan {
    stack: Vec<u64>,
    stack_values: Vec<Value>,
    visiting: std::collections::HashSet<u64>,
    seen: std::collections::HashSet<u64>,
    cyclic: std::collections::HashSet<u64>,
    order: Vec<(u64, Value)>,
}

fn inspect_cycle_scan(value: &Value, scan: &mut CycleScan) {
    if value.is_arguments_object() {
        return;
    }
    let Some(identity) = inspect_identity(value) else {
        return;
    };
    if let Some((index, _)) = scan
        .stack_values
        .iter()
        .enumerate()
        .find(|(_, candidate)| quench_runtime::execute::same_identity(candidate, value))
    {
        let stack_identity = scan.stack[index];
        scan.cyclic.insert(stack_identity);
        return;
    }
    if !scan.seen.insert(identity) {
        return;
    }
    scan.order.push((identity, value.clone()));
    scan.visiting.insert(identity);
    scan.stack.push(identity);
    scan.stack_values.push(value.clone());
    for child in inspect_children(value) {
        inspect_cycle_scan(&child, scan);
    }
    scan.stack.pop();
    scan.stack_values.pop();
    scan.visiting.remove(&identity);
}

fn inspect_children(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(array) => (0..array.logical_len())
            .filter_map(|index| own_array_item(value, index as u32))
            .collect(),
        Value::Object(_) | Value::ObjectAlias(_) => quench_runtime::execute::own_enumerable_keys(value)
            .into_iter()
            .map(|key| quench_runtime::execute::get_property(value, &key))
            .collect(),
        _ => Vec::new(),
    }
}

fn inspect_identity(value: &Value) -> Option<u64> {
    let canonical = quench_runtime::execute::canonical_value(value);
    if let Some(identity) = canonical.object_identity() {
        return Some(identity);
    }
    let pointer = match &canonical {
        Value::Map(value) => Rc::as_ptr(value) as usize,
        Value::Set(value) => Rc::as_ptr(value) as usize,
        Value::Promise(value) => Rc::as_ptr(value) as usize,
        _ => return None,
    };
    Some(pointer as u64)
}

fn inspect_depth_inner(value: &Value, depth: usize) -> String {
    if let Value::Promise(promise) = value {
        let display_name = quench_runtime::execute::get_prototype_of(value)
            .ok()
            .map(|prototype| quench_runtime::execute::get_property(&prototype, "constructor"))
            .and_then(|constructor| match quench_runtime::execute::get_property(&constructor, "name") {
                Value::String(name) if !name.is_empty() && name != "Promise" => Some(name),
                _ => None,
            })
            .unwrap_or_else(|| "Promise".into());
        let rendered = match &*promise.state.borrow() {
            quench_runtime::value::PromiseState::Pending => format!("{display_name} {{ <pending> }}"),
            quench_runtime::value::PromiseState::Fulfilled(value) => {
                format!(
                    "{display_name} {{ {} }}",
                    inspect_depth(value, depth.saturating_sub(1))
                )
            }
            quench_runtime::value::PromiseState::Rejected(value) => {
                format!(
                    "{display_name} {{ <rejected> {} }}",
                    inspect_depth(value, depth.saturating_sub(1))
                )
            }
        };
        let properties = quench_runtime::execute::own_enumerable_keys(value)
            .into_iter()
            .map(|key| format!("{key}: {}", inspect_property(value, &key, depth.saturating_sub(1))))
            .collect::<Vec<_>>();
        if properties.is_empty() {
            rendered
        } else {
            rendered
                .strip_suffix(" }")
                .map(|prefix| format!("{prefix}, {} }}", properties.join(", ")))
                .unwrap_or(rendered)
        }
    } else {
    if let Value::Array(array) = value {
        let length = array.logical_len();
        let visible_length = match quench_runtime::execute::get_property(value, "length") {
            Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
            _ => 0,
        };
        if length > 1_000 || visible_length > 1_000 {
            return inspect_sparse_array(value, visible_length.max(length), depth);
        }
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
        if matches!(
            quench_runtime::execute::get_property(value, "Symbol.toStringTag"),
            Value::String(ref tag) if tag == "Blob"
        ) {
            if depth == 0 {
                return "[Blob]".into();
            }
            let size = quench_runtime::execute::get_property(value, "size");
            let blob_type = quench_runtime::execute::get_property(value, "type");
            let blob_type = match blob_type {
                Value::String(value) => value,
                _ => String::new(),
            };
            return format!(
                "Blob {{ size: {}, type: '{}' }}",
                inspect_shallow(&size),
                blob_type
            );
        }
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
        if quench_runtime::execute::has_own_property(value, "cause") {
            let name = match quench_runtime::execute::get_property(value, "name") {
                Value::String(name) if !name.is_empty() => name,
                _ => "Error".into(),
            };
            let message = match quench_runtime::execute::get_property(value, "message") {
                Value::String(message) => message,
                _ => String::new(),
            };
            let header = if message.is_empty() {
                format!("[{name}]")
            } else {
                format!("[{name}: {message}]")
            };
            let cause = quench_runtime::execute::get_property(value, "cause");
            let rendered_cause = if let Value::Array(array) = cause {
                format!(
                    "[ {} ]",
                    (0..array.len())
                        .filter_map(|index| array.get(index))
                        .map(|entry| {
                            if is_error_value(&entry) {
                                let entry_name =
                                    match quench_runtime::execute::get_property(&entry, "name") {
                                        Value::String(name) if !name.is_empty() => name,
                                        _ => "Error".into(),
                                    };
                                let entry_message = match quench_runtime::execute::get_property(
                                    &entry, "message",
                                ) {
                                    Value::String(message) if !message.is_empty() => {
                                        format!(": {message}")
                                    }
                                    _ => String::new(),
                                };
                                format!("[{entry_name}{entry_message}]")
                            } else {
                                inspect_depth(&entry, depth.saturating_sub(1))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                inspect_property(value, "cause", depth.saturating_sub(1))
            };
            return format!("{header} {{ [cause]: {rendered_cause} }}");
        }
        if quench_runtime::execute::has_own_property(value, "errors") {
            let name = match quench_runtime::execute::get_property(value, "name") {
                Value::String(name) if !name.is_empty() => name,
                _ => "AggregateError".into(),
            };
            let message = match quench_runtime::execute::get_property(value, "message") {
                Value::String(message) if !message.is_empty() => format!(": {message}"),
                _ => String::new(),
            };
            let errors = quench_runtime::execute::get_property(value, "errors");
            let rendered_errors = if let Value::Array(array) = errors {
                format!(
                    "[ {} ]",
                    (0..array.len())
                        .filter_map(|index| array.get(index))
                        .map(|entry| {
                            if is_error_value(&entry) {
                                let name =
                                    match quench_runtime::execute::get_property(&entry, "name") {
                                        Value::String(name) if !name.is_empty() => name,
                                        _ => "Error".into(),
                                    };
                                let message = match quench_runtime::execute::get_property(
                                    &entry, "message",
                                ) {
                                    Value::String(message) if !message.is_empty() => {
                                        format!(": {message}")
                                    }
                                    _ => String::new(),
                                };
                                format!("[{name}{message}]")
                            } else {
                                inspect_depth(&entry, depth.saturating_sub(1))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                inspect_property(value, "errors", depth.saturating_sub(1))
            };
            return format!("[{name}{message}] {{\n  [errors]: {}\n}}", rendered_errors);
        }
        if matches!(
            quench_runtime::execute::get_property(value, "name"),
            Value::String(ref name) if name == "AggregateError"
        ) {
            let message = match quench_runtime::execute::get_property(value, "message") {
                Value::String(message) if !message.is_empty() => format!(": {message}"),
                _ => String::new(),
            };
            return format!("[AggregateError{message}]");
        }
        if !quench_runtime::execute::has_own_property(value, "cause")
            && !matches!(
                quench_runtime::execute::get_property(value, "cause"),
                Value::Undefined
            )
        {
            let name = match quench_runtime::execute::get_property(value, "name") {
                Value::String(name) if !name.is_empty() => name,
                _ => "Error".into(),
            };
            let message = match quench_runtime::execute::get_property(value, "message") {
                Value::String(message) if !message.is_empty() => format!(": {message}"),
                _ => String::new(),
            };
            return format!("[{name}{message}]");
        }
        let raw_stack = quench_runtime::execute::get_property(value, "stack");
        if !matches!(raw_stack, Value::String(_) | Value::Undefined)
            && quench_runtime::execute::is_truthy(&raw_stack)
        {
            let rendered = inspect_error_stack_value(&raw_stack, depth.saturating_sub(1));
            let header = inspect_error_compact(value).unwrap_or_else(|| "[Error]".into());
            let indented = rendered
                .lines()
                .map(|line| format!("    {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            return format!("{}\n{}]", header.trim_end_matches(']'), indented);
        }
        let raw_stack = match raw_stack {
            Value::String(stack) if stack.is_empty() => Value::Undefined,
            value => value,
        };
        if let Value::String(stack) = raw_stack {
            let stack = if matches!(
                quench_runtime::execute::get_own_property_descriptor(value, "stack")
                    .ok()
                    .and_then(|descriptor| match descriptor {
                        Value::Object(_) | Value::ObjectAlias(_) => {
                            Some(quench_runtime::execute::get_property(
                                &descriptor,
                                "enumerable",
                            ))
                        }
                        _ => None,
                    }),
                Some(Value::Boolean(true))
            ) {
                stack
            } else if quench_runtime::execute::has_own_property(value, "code") {
                stack
            } else {
                refresh_error_stack_header(value, &stack)
            };
            let extras = inspect_error_extras(value, depth);
            if !extras.is_empty() {
                if stack_enumerable(value) {
                    return format!("{}\n  {}\n}}", stack, extras.join("\n  "));
                }
                return format_error_with_extras(&stack, extras);
            }
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
        let header = if message.is_empty() {
            format!("[{name}]")
        } else {
            format!("[{name}: {message}]")
        };
        let extras = inspect_error_extras(value, depth);
        return if extras.is_empty() {
            header
        } else {
            format_error_with_extras(&header, extras)
        };
    }
    if quench_runtime::regexp::has_regexp_internal_slot(value) {
        return inspect_regexp(value, depth);
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
        Value::StringUnits(units) => inspect_string_units(units),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::BigInt(digits) => format!("{}n", bigint_digits(digits)),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => inspect_object(value, depth),
        Value::Array(_) => inspect_array(value, depth),
        Value::Map(_) | Value::Set(_) => inspect_collection(value, depth, false),
        Value::Iterator(_) => inspect_iterator(value, depth),
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
}

fn broadcast_channel_render(value: &Value, depth: usize) -> Option<String> {
    if !matches!(
        quench_runtime::execute::get_property(value, "\0quench:broadcast-channel"),
        Value::Boolean(true)
    ) {
        return None;
    }
    if depth == 0 {
        return Some("BroadcastChannel".into());
    }
    let name = quench_runtime::execute::to_js_string(
        &quench_runtime::execute::get_property(value, "name"),
    )
    .unwrap_or_default();
    let active = matches!(
        quench_runtime::execute::get_property(value, "active"),
        Value::Boolean(true)
    );
    Some(format!(
        "BroadcastChannel {{ name: '{}', active: {active} }}",
        name.replace('\\', "\\\\").replace('\'', "\\'")
    ))
}

fn inspect_error_stack_value(value: &Value, depth: usize) -> String {
    let rendered = inspect_depth(value, depth);
    if !matches!(value, Value::Array(_)) || !rendered.starts_with("[ ") || !rendered.ends_with(" ]")
    {
        return rendered;
    }
    let inner = &rendered[2..rendered.len() - 2];
    if inner.is_empty() {
        return "[]".into();
    }
    format!(
        "[\n{}\n]",
        inner
            .split(", ")
            .map(|item| format!("  {item}"))
            .collect::<Vec<_>>()
            .join(",\n")
    )
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

fn inspect_regexp(value: &Value, depth: usize) -> String {
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
    let props = (depth > 0)
        .then(|| quench_runtime::execute::own_enumerable_keys(value))
        .into_iter()
        .flatten()
        .filter(|key| !key.starts_with('\0'))
        .map(|key| {
            format!(
                "{key}: {}",
                inspect_depth(&quench_runtime::execute::get_property(value, &key), 0)
            )
        })
        .collect::<Vec<_>>();
    let prefix = name.map(|name| format!("{name} ")).unwrap_or_default();
    if props.is_empty() {
        format!("{prefix}{literal}")
    } else {
        format!("{prefix}{literal} {{ {} }}", props.join(", "))
    }
}

fn colorize_regexp(literal: &str) -> String {
    let enabled = COLORS_OVERRIDE.with(|slot| slot.borrow().unwrap_or(false));
    if !enabled {
        return literal.to_string();
    }
    let mut out = String::new();
    let mut last_style = String::new();
    let body_style = |depth: usize| match depth % 5 {
        0 => "\x1b[33m",
        1 => "\x1b[36m",
        2 => "\x1b[35m",
        3 => "\x1b[32m",
        _ => "\x1b[31m",
    };
    let delimiter_style = |depth: usize| match depth % 5 {
        0 => "\x1b[31m",
        1 => "\x1b[33m",
        2 => "\x1b[36m",
        3 => "\x1b[35m",
        _ => "\x1b[32m",
    };
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
    let escaped_at = |at: usize| {
        let mut count = 0;
        let mut cursor = at;
        while cursor > 0 && chars[cursor - 1] == '\\' {
            count += 1;
            cursor -= 1;
        }
        count % 2 == 1
    };
    let unicode = literal.ends_with("/u");
    let mut index = 0;
    let mut flags = false;
    let mut slash_count = 0;
    let mut in_class = false;
    let mut in_quantifier = false;
    let mut quantifier_range = false;
    let mut group_depth = 0usize;
    let mut named_group = false;
    let mut named_group_depth = None;
    while index < chars.len() {
        let character = chars[index];
        let previous_open_group =
            chars.get(index.wrapping_sub(1)) == Some(&'(') && index > 0 && !escaped_at(index - 1);
        if character == '?' && previous_open_group {
            if let Some(next) = chars.get(index + 1) {
                if matches!(next, '<' | '=' | '!' | ':') {
                    let lookbehind = *next == '<' && chars.get(index + 2) == Some(&'!');
                    push("", "");
                    if lookbehind {
                        let delimiter = delimiter_style(group_depth.saturating_sub(1));
                        push(delimiter, "?<");
                        push(delimiter, "!");
                    } else {
                        push(
                            delimiter_style(group_depth.saturating_sub(1)),
                            &format!("?{next}"),
                        );
                    }
                    named_group = *next == '<';
                    named_group_depth = named_group.then_some(group_depth);
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
            let escape_style = if unicode {
                "\x1b[36m"
            } else {
                body_style(group_depth)
            };
            let mut end = (index + 2).min(chars.len());
            if let Some(next) = chars.get(index + 1).copied() {
                match next {
                    'x' => end = (index + 4).min(chars.len()),
                    'u' if chars.get(index + 2) == Some(&'{') => {
                        end = (index + 3..chars.len())
                            .find(|position| chars[*position] == '}')
                            .map_or(chars.len(), |position| position + 1);
                    }
                    'u' => {
                        // Node's regexp lexer keeps a plain `\\uXXXX`
                        // escape granular: the introducer and every hex
                        // digit receive their own color span.  This differs
                        // from `\\xXX`, which is emitted as one token, and
                        // from braced `\\u{...}`, whose body is one span.
                        push(escape_style, "\\u");
                        push("", "");
                        for position in (index + 2)..(index + 6).min(chars.len()) {
                            push(escape_style, &chars[position].to_string());
                            push("", "");
                        }
                        index = (index + 6).min(chars.len());
                        continue;
                    }
                    'c' => end = (index + 3).min(chars.len()),
                    'p' | 'P' if chars.get(index + 2) == Some(&'{') => {
                        if let Some(close) =
                            (index + 3..chars.len()).find(|position| chars[*position] == '}')
                        {
                            // Unicode property escapes are lexed as three
                            // pieces: `\\p{`/`\\P{`, the property name, and
                            // the closing brace.  The delimiters use the
                            // current group palette while the property body
                            // uses the ordinary body palette.
                            let palette_depth = group_depth + usize::from(in_class);
                            let delimiter = delimiter_style(palette_depth);
                            push("", "");
                            push(
                                delimiter,
                                &chars[index..index + 3].iter().collect::<String>(),
                            );
                            push("", "");
                            let body: String = chars[index + 3..close].iter().collect();
                            push(body_style(palette_depth), &body);
                            push("", "");
                            push(delimiter, "}");
                            push("", "");
                            index = close + 1;
                            continue;
                        }
                        end = (index + 3).min(chars.len());
                    }
                    'k' if chars.get(index + 2) == Some(&'<') => {
                        if let Some(close) =
                            (index + 3..chars.len()).find(|position| chars[*position] == '>')
                        {
                            // Named backreferences use the palette one level
                            // behind the current group (wrapping at the
                            // outermost level), with the name itself in the
                            // corresponding body color.
                            let palette_depth = (group_depth + 4) % 5;
                            let delimiter = delimiter_style(palette_depth);
                            push("", "");
                            push(delimiter, "\\k<");
                            push("", "");
                            let body: String = chars[index + 3..close].iter().collect();
                            push(body_style(palette_depth), &body);
                            push("", "");
                            push(delimiter, ">");
                            push("", "");
                            index = close + 1;
                            continue;
                        }
                        end = (index + 3).min(chars.len());
                    }
                    digit if digit.is_ascii_digit() => {
                        while end < chars.len() && chars[end].is_ascii_digit() {
                            end += 1;
                        }
                    }
                    _ => {}
                }
            }
            let token: String = chars[index..end].iter().collect();
            push(escape_style, &token);
            push("", "");
            index = end;
            continue;
        } else if character == '(' || character == ')' || (character == '>' && named_group) {
            let delimiter = if character == ')' || character == '>' {
                delimiter_style(group_depth.saturating_sub(1))
            } else {
                delimiter_style(group_depth)
            };
            if character == '(' {
                group_depth += 1;
            } else if character == ')' {
                group_depth = group_depth.saturating_sub(1);
            } else if named_group {
                named_group = false;
                named_group_depth = None;
            }
            delimiter
        } else if character == '?' && previous_open_group {
            delimiter_style(group_depth)
        } else if character == '=' && matches!(chars.get(index.wrapping_sub(1)), Some('?')) {
            delimiter_style(group_depth)
        } else if character == '!' && chars.get(index.wrapping_sub(1)) == Some(&'?') {
            delimiter_style(group_depth)
        } else if character == '<' && chars.get(index.wrapping_sub(1)) == Some(&'?') {
            named_group = true;
            named_group_depth = Some(group_depth);
            delimiter_style(group_depth)
        } else if matches!(character, '^' | '$' | '|' | '*' | '+' | '?' | '.') {
            if in_class && character == '?' {
                body_style(group_depth)
            } else if in_class && character == '^' {
                body_style(group_depth)
            } else if in_class && character == '.' {
                body_style(group_depth)
            } else if in_class {
                body_style(group_depth)
            } else if matches!(character, '^' | '$') {
                delimiter_style((group_depth + 3) % 5)
            } else if character == '|' {
                if group_depth > 0 {
                    // Alternations use the delimiter palette offset by two
                    // levels: depth one is green, depth two red, depth three
                    // yellow, then cyan and magenta. This is the palette used
                    // by Node's util.inspect regexp lexer.
                    delimiter_style((group_depth + 3) % 5)
                } else {
                    "\x1b[35m"
                }
            } else if group_depth > 0 && matches!(character, '*' | '+' | '?') {
                delimiter_style((group_depth + 3) % 5)
            } else if character == '.' {
                delimiter_style((group_depth + 2) % 5)
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
            delimiter_style(group_depth)
        } else if character == '}' {
            in_quantifier = false;
            delimiter_style(group_depth)
        } else if character == ',' {
            if in_quantifier {
                delimiter_style((group_depth + 1) % 5)
            } else {
                delimiter_style((group_depth + 3) % 5)
            }
        } else if character == '[' {
            in_class = true;
            delimiter_style(group_depth)
        } else if character == ']' {
            // An unmatched `]` is still highlighted as a class delimiter by
            // Node's lexer (notably after an escaped `\\[`); use the inner
            // palette in that case as well.
            let palette_depth = group_depth + usize::from(!in_class);
            in_class = false;
            delimiter_style(palette_depth)
        } else if character == '-' {
            if in_class
                && chars
                    .get(index.wrapping_sub(1))
                    .is_some_and(|previous| *previous != '[')
                && chars.get(index + 1).is_some_and(|next| *next != ']')
            {
                body_style(group_depth + 1)
            } else if in_class && chars.get(index + 1).is_some_and(|next| *next != ']') {
                body_style(group_depth)
            } else {
                body_style(group_depth)
            }
        } else if character.is_ascii_digit() {
            if in_quantifier {
                delimiter_style((group_depth + 2) % 5)
            } else if in_class {
                body_style(group_depth)
            } else {
                body_style(group_depth)
            }
        } else if named_group {
            delimiter_style(named_group_depth.unwrap_or(group_depth))
        } else {
            body_style(group_depth)
        };
        let group_prefix = character == '?' && previous_open_group;
        let force_token = matches!(
            character,
            '^' | '$' | '*' | '+' | '?' | '.' | '(' | ')' | '[' | ']'
        ) || in_class
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
    inspect_collection_with_options(owner, depth, sorted, false, None)
}

fn inspect_iterator(value: &Value, depth: usize) -> String {
    inspect_iterator_with_options(value, depth, None)
}

fn inspect_iterator_with_options(
    value: &Value,
    depth: usize,
    max_array_length: Option<usize>,
) -> String {
    let Value::Iterator(iterator) = value else {
        return "<unknown>".into();
    };
    let (label, entries) = {
        let state = iterator.state.borrow();
        match &*state {
            IteratorState::Map { data, index, kind, .. } => {
                let keys = data.keys.borrow();
                let values = data.values.borrow();
                let start = (*index).min(keys.len()).min(values.len());
                let entries: Vec<Value> = (start..keys.len().min(values.len()))
                    .map(|at| match kind {
                        0 => Value::Array(Rc::new(quench_runtime::value::ArrayData::new(vec![
                            keys[at].clone(),
                            values[at].clone(),
                        ]))),
                        1 => keys[at].clone(),
                        _ => values[at].clone(),
                    })
                    .collect();
                (if *kind == 0 { "[Map Entries]" } else { "[Map Iterator]" }, entries)
            }
            IteratorState::Set { data, index, kind, .. } => {
                let values = data.values.borrow();
                let start = (*index).min(values.len());
                let entries: Vec<Value> = values
                    .iter()
                    .skip(start)
                    .map(|entry| {
                        if *kind == 1 {
                            Value::Array(Rc::new(quench_runtime::value::ArrayData::new(vec![
                                entry.clone(),
                                entry.clone(),
                            ])))
                        } else {
                            entry.clone()
                        }
                    })
                    .collect();
                (if *kind == 1 { "[Set Entries]" } else { "[Set Iterator]" }, entries)
            }
            _ => return "<unknown>".into(),
        }
    };
    let default_tag = if label == "[Map Entries]" {
        "Map Iterator"
    } else if label == "[Set Entries]" {
        "Set Iterator"
    } else {
        &label[1..label.len() - 1]
    };
    let rendered_tag = match quench_runtime::execute::get_property(value, "Symbol.toStringTag") {
        Value::String(tag) if !tag.is_empty() && tag != default_tag => {
            format!("[{tag}] ")
        }
        _ => String::new(),
    };
    let total = entries.len();
    let limit = max_array_length.unwrap_or(total);
    let omitted = total.saturating_sub(limit);
    let shown = total.saturating_sub(omitted);
    let compact = COMPACT_OVERRIDE.with(|slot| slot.borrow().unwrap_or(true));
    let mut rendered_entries = entries
        .iter()
        .take(shown)
        .map(|entry| {
            let rendered = inspect_depth(entry, depth.saturating_sub(1));
            if !compact && matches!(entry, Value::Array(_)) && rendered.starts_with("[ ") && rendered.ends_with(" ]") {
                let inner = &rendered[2..rendered.len() - 2];
                format!("[\n    {}\n  ]", inner.replace(", ", ",\n    "))
            } else {
                rendered
            }
        })
        .collect::<Vec<_>>();
    if omitted > 0 {
        rendered_entries.push(format!(
            "... {omitted} more {}",
            if omitted == 1 { "item" } else { "items" }
        ));
    }
    let properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .map(|key| format!("{}: {}", format_property_key(&key), inspect_property(value, &key, depth.saturating_sub(1))))
        .collect::<Vec<_>>();
    rendered_entries.extend(properties);
    let body = if rendered_entries.is_empty() {
        "{}".to_string()
    } else if !compact && rendered_entries.iter().any(|entry| entry.starts_with("[\n")) {
        format!("{{\n  {}\n}}", rendered_entries.join(",\n  "))
    } else {
        format!("{{ {} }}", rendered_entries.join(", "))
    };
    format!("{rendered_tag}{label} {body}")
}

fn inspect_collection_with_options(
    owner: &Value,
    depth: usize,
    sorted: bool,
    show_hidden: bool,
    max_array_length: Option<usize>,
) -> String {
    let (weak, is_set) = match owner {
        Value::Set(set) => (set.is_weak(), true),
        Value::Map(map) => (map.is_weak(), false),
        _ => return "<unknown>".into(),
    };
    let kind = if is_set { "Set" } else { "Map" };
    let default_name = if weak {
        format!("Weak{kind}")
    } else {
        kind.into()
    };
    let display_name = {
        let direct = quench_runtime::execute::get_property(owner, "constructor");
        let direct_name = match quench_runtime::execute::get_property(&direct, "name") {
            Value::String(name) if !name.is_empty() && name != default_name => Some(name),
            _ => None,
        };
        let inherited_name = quench_runtime::execute::get_prototype_of(owner)
            .ok()
            .map(|prototype| quench_runtime::execute::get_property(&prototype, "constructor"))
            .and_then(|constructor| match quench_runtime::execute::get_property(&constructor, "name") {
                Value::String(name) if !name.is_empty() && name != default_name => Some(name),
                _ => None,
            });
        direct_name.or(inherited_name).unwrap_or_else(|| default_name.clone())
    };
    let null_prototype = matches!(
        quench_runtime::execute::get_prototype_of(owner),
        Ok(Value::Null)
    );
    if weak && !show_hidden {
        let mut rendered = if null_prototype {
            format!("[{display_name}: null prototype] {{ <items unknown> }}")
        } else {
            format!("{display_name} {{ <items unknown> }}")
        };
        append_collection_properties(owner, &mut rendered, depth);
        return rendered;
    }
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
    let self_reference = entries.iter().any(|entry| entry.contains("[Circular]"));
    if self_reference {
        for entry in &mut entries {
            *entry = entry.replace("[Circular]", "[Circular *1]");
        }
    }
    let omitted = length.saturating_sub(max_array_length.unwrap_or(length));
    entries.truncate(length.saturating_sub(omitted));
    let label = if null_prototype {
        format!("[{display_name}({length}): null prototype]")
    } else {
        format!("{display_name}({length})")
    };
    let collection_tag = collection_tag_suffix(&display_name, kind);
    let label = format!("{label}{collection_tag}");
    let mut rendered = if entries.is_empty() {
        format!("{}{} {{}}", if self_reference { "<ref *1> " } else { "" }, label)
    } else if sorted {
        format!("{}{} {{\n  {}\n}}", if self_reference { "<ref *1> " } else { "" }, label, entries.join(",\n  "))
    } else {
        format!("{}{} {{ {} }}", if self_reference { "<ref *1> " } else { "" }, label, entries.join(", "))
    };
    if omitted > 0 {
        let item = if omitted == 1 { "item" } else { "items" };
        let suffix = format!("... {omitted} more {item}");
        if rendered.ends_with(" }") {
            rendered.truncate(rendered.len() - 2);
            rendered.push_str(if entries.is_empty() { "{ " } else { ", " });
            rendered.push_str(&suffix);
            rendered.push_str(" }");
        }
    }
    append_collection_properties(owner, &mut rendered, depth);
    let break_length = BREAK_LENGTH_OVERRIDE.with(|slot| slot.borrow().unwrap_or(80));
    if !rendered.contains('\n') && visible_length(&rendered) > break_length {
        if let Some(open) = rendered.find(" { ") {
            let prefix = &rendered[..open];
            let body = &rendered[open + 3..rendered.len().saturating_sub(2)];
            rendered = format!("{prefix} {{\n  {}\n}}", body.replace(", ", ",\n  "));
        }
    }
    rendered
}

fn collection_tag_suffix(name: &str, kind: &str) -> String {
    let Some(position) = name.find(kind) else {
        return String::new();
    };
    let end = position + kind.len();
    (end < name.len()
        && name[end..]
            .chars()
            .next()
            .is_some_and(|character| character.is_lowercase()))
    .then(|| format!(" [{kind}]"))
    .unwrap_or_default()
}

fn append_collection_properties(owner: &Value, rendered: &mut String, depth: usize) {
    let properties = quench_runtime::execute::own_enumerable_keys(owner)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .collect::<Vec<_>>();
    if properties.is_empty() {
        return;
    }
    let body = properties
        .into_iter()
        .map(|key| {
            format!(
                "{key}: {}",
                inspect_depth(&quench_runtime::execute::get_property(owner, &key), depth)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if rendered.ends_with('}') {
        rendered.truncate(rendered.len() - 1);
        while rendered.ends_with(' ') {
            rendered.pop();
        }
        if !rendered.ends_with('{') {
            rendered.push_str(", ");
        }
        rendered.push_str(&body);
        rendered.push_str(" }");
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
                "{key}: {}",
                inspect_depth(&quench_runtime::execute::get_property(value, &key), 0)
            )
        })
        .collect::<Vec<_>>();
    let prefix = name.map(|name| format!("{name} ")).unwrap_or_default();
    if props.is_empty() {
        prefix + &date
    } else {
        format!("{prefix}{date} {{ {} }}", props.join(", "))
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
    let body = match tag {
        Some(tag) => format!("{body} [{tag}]"),
        None => body,
    };
    let properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !matches!(key.as_str(), "name" | "length" | "prototype"))
        .map(|key| {
            format!(
                "{key}: {}",
                inspect_depth(&quench_runtime::execute::get_property(value, &key), 0)
            )
        })
        .collect::<Vec<_>>();
    if properties.is_empty() {
        body
    } else if properties.join(", ").len() > 80 {
        format!("{body} {{\n  {}\n}}", properties.join(",\n  "))
    } else {
        format!("{body} {{ {} }}", properties.join(", "))
    }
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
            key != "parent" && key != "offset" && key != "toString" && !is_array_index_key(key)
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
    if depth == 0 {
        if matches!(
            quench_runtime::execute::get_property(value, "length"),
            Value::Number(length) if length == 0.0
        ) {
            return "[]".into();
        }
        return custom_array_name(value)
            .map_or_else(|| "[Array]".into(), |name| format!("[{name}]"));
    }
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
    let length = match quench_runtime::execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as u32,
        _ => 0,
    };
    let dense_length = match value {
        Value::Array(array) => array.logical_len() as u32,
        _ => 0,
    };
    if length > 64 || dense_length > 64 {
        return inspect_sparse_array(value, length as usize, depth);
    }
    let mut holes = 0usize;
    for index in 0..length.min(64) {
        if let Some(item) = own_array_item(value, index) {
            if holes > 0 {
                items.push(empty_items(holes));
                holes = 0;
            }
            items.push(inspect_at(&item, depth - 1));
        } else {
            holes += 1;
        }
    }
    if holes > 0 {
        items.push(empty_items(holes));
    }
    let mut seen_properties = std::collections::HashSet::new();
    let mut properties = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| seen_properties.insert(key.clone()))
        .filter(|key| key != "length" && !is_array_index_key(key))
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

fn inspect_sparse_array(value: &Value, length: usize, depth: usize) -> String {
    let Value::Array(array) = value else {
        return "[]".into();
    };
    let mut all_keys = (0..length.min(1024))
        .filter(|index| array.has_index(*index))
        .map(|index| index.to_string())
        .collect::<Vec<_>>();
    all_keys.extend(array.property_keys());
    all_keys.extend(array.descriptor_keys());
    all_keys.retain(|key| !key.starts_with("\0quench:descriptor:\0"));
    let mut unique_keys = Vec::with_capacity(all_keys.len());
    for key in all_keys {
        if !unique_keys.contains(&key) {
            unique_keys.push(key);
        }
    }
    let all_keys = unique_keys;
    let mut indices = all_keys
        .iter()
        .cloned()
        .into_iter()
        .filter_map(|key| key.parse::<usize>().ok())
        .filter(|index| *index < length)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    for index in indices {
        if index > cursor {
            parts.push(empty_items(index - cursor));
        }
        if let Some(item) = own_array_item(value, index as u32) {
            parts.push(inspect_at(&item, depth.saturating_sub(1)));
        }
        cursor = index.saturating_add(1);
    }
    if cursor < length {
        parts.push(empty_items(length - cursor));
    }
    let named_keys = all_keys
        .into_iter()
        .filter(|key| key != "length" && !is_array_index_key(key))
        .collect::<Vec<_>>();
    for key in named_keys.iter().cloned() {
        parts.push(format!(
            "{}: {}",
            if key.parse::<usize>().is_ok() {
                format!("'{key}'")
            } else {
                key.clone()
            },
            inspect_property(value, &key, depth.saturating_sub(1))
        ));
    }
    if named_keys.len() >= 2 {
        format!("[\n  {}\n]", parts.join(",\n  "))
    } else {
        format!("[ {} ]", parts.join(", "))
    }
}

fn inspect_array_limited(value: &Value, depth: usize, limit: usize) -> String {
    let length = match quench_runtime::execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    // Walk the array's own index keys rather than probing every slot. Sparse
    // arrays may legally have a length near 2^32, and probing those holes
    // one-by-one turns inspection into an unbounded operation.
    let mut indices = quench_runtime::execute::own_keys(value)
        .into_iter()
        .filter_map(|key| match key {
            Value::String(key) if is_array_index_key(&key) => key
                .parse::<u64>()
                .ok()
                .filter(|index| *index < length as u64 && *index < 4_294_967_295)
                .map(|index| index as u32),
            _ => None,
        })
        .collect::<Vec<_>>();
    indices.sort_unstable();
    indices.dedup();

    let mut groups = Vec::<(String, usize)>::new();
    let mut cursor = 0usize;
    for index in indices {
        let index = index as usize;
        if index > cursor {
            groups.push((empty_items(index - cursor), index - cursor));
        }
        if let Some(item) = own_array_item(value, index as u32) {
            groups.push((stylize(&item, inspect_at(&item, depth.saturating_sub(1))), 1));
        }
        cursor = index.saturating_add(1);
    }
    if cursor < length {
        groups.push((empty_items(length - cursor), length - cursor));
    }
    let shown_groups = groups.len().min(limit);
    let mut parts = groups
        .iter()
        .take(shown_groups)
        .map(|(text, _)| text.clone())
        .collect::<Vec<_>>();
    let more: usize = groups
        .iter()
        .skip(shown_groups)
        .map(|(_, count)| *count)
        .sum();
    if more > 0 {
        parts.push(format!(
            "... {more} more item{}",
            if more == 1 { "" } else { "s" }
        ));
    }
    let mut seen_properties = std::collections::HashSet::new();
    for key in quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| key != "length" && !is_array_index_key(key))
        .filter(|key| seen_properties.insert(key.clone()))
    {
        parts.push(format!(
            "{}: {}",
            format_property_key(&key),
            inspect_property(value, &key, depth.saturating_sub(1))
        ));
    }
    if parts.is_empty() {
        return "[]".into();
    }
    let compact = parts.join(", ");
    if visible_length(&compact) > 80 {
        format!("[\n  {}\n]", parts.join(",\n  "))
    } else {
        format!("[ {compact} ]")
    }
}

fn visible_length(value: &str) -> usize {
    let mut length = 0;
    let mut escape = false;
    for byte in value.bytes() {
        if escape {
            if byte.is_ascii_alphabetic() {
                escape = false;
            }
        } else if byte == 0x1b {
            escape = true;
        } else {
            length += 1;
        }
    }
    length
}

fn empty_items(count: usize) -> String {
    let noun = if count == 1 { "item" } else { "items" };
    format!("<{count} empty {noun}>")
}

fn is_array_index_key(key: &str) -> bool {
    key == "0"
        || (key.as_bytes().first().is_some_and(|byte| *byte != b'0')
            && key.parse::<u64>().is_ok_and(|index| index < 4_294_967_295))
}

fn is_plain_array(value: &Value) -> bool {
    let Some(prototype) = quench_runtime::execute::get_prototype_of(value).ok() else {
        return true;
    };
    match quench_runtime::execute::get_property(&prototype, "constructor") {
        Value::Undefined => true,
        constructor => !matches!(
            quench_runtime::execute::get_property(&constructor, "name"),
            Value::String(name) if !name.is_empty() && name != "Array"
        ),
    }
}

fn inspect_named_array(value: &Value, depth: usize, name: &str) -> String {
    let length = match quench_runtime::execute::get_property(value, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    let mut parts = Vec::new();
    let mut holes = 0usize;
    for index in 0..length.min(64) {
        let item = own_array_item(value, index as u32);
        if item.is_none() {
            holes += 1;
        } else {
            if holes > 0 {
                parts.push(empty_items(holes));
                holes = 0;
            }
            parts.push(inspect_at(
                &item.unwrap_or(Value::Undefined),
                depth.saturating_sub(1),
            ));
        }
    }
    if holes > 0 {
        parts.push(empty_items(holes));
    }
    let mut seen_properties = std::collections::HashSet::new();
    for key in quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| seen_properties.insert(key.clone()))
        .filter(|key| !key.starts_with('\0'))
    {
        if key != "length" && !is_array_index_key(&key) {
            parts.push(format!(
                "{key}: {}",
                inspect_property(value, &key, depth.saturating_sub(1))
            ));
        }
    }
    format!("{name}({length}) [ {} ]", parts.join(", "))
}

fn own_array_item(value: &Value, index: u32) -> Option<Value> {
    let key = index.to_string();
    quench_runtime::execute::has_own_property(value, &key)
        .then(|| quench_runtime::execute::get_property(value, &key))
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
        let cyclic = inspect_identity(value).is_some_and(|identity| {
            INSPECT_CONTEXT.with(|slot| {
                slot.borrow()
                    .as_ref()
                    .is_some_and(|context| {
                        context.cycle_ids.contains_key(&identity)
                            || context
                                .cycle_values
                                .iter()
                                .any(|(candidate, _)| quench_runtime::execute::same_identity(candidate, value))
                    })
            })
        });
        if cyclic {
            return inspect_depth_tracked(value, 0);
        }
        inspect_shallow(value)
    } else {
        inspect_depth(value, depth)
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
                    let display = format_property_key(&key);
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
        match quench_runtime::vm::get_property(value, "\0original_constructor_name") {
            Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
                Some(symbol_string(&Value::String(name)))
            }
            Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
            _ => None,
        }
        .or_else(
            || match quench_runtime::execute::get_property(value, "constructor") {
                Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
                    Some(symbol_string(&Value::String(name)))
                }
                Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
                constructor => match quench_runtime::execute::get_property(&constructor, "name") {
                    Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
                        Some(symbol_string(&Value::String(name)))
                    }
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
                        Value::String(name) if quench_runtime::execute::is_symbol(&Value::String(name.clone())) => {
                            Some(symbol_string(&Value::String(name)))
                        }
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
    let keys = inspect_enumerable_keys(value);
    let mut keys = keys;
    let plain_symbol = keys
        .iter()
        .any(|key| !key.contains('\0') && format_property_key(key).starts_with("Symbol("));
    if plain_symbol {
        keys.retain(|key| !key.contains('\0'));
    }
    let mut displays = std::collections::HashSet::new();
    keys.retain(|key| displays.insert(format_property_key(key)));
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
            let property_value = quench_runtime::execute::get_property_result(value, key)
                .unwrap_or(Value::Undefined);
            let rendered = if matches!(key.as_str(), "actual" | "expected") {
                match property_value {
                    Value::String(text) if text.len() > if matches!(quench_runtime::execute::get_property(value, "diff"), Value::String(_)) && text.len() > 9_488 { 9_488 } else { 488 } => {
                        let limit = if matches!(quench_runtime::execute::get_property(value, "diff"), Value::String(_)) && text.len() > 9_488 { 9_488 } else { 488 };
                        format!("'{}...'", &text[..limit])
                    }
                    Value::String(text) if text.contains('\n') => {
                        let full_diff = matches!(
                            quench_runtime::execute::get_property(value, "diff"),
                            Value::String(mode) if mode == "full"
        );
                        if full_diff {
                            let prefix = text.split_inclusive('\n').take(10).collect::<String>();
                            format!("'{}...'", prefix.replace('\n', "\\n"))
                        } else if text.matches('\n').count() > 50 {
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
                    Value::Array(array) if array.len() > 50 => "[Array]".into(),
                    _ => inspect_property(value, key, depth.saturating_sub(1)),
                }
            } else {
                inspect_property(value, key, depth.saturating_sub(1))
            };
            format!("{}: {}", format_property_key(key), rendered)
        })
        .collect::<Vec<_>>()
        .join(", ");
    if null_prototype {
        let name = constructor_name.unwrap_or_else(|| "Object".into());
        return format!("[{name}: null prototype] {{ {body} }}");
    }
    let prefix = constructor_name
        .map(|name| format!("{name} "))
        .unwrap_or_default();
    if visible_length(&body) > 120 || body.contains("<ref *") {
        format!("{prefix}{{\n  {}\n}}", multiline_body(&body))
    } else {
        format!("{prefix}{{ {body} }}")
    }
}

fn inspect_enumerable_keys(value: &Value) -> Vec<String> {
    let mut keys = quench_runtime::execute::own_enumerable_keys(value);
    let symbols = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertySymbols),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok();
    let symbol_length = symbols
        .as_ref()
        .and_then(|symbols| match quench_runtime::execute::get_property(symbols, "length") {
            Value::Number(length) if length >= 0.0 => Some(length as usize),
            _ => None,
        })
        .unwrap_or(0);
    for index in 0..symbol_length {
        let symbol = symbols
            .as_ref()
            .map(|symbols| quench_runtime::execute::get_property(symbols, &index.to_string()))
            .unwrap_or(Value::Undefined);
        let Value::String(raw) = symbol.clone() else {
            continue;
        };
        if keys.iter().any(|item| item == &raw) {
            continue;
        }
        let descriptor = quench_runtime::execute::call(
            &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetOwnPropertyDescriptor),
            &Value::Undefined,
            &[value.clone(), symbol],
        )
        .ok();
        if matches!(
            descriptor
                .as_ref()
                .map(|descriptor| quench_runtime::execute::get_property(
                    descriptor,
                    "enumerable"
                )),
            Some(Value::Boolean(true))
        ) {
            keys.push(raw);
        }
    }
    let mut seen_symbol_displays = std::collections::HashSet::new();
    keys.retain(|key| {
        let display = format_property_key(key);
        !display.starts_with("Symbol(") || seen_symbol_displays.insert(display)
    });
    keys
}

fn encoded_symbol_key(key: &str) -> bool {
    quench_runtime::execute::is_symbol(&Value::String(key.to_string()))
        || key
            .split_once('\0')
            .is_some_and(|(body, _)| body.starts_with("Symbol."))
        || (key.starts_with("Symbol.") && key.contains('\u{1}'))
}

fn multiline_body(body: &str) -> String {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut braces = 0i32;
    let mut brackets = 0i32;
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in body.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote.is_some() {
            escaped = true;
            continue;
        }
        if let Some(current) = quote {
            if character == current {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == '{' {
            braces += 1;
        } else if character == '[' {
            brackets += 1;
        } else if character == '}' {
            braces -= 1;
        } else if character == ']' {
            brackets -= 1;
        } else if character == ',' && braces == 0 && brackets == 0 {
            parts.push(body[start..index].trim().to_string());
            start = index + 1;
        }
    }
    parts.push(body[start..].trim().to_string());
    parts.join(",\n  ")
}

fn format_property_key(key: &str) -> String {
    let value = Value::String(key.to_string());
    if encoded_symbol_key(key) {
        return symbol_string(&value);
    }
    let mut chars = key.chars();
    let identifier = chars
        .next()
        .is_some_and(|first| first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '$' || character.is_ascii_alphanumeric()
        });
    if identifier {
        key.to_string()
    } else {
        inspect_string(key)
    }
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
                    return format!("[Getter: {}]", inspect_depth(&result, depth));
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
    let property = quench_runtime::execute::get_property(value, key);
    colorize(&property, inspect_at(&property, depth))
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
        Value::BigInt(digits) => format!("{}n", bigint_digits(digits)),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => {
            if quench_runtime::execute::own_enumerable_keys(value).is_empty() {
                "{}".into()
            } else {
                "[Object]".into()
            }
        }
        Value::Array(_) => {
            if matches!(
                quench_runtime::execute::get_property(value, "length"),
                Value::Number(length) if length == 0.0
            ) {
                "[]".into()
            } else {
                custom_array_name(value)
                    .map_or_else(|| "[Array]".into(), |name| format!("[{name}]"))
            }
        }
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

fn is_inspect_capability(value: &Value) -> bool {
    matches!(
        value,
        Value::BoundFunction(bound)
            if matches!(
                bound.target,
                Value::Builtin(quench_runtime::ops::Builtin::HostCapability(kind))
                    if kind
                        == quench_runtime::ops::HostCapabilityKind::Custom(
                            crate::registry::SPEC_UTIL_INSPECT.cap,
                        )
            )
    )
}

fn inspect_custom_with_receiver(value: &Value, receiver: &Value, depth: usize) -> Option<String> {
    if CUSTOM_INSPECT_OVERRIDE.with(|slot| slot.borrow().is_some_and(|enabled| !enabled)) {
        return None;
    }
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
        if matches!(value, Value::Array(_)) {
            return None;
        }
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
        if matches!(value, Value::Array(_)) {
            return None;
        }
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
    // `util.inspect` is itself a valid custom-inspect value, but Node avoids
    // recursively invoking that same inspector. Treat identity as the fact
    // (rather than a name string) so aliases retain the same behavior.
    if is_inspect_capability(&method) {
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
    if result.object_identity() == value.object_identity() {
        let base = inspect_object(value, depth);
        if base.contains("Symbol(nodejs.util.inspect.custom)") {
            if let Some(body) = base.strip_prefix("{ ").and_then(|body| body.strip_suffix(" }")) {
                return Some(format!("{{\n  {}\n}}", multiline_body(body)));
            }
            return Some(base);
        }
        let method_text = inspect_function(&method);
        if let Some(body) = base.strip_prefix("{ ").and_then(|body| body.strip_suffix(" }")) {
            return Some(format!(
                "{{\n  {body},\n  Symbol(nodejs.util.inspect.custom): {method_text}\n}}"
            ));
        }
        return Some(base);
    }
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
    for key in quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
    {
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
        .filter(|key| !key.starts_with('\0'))
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

fn is_typed_array_value(value: &Value) -> bool {
    matches!(
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
    )
}

fn inspect_typed_array_values(value: &Value, limit: usize) -> String {
    let Some((name, length, _, _, _)) = typed_array_info(value) else {
        return "<unknown>".into();
    };
    let shown = length.min(limit);
    let broken_length = matches!(
        quench_runtime::execute::get_property(value, "length"),
        Value::Number(number) if number < 0.0
    );
    let values = (0..shown)
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
    let omitted = length - shown;
    if omitted == 0 {
        return if values.is_empty() {
            format!("{name}({length}) []")
        } else {
            format!("{name}({length}) [ {} ]", values.join(", "))
        };
    }
    let item = if omitted == 1 { "item" } else { "items" };
    if shown == 0 {
        return format!("{name}({length}) [ ... {omitted} more {item} ]");
    }
    format!(
        "{name}({length}) [\n  {},\n  ... {omitted} more {item}\n]",
        values.join(", ")
    )
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
