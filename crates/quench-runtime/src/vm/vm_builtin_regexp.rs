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
            crate::builtins::function_prototype_to_string(receiver)
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
    if matches!(value, Value::Builtin(Builtin::RegExpPrototype))
        && crate::vm::current_context_or_default().realm() == crate::ops::RealmId::ROOT
    {
        return Ok(match key {
            "source" => Value::String("(?:)".to_string()),
            "flags" => Value::String(String::new()),
            _ => Value::Undefined,
        });
    }
    if key == "flags" {
        if !crate::value::is_object(value) {
            return Err(crate::value::error::throw_type_error(
                "RegExp accessor called on incompatible receiver",
            ));
        }
        return regexp_flags(value);
    }
    if key == "source" {
        if !crate::regexp::has_regexp_internal_slot(value)
            || !crate::regexp::is_current_realm(value)
        {
            return Err(crate::value::error::throw_type_error(
                "RegExp accessor called on incompatible receiver",
            ));
        }
        let source = crate::regexp::extract_source(value);
        return Ok(if source.is_empty() {
            Value::String("(?:)".to_string())
        } else {
            crate::strings::source_value(&source)
        });
    }
    if !crate::regexp::has_regexp_internal_slot(value)
        || !crate::regexp::is_current_realm(value)
    {
        return Err(crate::value::error::throw_type_error(
            "RegExp accessor called on incompatible receiver",
        ));
    }
    let flag = match key {
        "global" => 'g',
        "ignoreCase" => 'i',
        "multiline" => 'm',
        "dotAll" => 's',
        "unicode" => 'u',
        "unicodeSets" => 'v',
        "sticky" => 'y',
        "hasIndices" => 'd',
        _ => return Ok(Value::Undefined),
    };
    Ok(Value::Boolean(crate::regexp::extract_flags(value).contains(flag)))
}

fn regexp_flags(value: &Value) -> Result<Value, VmError> {
    let properties = [
        ("hasIndices", 'd'),
        ("global", 'g'),
        ("ignoreCase", 'i'),
        ("multiline", 'm'),
        ("dotAll", 's'),
        ("unicode", 'u'),
        ("unicodeSets", 'v'),
        ("sticky", 'y'),
    ];
    let mut flags = String::new();
    for (key, flag) in properties {
        if crate::execute::is_truthy(&crate::execute::get_property_result(value, key)?) {
            flags.push(flag);
        }
    }
    Ok(Value::String(flags))
}
