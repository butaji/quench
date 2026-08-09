//! `Intl.PluralRules`.

use crate::{execute::VmError, value::Value};

use super::{default_locale, make_object, resolve_locales, runtime_error, SLOT};

pub(crate) fn construct(arguments: &[Value]) -> Result<Value, VmError> {
    let locales = resolve_locales(arguments)?;
    let locale = locales.first().cloned().unwrap_or_else(default_locale);
    Ok(make_object(vec![
        (
            "select".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlPluralRulesSelect),
        ),
        (
            "resolvedOptions".to_string(),
            Value::Builtin(crate::ops::Builtin::IntlPluralRulesResolvedOptions),
        ),
        (
            SLOT.to_string(),
            make_object(vec![
                ("locale".to_string(), Value::String(locale)),
                ("type".to_string(), Value::String("cardinal".to_string())),
            ]),
        ),
    ]))
}

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        crate::ops::Builtin::IntlPluralRulesSelect => {
            let number = super::number::to_number(arguments.first());
            Ok(Value::String(select(number)))
        }
        crate::ops::Builtin::IntlPluralRulesResolvedOptions => {
            let _ = receiver;
            Ok(make_object(vec![
                ("locale".to_string(), Value::String(default_locale())),
                ("type".to_string(), Value::String("cardinal".to_string())),
            ]))
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn select(number: f64) -> String {
    if number == 1.0 {
        "one"
    } else if number == 2.0 {
        "two"
    } else if number == 0.0 {
        "zero"
    } else {
        "other"
    }
    .to_string()
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlPluralRules => Some(construct(arguments)),
        crate::ops::Builtin::IntlPluralRulesSelect
        | crate::ops::Builtin::IntlPluralRulesResolvedOptions => {
            Some(prototype_method(builtin, arguments, receiver))
        }
        _ => None,
    }
}
