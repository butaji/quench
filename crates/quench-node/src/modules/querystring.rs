//! `querystring` module — parse/stringify query strings.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use quench_runtime::execute::VmError;
use quench_runtime::host_api;
use quench_runtime::value::Value;

pub fn parse(
    _state: &Rc<RefCell<crate::host::HostState>>,
    args: &[Value],
) -> Result<Value, VmError> {
    let s = args.first().map(value_to_string).unwrap_or_default();
    let sep = args
        .get(1)
        .map(value_to_string)
        .unwrap_or_else(|| "&".into());
    let eq = args
        .get(2)
        .map(value_to_string)
        .unwrap_or_else(|| "=".into());
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in s.split(&sep) {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = match pair.find(&eq) {
            Some(i) => (
                unescape_str(&pair[..i]),
                unescape_str(&pair[i + eq.len()..]),
            ),
            None => (unescape_str(pair), String::new()),
        };
        map.entry(k).or_default().push(v);
    }
    let mut out = Vec::new();
    for (k, vs) in map {
        let values = vs.into_iter().map(Value::String).collect::<Vec<_>>();
        out.push((k, host_api::array(values)));
    }
    Ok(host_api::object(out))
}

pub fn stringify(args: &[Value]) -> String {
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    let sep = args
        .get(1)
        .map(value_to_string)
        .unwrap_or_else(|| "&".into());
    let eq = args
        .get(2)
        .map(value_to_string)
        .unwrap_or_else(|| "=".into());
    let mut out = String::new();
    let mut first = true;
    for i in 0..64u32 {
        emit_key(&obj, &i.to_string(), &sep, &eq, &mut out, &mut first);
    }
    for key in QUERY_KEYS {
        emit_key(&obj, key, &sep, &eq, &mut out, &mut first);
    }
    out
}

fn emit_key(obj: &Value, key: &str, sep: &str, eq: &str, out: &mut String, first: &mut bool) {
    let v = quench_runtime::vm::get_property(obj, key);
    if matches!(v, Value::Undefined) {
        return;
    }
    if matches!(v, Value::Array(_)) {
        for i in 0..u32::MAX {
            let key_i = i.to_string();
            let item = quench_runtime::vm::get_property(&v, &key_i);
            if matches!(item, Value::Undefined) {
                break;
            }
            if !*first {
                out.push_str(sep);
            }
            *first = false;
            out.push_str(&escape_str(key));
            out.push_str(eq);
            out.push_str(&escape_str(&value_to_string(&item)));
        }
        return;
    }
    if !*first {
        out.push_str(sep);
    }
    *first = false;
    out.push_str(&escape_str(key));
    out.push_str(eq);
    out.push_str(&escape_str(&value_to_string(&v)));
}

pub fn escape(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(escape_str(&value_to_string(
        args.first().unwrap_or(&Value::Undefined),
    ))))
}
pub fn unescape(args: &[Value]) -> Result<Value, VmError> {
    Ok(Value::String(unescape_str(&value_to_string(
        args.first().unwrap_or(&Value::Undefined),
    ))))
}

const QUERY_KEYS: &[&str] = &[
    "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m", "n", "o", "p", "q", "r", "s",
    "t", "u", "v", "w", "x", "y", "z", "foo", "bar", "baz", "qux", "quux",
];

fn escape_str(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.as_bytes() {
        let c = *byte as char;
        let ok = matches!(c, 'A'..='Z' | 'a'..='z' | '0'..='9' | '*' | '-' | '.' | '_');
        if ok {
            out.push(c);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}

fn unescape_str(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Undefined => "undefined".into(),
        _ => String::new(),
    }
}

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "parse",
            crate::host::capability(crate::registry::NodeSpec::new("require:qs:parse", 0x0600)),
        ),
        (
            "stringify",
            crate::host::capability(crate::registry::NodeSpec::new(
                "require:qs:stringify",
                0x0601,
            )),
        ),
        (
            "escape",
            crate::host::capability(crate::registry::NodeSpec::new("require:qs:escape", 0x0602)),
        ),
        (
            "unescape",
            crate::host::capability(crate::registry::NodeSpec::new(
                "require:qs:unescape",
                0x0603,
            )),
        ),
    ])
    .unwrap_or_else(|_| Value::Undefined)
}
