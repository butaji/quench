fn range_error(message: &str) -> VmError {
    let arguments = [Value::String(message.to_string())];
    VmError::Thrown(crate::builtins::error(Builtin::RangeError, &arguments))
}
fn function_prototype_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        Builtin::FunctionPrototypeToString => {
            Ok(crate::builtins::function_prototype_to_string(receiver))
        }
        Builtin::FunctionPrototypeValueOf => {
            Ok(crate::builtins::function_prototype_value_of(receiver))
        }
        _ => Ok(Value::Undefined),
    }
}
fn regexp_prototype_to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = receiver.filter(|value| !matches!(value, Value::Null | Value::Undefined))
    else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.toString called on incompatible receiver",
        ));
    };
    let source = crate::execute::get_property(value, "source");
    let flags = crate::execute::get_property(value, "flags");
    let source_str = if let Value::String(s) = &source {
        s.clone()
    } else {
        String::new()
    };
    let flags_str = if let Value::String(s) = &flags {
        s.clone()
    } else {
        String::new()
    };
    Ok(Value::String(format!("/{source_str}/{flags_str}")))
}

fn regexp_prototype_accessor(receiver: Option<&Value>, key: &str) -> Result<Value, VmError> {
    let Some(value) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp accessor called on incompatible receiver",
        ));
    };
    if matches!(value, Value::Builtin(Builtin::RegExpPrototype)) {
        return Ok(Value::String(if key == "source" {
            "(?:)".to_string()
        } else {
            String::new()
        }));
    }
    if !crate::regexp::has_regexp_internal_slot(value) {
        return Err(crate::value::error::throw_type_error(
            "RegExp accessor called on incompatible receiver",
        ));
    }
    Ok(crate::execute::get_property(value, key))
}
