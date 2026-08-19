//! `util` module — formatting + type inspection.
//!
//! Node-compatible `util.format` with `%s`, `%d`, `%i`, `%f`, `%j`,
//! `%o`, `%O`, `%%`. Plus `util.inspect` (string-only; sufficient
//! for the test262 + Node fixture conformance surface).

use quench_runtime::value::Value;

/// Module wiring: returns the `(name, value)` pairs the host
/// installs into the `util` namespace.
pub fn build() -> Vec<(String, Value)> {
    vec![
        (
            "format".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_FORMAT),
        ),
        (
            "inspect".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_INSPECT),
        ),
        (
            "getCallSites".to_string(),
            crate::host::capability(crate::registry::SPEC_UTIL_GETCALLSITES),
        ),
    ]
}

/// `util.format` — see test fixture `parallel/test-util-format.js`.
pub fn format(args: &[Value]) -> String {
    if args.is_empty() {
        return String::new();
    }
    if let Value::String(template) = &args[0] {
        format_template(template, args)
    } else {
        format_varargs(args)
    }
}

/// Public for `console.log` reuse.
pub fn format_template(template: &str, args: &[Value]) -> String {
    let mut out = String::new();
    let mut iter = template.chars().peekable();
    let mut index = 1usize;
    let mut extra = false;
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
        let arg = args.get(index).cloned().unwrap_or(Value::Undefined);
        index += 1;
        out.push_str(&format_spec(spec, &arg));
        extra = true;
    }
    // Node's util.format appends remaining positional args separated
    // by spaces, mirroring console.log's behavior.
    for arg in args.iter().skip(index) {
        if extra {
            out.push(' ');
        }
        extra = true;
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
        'd' | 'i' => to_int_string(arg),
        'f' => to_float_string(arg),
        'j' => json_string(arg),
        'o' | 'O' => inspect(arg),
        other => format!("%{other}"),
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => "<unknown>".into(),
    }
}

fn to_number(value: &Value) -> f64 {
    match value {
        Value::Number(n) => *n,
        Value::String(s) => s.parse().unwrap_or(f64::NAN),
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

fn to_int_string(value: &Value) -> String {
    let n = to_number(value);
    if n.is_nan() {
        "NaN".into()
    } else {
        format!("{}", n as i64)
    }
}

fn to_float_string(value: &Value) -> String {
    let n = to_number(value);
    if n.is_nan() {
        "NaN".into()
    } else {
        format!("{n}")
    }
}

fn json_string(value: &Value) -> String {
    if matches!(value, Value::Undefined) {
        return "undefined".into();
    }
    std::string::ToString::to_string(&format!("{:?}", value))
}

/// `util.inspect` — string-only, sufficient for fixtures.
pub fn inspect(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) => inspect_object(value, 2),
        Value::Array(_) => inspect_array(value, 2),
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
    format!("[ {} ]", items.join(", "))
}

fn inspect_at(value: &Value, depth: usize) -> String {
    if depth == 0 {
        return inspect_shallow(value);
    }
    match value {
        Value::Object(_) => inspect_object(value, depth - 1),
        Value::Array(_) => inspect_array(value, depth - 1),
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
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        Value::Object(_) => "[object Object]".into(),
        Value::Array(_) => "[Array]".into(),
        _ => "<unknown>".into(),
    }
}
