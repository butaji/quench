//! `util` module — formatting + type inspection.
//!
//! Node-compatible `util.format` with `%s`, `%d`, `%i`, `%f`, `%j`,
//! `%o`, `%O`, `%%`. Plus `util.inspect` (string-only; sufficient
//! for the test262 + Node fixture conformance surface).

use std::cell::RefCell;
use std::collections::HashMap;

use quench_runtime::execute::VmError;
use quench_runtime::ops::FunctionKind;
use quench_runtime::value::{IteratorState, Value};

thread_local! {
    /// The live `util.inspect.defaultOptions` object; formatters read
    /// through it so JavaScript-side mutation is observed.
    static INSPECT_DEFAULT_OPTIONS: RefCell<Option<Value>> = const { RefCell::new(None) };
    /// Per-call override set by `util.formatWithOptions`.
    static SEPARATOR_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
}

/// `util.formatWithOptions(options, ...args)`.
pub fn format_with_options(args: &[Value], numeric_separator: bool) -> String {
    SEPARATOR_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(numeric_separator));
    let result = format(args);
    SEPARATOR_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
    result
}

/// Parse Node's dotenv-style environment format into a null-prototype object.
pub fn parse_env(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(source)) = arguments.first() else {
        return Err(crate::modules::buffer_enc::invalid_arg_type(
            "str must be a string".into(),
        ));
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
        (
            "promisify".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_PROMISIFY),
        ),
        ("inspect".to_string(), inspect_capability()),
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
        "isGeneratorFunction" => matches!(value, Value::Function(function) if function.kind == FunctionKind::Generator && !function.is_async),
        "isAsyncFunction" => matches!(value, Value::Function(function) if function.is_async && function.kind != FunctionKind::Generator),
        "isArgumentsObject" => value.is_arguments_object(),
        "isBooleanObject" => boxed_constructor(value, "Boolean"),
        "isNumberObject" => boxed_constructor(value, "Number"),
        "isStringObject" => boxed_constructor(value, "String"),
        "isSymbolObject" => boxed_constructor(value, "Symbol"),
        "isBigIntObject" => boxed_constructor(value, "BigInt"),
        "isBoxedPrimitive" => ["Boolean", "Number", "String", "Symbol", "BigInt"].iter().any(|kind| boxed_constructor(value, kind)),
        "isNativeError" => matches!(quench_runtime::execute::get_property_result(value, "\0error_slot"), Ok(Value::Boolean(true))),
        "isExternal" => matches!(quench_runtime::execute::get_property_result(value, "__quench_external"), Ok(Value::Boolean(true))),
        "isFloat16Array" => value.is_float16_array(),
        "isModuleNamespaceObject" => matches!(quench_runtime::execute::get_property_result(value, "\0module_namespace"), Ok(Value::Boolean(true))),
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
    matches!(value, Value::Object(_) | Value::ObjectAlias(_) | Value::BindingCell(_))
        && matches!(prototype, Ok(Value::Builtin(actual)) if actual == expected)
        && matches!(quench_runtime::execute::get_property_result(value, "_value"), Ok(Value::Boolean(_) | Value::Number(_) | Value::String(_) | Value::BigInt(_)))
}

fn inspect_capability() -> Value {
    let inspect = crate::host::capability(crate::registry::SPEC_UTIL_INSPECT);
    let options = quench_runtime::host_api::object(vec![(
        "numericSeparator".to_string(),
        Value::Boolean(false),
    )]);
    INSPECT_DEFAULT_OPTIONS.with(|slot| *slot.borrow_mut() = Some(options.clone()));
    let _ = quench_runtime::execute::set_callable_property(&inspect, "defaultOptions", options);
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
        out.push_str(&format_spec('s', arg));
    }
    out
}

fn format_varargs(args: &[Value]) -> String {
    let mut out = String::new();
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&inspect(arg));
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
        'o' | 'O' => inspect(arg),
        other => format!("%{other}"),
    }
}

fn value_to_string(value: &Value) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
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
        Value::Object(_)
        | Value::ObjectAlias(_)
        | Value::Array(_)
        | Value::Function(_)
        | Value::BoundFunction(_) => match quench_runtime::execute::to_js_string(value) {
            Ok(text) if text != "[object Object]" && !text.is_empty() => text,
            // `%s` inspects plain objects at depth 0: nested containers
            // collapse to `[Array]` / `[Object]`.
            _ => inspect_depth(value, 0),
        },
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
    if description.is_empty() || description == "\u{1}" {
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
    inspect_depth(value, 2)
}

fn inspect_depth(value: &Value, depth: usize) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    if matches!(
        quench_runtime::execute::get_property_result(value, "\0regexp"),
        Ok(Value::Boolean(true))
    ) {
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
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => inspect_object(value, depth),
        Value::Array(_) => inspect_array(value, depth),
        Value::Function(_) | Value::BoundFunction(_) => inspect_function(value),
        Value::Uint8Array(view) if is_buffer_view(value) => inspect_buffer(value, view),
        Value::Uint8Array(_) => "Uint8Array(0) []".into(),
        Value::BigInt(digits) => format!("{digits}n"),
        _ => "<unknown>".into(),
    }
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
    format!("/{source}/{flags}")
}

fn inspect_date(value: &Value) -> String {
    let method = quench_runtime::execute::get_property(value, "toISOString");
    match quench_runtime::execute::call(&method, value, &[]) {
        Ok(Value::String(date)) => date,
        _ => "Invalid Date".into(),
    }
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
    let null_prototype = matches!(quench_runtime::execute::get_prototype_of(value), Ok(Value::Null));
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
            Ok(Value::Builtin(quench_runtime::ops::Builtin::AsyncFunctionPrototype))
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
    let properties = quench_runtime::execute::own_enumerable_keys(value)
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
    if items.is_empty() {
        return "[]".into();
    }
    format!("[ {} ]", items.join(", "))
}

fn inspect_at(value: &Value, depth: usize) -> String {
    if depth == 0 {
        return inspect_shallow(value);
    }
    match value {
        Value::Object(_) | Value::ObjectAlias(_) if depth > 0 => inspect_object(value, depth),
        Value::Array(_) if depth > 0 => inspect_array(value, depth),
        _ => inspect_shallow(value),
    }
}

/// Plain objects render as `{ key: value, ... }` with shallow values.
fn inspect_object(value: &Value, depth: usize) -> String {
    let prototype = quench_runtime::execute::get_prototype_of(value).ok();
    let original_prototype = value.original_prototype();
    let null_prototype = matches!(prototype, Some(Value::Null));
    let constructor_name = original_prototype.as_ref().or(prototype.as_ref()).and_then(|prototype| {
        let constructor = quench_runtime::execute::get_property(prototype, "constructor");
        match quench_runtime::execute::get_property(&constructor, "name") {
            Value::String(name) if !name.is_empty() && name != "Object" => Some(name),
            _ => None,
        }
    });
    let keys = quench_runtime::execute::own_enumerable_keys(value);
    if keys.is_empty() {
        return if let Some(name) = constructor_name {
            format!("[{name}: null prototype] {{}}")
        } else if null_prototype {
            "[Object: null prototype] {}".into()
        } else {
            "{}".into()
        };
    }
    let body = keys
        .iter()
        .map(|key| {
            format!(
                "{}: {}",
                if key.parse::<usize>().is_ok() { format!("'{key}'") } else { key.clone() },
                inspect_at(&quench_runtime::execute::get_property(value, key), depth)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    if null_prototype {
        return format!("[Object: null prototype] {{ {body} }}");
    }
    format!("{{ {body} }}")
}

fn inspect_shallow(value: &Value) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
        Value::Array(_) => "[Array]".into(),
        Value::Function(_) | Value::BoundFunction(_) => inspect_function(value),
        Value::Uint8Array(view) if is_buffer_view(value) => inspect_buffer(value, view),
        Value::Uint8Array(view) => format!("Uint8Array({}) []", view.length),
        _ => "<unknown>".into(),
    }
}
