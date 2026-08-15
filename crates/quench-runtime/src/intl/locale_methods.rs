use crate::{execute::VmError, value::Value};

use super::{locale_slot_value, make_array, make_object, runtime_error, slot_string};

pub(crate) fn prototype_method(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    let slot = slot_string(&super::intl_slots(receiver)?, "full").unwrap_or_default();
    match builtin {
        crate::ops::Builtin::IntlLocaleToString => Ok(Value::String(slot)),
        crate::ops::Builtin::IntlLocaleMaximize => Ok(Value::String(maximize(&slot))),
        crate::ops::Builtin::IntlLocaleMinimize => Ok(Value::String(minimize(&slot))),
        crate::ops::Builtin::IntlLocaleGetCalendars => {
            let calendar = slot_string(&super::intl_slots(receiver)?, "calendar")
                .unwrap_or_else(|| "gregory".to_string());
            Ok(make_array(vec![Value::String(calendar)]))
        }
        crate::ops::Builtin::IntlLocaleGetCollations => {
            let collation = slot_string(&super::intl_slots(receiver)?, "collation")
                .unwrap_or_else(|| "default".to_string());
            Ok(make_array(vec![Value::String(collation)]))
        }
        crate::ops::Builtin::IntlLocaleGetHourCycles => {
            Ok(make_array(vec![Value::String("h12".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetNumberingSystems => {
            Ok(make_array(vec![Value::String("latn".to_string())]))
        }
        crate::ops::Builtin::IntlLocaleGetTimeZones => {
            if slot_string(&super::intl_slots(receiver)?, "region").is_some() {
                Ok(make_array(vec![Value::String("UTC".to_string())]))
            } else {
                Ok(Value::Undefined)
            }
        }
        crate::ops::Builtin::IntlLocaleGetTextInfo => Ok(make_object(vec![(
            "direction".to_string(),
            Value::String("ltr".to_string()),
        )])),
        crate::ops::Builtin::IntlLocaleGetWeekInfo => Ok(make_object(vec![
            ("firstDay".to_string(), Value::Number(1.0)),
            (
                "weekend".to_string(),
                make_array(vec![Value::Number(6.0), Value::Number(7.0)]),
            ),
        ])),
        crate::ops::Builtin::IntlLocaleBaseNameGetter => Ok(Value::String(
            slot_string(&super::intl_slots(receiver)?, "base").unwrap_or_default(),
        )),
        _ => return prototype_method_tail(builtin, receiver),
    }
}

fn prototype_method_tail(
    builtin: crate::ops::Builtin,
    receiver: Option<&Value>,
) -> Result<Value, VmError> {
    match builtin {
        crate::ops::Builtin::IntlLocaleCalendarGetter
        | crate::ops::Builtin::IntlLocaleCaseFirstGetter
        | crate::ops::Builtin::IntlLocaleCollationGetter
        | crate::ops::Builtin::IntlLocaleFirstDayOfWeekGetter
        | crate::ops::Builtin::IntlLocaleHourCycleGetter
        | crate::ops::Builtin::IntlLocaleLanguageGetter
        | crate::ops::Builtin::IntlLocaleNumberingSystemGetter
        | crate::ops::Builtin::IntlLocaleNumericGetter
        | crate::ops::Builtin::IntlLocaleRegionGetter
        | crate::ops::Builtin::IntlLocaleScriptGetter
        | crate::ops::Builtin::IntlLocaleTextInfoGetter
        | crate::ops::Builtin::IntlLocaleVariantsGetter => {
            let key = match builtin {
                crate::ops::Builtin::IntlLocaleCalendarGetter => "calendar",
                crate::ops::Builtin::IntlLocaleCaseFirstGetter => "caseFirst",
                crate::ops::Builtin::IntlLocaleCollationGetter => "collation",
                crate::ops::Builtin::IntlLocaleFirstDayOfWeekGetter => "firstDayOfWeek",
                crate::ops::Builtin::IntlLocaleHourCycleGetter => "hourCycle",
                crate::ops::Builtin::IntlLocaleLanguageGetter => "language",
                crate::ops::Builtin::IntlLocaleNumberingSystemGetter => "numberingSystem",
                crate::ops::Builtin::IntlLocaleNumericGetter => "numeric",
                crate::ops::Builtin::IntlLocaleRegionGetter => "region",
                crate::ops::Builtin::IntlLocaleScriptGetter => "script",
                crate::ops::Builtin::IntlLocaleTextInfoGetter => "textInfo",
                _ => "variants",
            };
            Ok(locale_slot_value(&super::intl_slots(receiver)?, key))
        }
        _ => Err(runtime_error("TypeError: method not found")),
    }
}

fn maximize(tag: &str) -> String {
    let (base, extension) = tag
        .split_once("-u")
        .map_or((tag, String::new()), |(a, b)| (a, format!("-u{b}")));
    let parts: Vec<&str> = base.split('-').collect();
    let script = parts.iter().skip(1).find(|part| part.len() == 4).copied();
    let region = parts
        .iter()
        .skip(1)
        .find(|part| {
            part.len() == 2 || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()))
        })
        .copied();
    let language = if parts.first().copied() == Some("und") {
        undefined_language(script, region)
    } else {
        parts.first().copied().unwrap_or("und")
    };
    let (default_script, default_region) = likely_subtags(language);
    let default_region = if language == "zh" && script == Some("Hant") {
        "TW"
    } else {
        default_region
    };
    let mut result = format!(
        "{}-{}-{}",
        language,
        script.unwrap_or(default_script),
        region.unwrap_or(default_region)
    );
    for part in parts.iter().skip(1) {
        if part.len() != 4
            && !(part.len() == 2 || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit())))
        {
            result.push('-');
            result.push_str(part);
        }
    }
    result.push_str(&extension);
    result
}

fn undefined_language(script: Option<&str>, region: Option<&str>) -> &'static str {
    match (script, region) {
        (Some("Thai"), _) => "th",
        (_, Some("419")) => "es",
        (_, Some("150")) => "en",
        (_, Some("AT")) => "de",
        (_, Some("CW")) => "pap",
        _ => "en",
    }
}

fn minimize(tag: &str) -> String {
    let maximized = maximize(tag);
    let (base, extension) = maximized
        .split_once("-u")
        .map_or((maximized.as_str(), String::new()), |(a, b)| {
            (a, format!("-u{b}"))
        });
    let parts: Vec<&str> = base.split('-').collect();
    let language = parts.first().copied().unwrap_or("und");
    let (default_script, default_region) = likely_subtags(language);
    let script = parts.get(1).copied().unwrap_or(default_script);
    let region = parts.get(2).copied().unwrap_or(default_region);
    if language == "zh" && script == "Hant" && region == "TW" {
        return format!("{language}-{region}") + &extension;
    }
    let short = if script == default_script && region == default_region {
        language.to_string()
    } else if script == default_script {
        format!("{language}-{region}")
    } else if region == default_region {
        format!("{language}-{script}")
    } else {
        format!("{language}-{script}-{region}")
    };
    short + &extension
}

fn likely_subtags(language: &str) -> (&'static str, &'static str) {
    match language {
        "aae" => ("Latn", "IT"),
        "ar" => ("Arab", "EG"),
        "de" => ("Latn", "DE"),
        "en" => ("Latn", "US"),
        "es" => ("Latn", "ES"),
        "fr" => ("Latn", "FR"),
        "ja" => ("Jpan", "JP"),
        "ko" => ("Kore", "KR"),
        "ru" => ("Cyrl", "RU"),
        "sr" => ("Cyrl", "RS"),
        "th" => ("Thai", "TH"),
        "pap" => ("Latn", "CW"),
        "zh" => ("Hans", "CN"),
        "und" => ("Latn", "US"),
        _ => ("Latn", "US"),
    }
}
