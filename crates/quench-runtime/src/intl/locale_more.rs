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

fn locale_slot_value(slots: &[(String, Value)], key: &str) -> Value {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .map_or(Value::Undefined, |(_, value)| value.clone())
}

pub(crate) fn dispatch(
    builtin: crate::ops::Builtin,
    arguments: &[Value],
    receiver: Option<&Value>,
) -> Option<Result<Value, VmError>> {
    match builtin {
        crate::ops::Builtin::IntlLocale => Some(construct(arguments)),
        crate::ops::Builtin::IntlLocaleToString
        | crate::ops::Builtin::IntlLocaleMaximize
        | crate::ops::Builtin::IntlLocaleMinimize
        | crate::ops::Builtin::IntlLocaleGetCalendars
        | crate::ops::Builtin::IntlLocaleGetCollations
        | crate::ops::Builtin::IntlLocaleGetHourCycles
        | crate::ops::Builtin::IntlLocaleGetNumberingSystems
        | crate::ops::Builtin::IntlLocaleGetTimeZones
        | crate::ops::Builtin::IntlLocaleGetTextInfo
        | crate::ops::Builtin::IntlLocaleGetWeekInfo
        | crate::ops::Builtin::IntlLocaleBaseNameGetter
        | crate::ops::Builtin::IntlLocaleCalendarGetter
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
            Some(prototype_method(builtin, receiver))
        }
        _ => None,
    }
}
