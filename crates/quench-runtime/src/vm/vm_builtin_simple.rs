fn is_simple_builtin(builtin: Builtin) -> bool {
    is_simple_conversion(builtin)
        || is_simple_numbers(builtin)
        || is_simple_regexp(builtin)
        || is_simple_errors(builtin)
}

fn is_simple_conversion(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Boolean
            | Builtin::BooleanValueOf
            | Builtin::BooleanToString
            | Builtin::Eval
            | Builtin::Escape
            | Builtin::EncodeURI
            | Builtin::EncodeURIComponent
            | Builtin::DecodeURI
            | Builtin::DecodeURIComponent
            | Builtin::IsFinite
            | Builtin::IsNaN
            | Builtin::SymbolToString
            | Builtin::SymbolValueOf
            | Builtin::SymbolPrototypeToPrimitive
            | Builtin::SymbolDescriptionGetter
            | Builtin::StringToString
            | Builtin::StringValueOf
            | Builtin::BoxedValueOf
            | Builtin::ObjectPrototypeToString
            | Builtin::ObjectPrototypeValueOf
            | Builtin::FunctionPrototypeToString
            | Builtin::FunctionPrototypeValueOf
            | Builtin::Object
            | Builtin::Date
            | Builtin::Function
            | Builtin::AsyncFunction
            | Builtin::GeneratorFunction
            | Builtin::AsyncGeneratorFunction
    )
}

fn is_simple_numbers(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::NumberIsInteger
            | Builtin::NumberIsSafeInteger
            | Builtin::Number
            | Builtin::BigInt
            | Builtin::BigIntAsIntN
            | Builtin::BigIntAsUintN
            | Builtin::BigIntToString
            | Builtin::NumberToString
            | Builtin::NumberValueOf
            | Builtin::BigIntValueOf
            | Builtin::NumberToFixed
            | Builtin::NumberToPrecision
            | Builtin::NumberToExponential
    )
}

fn is_simple_regexp(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::RegExpPrototypeToString
            | Builtin::RegExpCompile
            | Builtin::RegExpLegacyGetter
            | Builtin::RegExpSourceGetter
            | Builtin::RegExpFlagsGetter
            | Builtin::RegExpGlobalGetter
            | Builtin::RegExpIgnoreCaseGetter
            | Builtin::RegExpMultilineGetter
            | Builtin::RegExpDotAllGetter
            | Builtin::RegExpUnicodeGetter
            | Builtin::RegExpStickyGetter
            | Builtin::RegExpHasIndicesGetter
    )
}

fn is_simple_errors(builtin: Builtin) -> bool {
    matches!(
        builtin,
        Builtin::Error
            | Builtin::RangeError
            | Builtin::ReferenceError
            | Builtin::SyntaxError
            | Builtin::EvalError
            | Builtin::URIError
            | Builtin::AggregateError
            | Builtin::TypeError
            | Builtin::ThrowTypeError
            | Builtin::SuppressedError
            | Builtin::ErrorIsError
            | Builtin::ErrorPrototypeToString
            | Builtin::ErrorPrototypeNameGetter
            | Builtin::ErrorPrototypeMessageGetter
            | Builtin::ErrorPrototypeCauseGetter
            | Builtin::ErrorPrototypeStackGetter
            | Builtin::ErrorPrototypeStackSetter
            | Builtin::WeakRefDeref
    )
}
fn execute_simple_builtin(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    if let Some(result) = simple_prelude(builtin, arguments, receiver) {
        return result;
    }
    if let Some(result) = execute_simple_conversion(builtin, arguments, receiver) {
        return result;
    }
    if let Some(result) = execute_simple_number(builtin, arguments, receiver) {
        return result;
    }
    if let Some(result) = execute_simple_regexp(builtin, arguments, receiver) {
        return result;
    }
    if let Some(result) = execute_simple_error(builtin, arguments, receiver) {
        return result;
    }
    Ok(Value::Undefined)
}

fn execute_simple_conversion(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::Boolean => Ok(Value::Boolean(arguments.first().is_some_and(is_truthy))),
        Builtin::BooleanValueOf => boolean_value_of(receiver),
        Builtin::BooleanToString => boolean_to_string(receiver),
        Builtin::Eval => crate::reflect::builtin(builtin, arguments, receiver),
        Builtin::Escape => Ok(crate::builtins::escape(arguments.first())),
        Builtin::EncodeURI => crate::builtins::encode_uri(arguments.first(), true),
        Builtin::EncodeURIComponent => crate::builtins::encode_uri(arguments.first(), false),
        Builtin::DecodeURI => crate::builtins::decode_uri(arguments.first(), true),
        Builtin::DecodeURIComponent => crate::builtins::decode_uri(arguments.first(), false),
        Builtin::IsFinite => is_finite_check(arguments.first(), receiver).map(Value::Boolean),
        Builtin::IsNaN => is_nan_check(arguments.first(), receiver).map(Value::Boolean),
        Builtin::SymbolToString => symbol_to_string(receiver),
        Builtin::SymbolValueOf | Builtin::SymbolPrototypeToPrimitive => symbol_value_of(receiver),
        Builtin::SymbolDescriptionGetter => symbol_description(receiver),
        Builtin::StringToString | Builtin::StringValueOf => string_value_of(receiver),
        Builtin::BoxedValueOf => Ok(boxed_value(receiver)),
        Builtin::ObjectPrototypeToString => Ok(crate::builtins::prototype_to_string(receiver)),
        Builtin::ObjectPrototypeValueOf => crate::builtins::prototype_value_of(receiver),
        Builtin::FunctionPrototypeToString | Builtin::FunctionPrototypeValueOf => {
            function_prototype_builtin(builtin, receiver)
        }
        Builtin::Object => Ok(crate::builtins::object(arguments)),
        Builtin::Date => Ok(crate::date::call()),
        _ => return None,
    };
    Some(result)
}

fn execute_simple_number(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::Number => explicit_number(arguments.first()).map(Value::Number),
        Builtin::BigInt => explicit_bigint(arguments.first()),
        Builtin::BigIntAsIntN | Builtin::BigIntAsUintN => {
            bigint_as_n(arguments, builtin == Builtin::BigIntAsIntN)
        }
        Builtin::BigIntToString => bigint_to_string(receiver, arguments),
        Builtin::NumberToString => boolean_or_number_string(receiver, arguments),
        Builtin::NumberValueOf => number_value_of(receiver),
        Builtin::BigIntValueOf => bigint_value_of(receiver),
        Builtin::NumberToFixed | Builtin::NumberToPrecision | Builtin::NumberToExponential => {
            crate::number_fmt::number_format(receiver, arguments.first(), builtin)
        }
        _ => return None,
    };
    Some(result)
}

fn execute_simple_regexp(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    let result = match builtin {
        Builtin::RegExpPrototypeToString => regexp_prototype_to_string(receiver),
        Builtin::RegExpCompile => crate::regexp::compile_method_for_vm(receiver, arguments),
        Builtin::RegExpLegacyGetter => crate::regexp::legacy_getter(receiver),
        Builtin::RegExpSourceGetter => regexp_prototype_accessor(receiver, "source"),
        Builtin::RegExpFlagsGetter => regexp_prototype_accessor(receiver, "flags"),
        Builtin::RegExpGlobalGetter => regexp_prototype_accessor(receiver, "global"),
        Builtin::RegExpIgnoreCaseGetter => regexp_prototype_accessor(receiver, "ignoreCase"),
        Builtin::RegExpMultilineGetter => regexp_prototype_accessor(receiver, "multiline"),
        Builtin::RegExpDotAllGetter => regexp_prototype_accessor(receiver, "dotAll"),
        Builtin::RegExpUnicodeGetter => regexp_prototype_accessor(receiver, "unicode"),
        Builtin::RegExpStickyGetter => regexp_prototype_accessor(receiver, "sticky"),
        Builtin::RegExpHasIndicesGetter => regexp_prototype_accessor(receiver, "hasIndices"),
        _ => return None,
    };
    Some(result)
}

fn execute_simple_error(
    builtin: Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    if !is_simple_errors(builtin) {
        return None;
    }
    Some(error_builtin(builtin, arguments, receiver))
}
