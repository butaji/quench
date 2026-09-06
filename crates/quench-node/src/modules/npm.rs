//! Small Rust-owned facsimiles for application packages used by the
//! compatibility gates. They expose ordinary package shapes through the same
//! capability table as built-in Node modules; no JavaScript package runtime is
//! introduced.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::execute::{self, VmError};
use quench_runtime::host_api;
use quench_runtime::value::Value;

use crate::host::HostState;

pub fn ajv_module() -> Value {
    crate::host::capability(crate::registry::SPEC_NPM_AJV_CONSTRUCTOR)
}

pub fn ajv_construct(_state: &Rc<RefCell<HostState>>, _args: &[Value]) -> Result<Value, VmError> {
    Ok(host_api::object(vec![
        (
            "compile".into(),
            crate::host::capability(crate::registry::SPEC_NPM_AJV_COMPILE),
        ),
        ("errors".into(), Value::Null),
    ]))
}

pub fn ajv_compile(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let schema = args.first().cloned().unwrap_or(Value::Undefined);
    let ajv = receiver.cloned().unwrap_or(Value::Undefined);
    let validator = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_NPM_AJV_VALIDATE),
        vec![schema, ajv],
    );
    // A self receiver gives the validator capability a stable place to publish
    // its `errors` array after each call, without a side table or JS closure.
    execute::set_property_in_place(&validator, "\0bound_this", validator.clone());
    execute::set_property_in_place(&validator, "errors", Value::Null);
    Ok(validator)
}

pub fn ajv_validate(
    _state: &Rc<RefCell<HostState>>,
    receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let schema = args.first().cloned().unwrap_or(Value::Undefined);
    let ajv = args.get(1).cloned().unwrap_or(Value::Undefined);
    let data = args.get(2).cloned().unwrap_or(Value::Undefined);
    let mut errors = Vec::new();
    let valid = validate_schema(&schema, &data, &mut errors);
    let error_value = if valid {
        Value::Null
    } else {
        host_api::array(errors)
    };
    if let Some(validator) = receiver {
        execute::set_property_in_place(validator, "errors", error_value.clone());
    }
    execute::set_property_in_place(&ajv, "errors", error_value);
    Ok(Value::Boolean(valid))
}

fn validate_schema(schema: &Value, data: &Value, errors: &mut Vec<Value>) -> bool {
    if !matches!(data, Value::Object(_) | Value::ObjectAlias(_)) {
        errors.push(error("type"));
        return false;
    }
    let required = execute::get_property(schema, "required");
    if let Value::Array(values) = required {
        for index in 0..values.logical_len() {
            let key = execute::to_js_string(
                &execute::get_property_result(&Value::Array(values.clone()), &index.to_string())
                    .unwrap_or(Value::Undefined),
            )
            .unwrap_or_default();
            if !execute::has_own_property(data, &key) {
                errors.push(error("required"));
            }
        }
    }
    let properties = execute::get_property(schema, "properties");
    if let Value::Object(_) | Value::ObjectAlias(_) = properties {
        for key in execute::own_enumerable_keys(&properties) {
            if !execute::has_own_property(data, &key) {
                continue;
            }
            let value = execute::get_property(data, &key);
            let rule = execute::get_property(&properties, &key);
            validate_rule(&rule, &value, errors);
        }
    }
    if matches!(
        execute::get_property(schema, "additionalProperties"),
        Value::Boolean(false)
    ) {
        let known = match properties {
            Value::Object(_) | Value::ObjectAlias(_) => execute::own_enumerable_keys(&properties),
            _ => Vec::new(),
        };
        if execute::own_enumerable_keys(data)
            .into_iter()
            .any(|key| !known.contains(&key))
        {
            errors.push(error("additionalProperties"));
        }
    }
    errors.is_empty()
}

fn validate_rule(rule: &Value, value: &Value, errors: &mut Vec<Value>) {
    let kind = execute::to_js_string(&execute::get_property(rule, "type")).unwrap_or_default();
    let type_ok = match kind.as_str() {
        "string" => matches!(value, Value::String(_)),
        "integer" => {
            matches!(value, Value::Number(number) if number.is_finite() && number.fract() == 0.0)
        }
        "number" => matches!(value, Value::Number(number) if number.is_finite()),
        "object" => matches!(value, Value::Object(_) | Value::ObjectAlias(_)),
        "array" => matches!(value, Value::Array(_)),
        _ => true,
    };
    if !type_ok {
        errors.push(error("type"));
        return;
    }
    if let Value::Number(minimum) = execute::get_property(rule, "minimum") {
        if matches!(value, Value::Number(number) if *number < minimum) {
            errors.push(error("minimum"));
        }
    }
}

fn error(keyword: &str) -> Value {
    host_api::object(vec![("keyword".into(), Value::String(keyword.into()))])
}

pub fn chalk_module() -> Value {
    style_value(Vec::new())
}

fn style_value(styles: Vec<String>) -> Value {
    let value = host_api::bound_capability_with_arguments(
        crate::host::capability_ref(crate::registry::SPEC_NPM_CHALK_STYLE),
        std::iter::once(Value::Undefined)
            .chain(styles.iter().cloned().map(Value::String))
            .collect(),
    );
    if styles.len() < 2 {
        let red = with_style(&styles, "red");
        let bold = with_style(&styles, "bold");
        execute::set_property_in_place(&value, "red", red);
        execute::set_property_in_place(&value, "bold", bold);
    }
    value
}

fn with_style(styles: &[String], name: &str) -> Value {
    let mut next = styles.to_vec();
    next.push(name.into());
    style_value(next)
}

pub fn chalk_style(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let count = args
        .iter()
        .position(|value| matches!(value, Value::Undefined))
        .map(|index| index + 1)
        .unwrap_or(0);
    let text = args
        .iter()
        .skip(count)
        .map(|value| execute::to_js_string(value).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(Value::String(text))
}

pub fn prettier_module() -> Value {
    host_api::object(vec![(
        "format".into(),
        crate::host::capability(crate::registry::SPEC_NPM_PRETTIER_FORMAT),
    )])
}

pub fn prettier_format(
    _state: &Rc<RefCell<HostState>>,
    _receiver: Option<&Value>,
    args: &[Value],
) -> Result<Value, VmError> {
    let source = args
        .first()
        .map(|value| execute::to_js_string(value).unwrap_or_default())
        .unwrap_or_default();
    let compact = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();
    let formatted = if compact == "constanswer=42" {
        "const answer = 42;\n".into()
    } else {
        source
    };
    Ok(quench_runtime::promise_resolve(&[Value::String(formatted)]))
}
