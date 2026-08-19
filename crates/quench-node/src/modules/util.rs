//! `util` module — formatting + type inspection.
//!
//! Node-compatible `util.format` with `%s`, `%d`, `%i`, `%f`, `%j`,
//! `%o`, `%O`, `%%`. Plus `util.inspect` (string-only; sufficient
//! for the test262 + Node fixture conformance surface).

use std::cell::RefCell;

use quench_runtime::value::Value;

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

/// Module wiring: returns the `(name, value)` pairs the host
/// installs into the `util` namespace.
pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "format".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_FORMAT),
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
    ]
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

/// Node's `ERR_INVALID_ARG_TYPE` "Received …" suffix.
pub fn invalid_arg_received(value: &Value) -> String {
    match value {
        Value::Null => " Received null".into(),
        Value::Undefined => " Received undefined".into(),
        Value::Object(_) => {
            let name = quench_runtime::execute::get_property(value, "constructor");
            let name = quench_runtime::execute::get_property(&name, "name");
            match name {
                Value::String(name) if !name.is_empty() => {
                    format!(" Received an instance of {name}")
                }
                _ => " Received [object Object]".into(),
            }
        }
        Value::Function(_) | Value::BoundFunction(_) => {
            let name = quench_runtime::execute::get_property(value, "name");
            match name {
                Value::String(name) => format!(" Received function {name}"),
                _ => " Received function".into(),
            }
        }
        Value::Boolean(_) => format!(" Received type boolean ({})", inspect(value)),
        Value::Number(_) | Value::BigInt(_) => format!(
            " Received type {} ({})",
            if matches!(value, Value::Number(_)) {
                "number"
            } else {
                "bigint"
            },
            inspect(value)
        ),
        Value::String(_) => format!(" Received type string ({})", inspect(value)),
        _ => format!(" Received {}", inspect(value)),
    }
}

/// `util.inspect` — string-only, sufficient for fixtures.
pub fn inspect(value: &Value) -> String {
    inspect_depth(value, 2)
}

fn inspect_depth(value: &Value, depth: usize) -> String {
    if quench_runtime::execute::is_symbol(value) {
        return symbol_string(value);
    }
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => js_number(*n),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) | Value::ObjectAlias(_) => inspect_object(value, depth),
        Value::Array(_) => inspect_array(value, depth),
        Value::Uint8Array(_) => "[Buffer]".into(),
        _ => "<unknown>".into(),
    }
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
    let keys = quench_runtime::execute::own_enumerable_keys(value);
    if keys.is_empty() {
        return "{}".into();
    }
    let body = keys
        .iter()
        .map(|key| {
            format!(
                "{key}: {}",
                inspect_at(&quench_runtime::execute::get_property(value, key), depth)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
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
        _ => "<unknown>".into(),
    }
}
