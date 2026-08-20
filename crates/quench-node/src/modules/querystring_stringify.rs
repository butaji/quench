//! `QueryString.stringify` — port of Node's `lib/querystring.js`
//! `stringify` with `stringifyPrimitive`/`encodeStringified` semantics.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::value::Value;

use crate::host::HostState;
use crate::modules::querystring::{call_string, encode_str, module_fn, Decode};
// ---- stringify ----

/// The active encoder: the module's `escape` unless the caller supplied
/// `options.encodeURIComponent`.
fn select_encode(options: Option<&Value>, receiver: Option<&Value>) -> Decode {
    match options.map(|o| execute::get_property(o, "encodeURIComponent")) {
        Some(f) if quench_runtime::is_callable(&f) => Decode::Custom(f),
        _ => module_fn(receiver, "escape", crate::registry::SPEC_QS_ESCAPE.cap),
    }
}

/// `sep ||= '&'` / `eq ||= '='`.
fn text_arg(arg: Option<&Value>, default: &str) -> Result<String, VmError> {
    match arg {
        Some(value) if execute::is_truthy(value) => execute::to_js_string(value),
        _ => Ok(default.to_string()),
    }
}

fn is_object_like(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(_) | Value::ObjectAlias(_) | Value::Array(_) | Value::Proxy(_)
    )
}

/// `stringifyPrimitive`.
fn stringify_primitive(value: &Value) -> String {
    if execute::is_symbol(value) {
        return String::new();
    }
    if let Some(units) = execute::string_units(value) {
        return String::from_utf16_lossy(&units);
    }
    match value {
        Value::Number(n) if n.is_finite() => execute::number_to_js_string(*n),
        Value::BigInt(digits) => digits.clone(),
        Value::Boolean(b) => b.to_string(),
        _ => String::new(),
    }
}

/// `encodeStringified` (builtin encoder) / `encodeStringifiedCustom`.
fn encode_value(value: &Value, encode: &Decode) -> Result<String, VmError> {
    if let Decode::Custom(f) = encode {
        let units: Vec<u16> = stringify_primitive(value).encode_utf16().collect();
        return call_string(f, &units, None);
    }
    if execute::is_symbol(value) {
        return Ok(String::new());
    }
    if let Some(units) = execute::string_units(value) {
        return encode_str(&units);
    }
    Ok(match value {
        Value::Number(n) if n.is_finite() && n.abs() < 1e21 => execute::number_to_js_string(*n),
        Value::Number(n) if n.is_finite() => {
            let units: Vec<u16> = execute::number_to_js_string(*n).encode_utf16().collect();
            encode_str(&units)?
        }
        Value::BigInt(digits) => digits.clone(),
        Value::Boolean(b) => b.to_string(),
        _ => String::new(),
    })
}

fn emit_field(
    fields: &mut String,
    sep: &str,
    eq: &str,
    key: &str,
    value: &Value,
    encode: &Decode,
) -> Result<(), VmError> {
    let ks = encode_value(&Value::String(key.to_string()), encode)? + eq;
    if let Value::Array(_) = value {
        for index in 0..u32::MAX {
            let item = execute::get_property(value, &index.to_string());
            if matches!(item, Value::Undefined) {
                break;
            }
            if !fields.is_empty() || index > 0 {
                fields.push_str(sep);
            }
            fields.push_str(&ks);
            fields.push_str(&encode_value(&item, encode)?);
        }
        return Ok(());
    }
    if !fields.is_empty() {
        fields.push_str(sep);
    }
    fields.push_str(&ks);
    fields.push_str(&encode_value(value, encode)?);
    Ok(())
}

pub fn stringify(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let sep = text_arg(args.get(1), "&")?;
    let eq = text_arg(args.get(2), "=")?;
    let encode = select_encode(args.get(3), receiver);
    let obj = args.first().cloned().unwrap_or(Value::Undefined);
    if !is_object_like(&obj) {
        return Ok(Value::String(String::new()));
    }
    let mut fields = String::new();
    for key in execute::own_enumerable_keys(&obj) {
        let value = execute::get_property(&obj, &key);
        emit_field(&mut fields, &sep, &eq, &key, &value, &encode)?;
    }
    Ok(Value::String(fields))
}
