pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    use Builtin::*;
    if let Some(name) = intl_constructor_short_name(builtin) {
        return name;
    }
    if let Some(name) = metadata_builtin_name(builtin) {
        return name.strip_prefix("Intl.").unwrap_or(name);
    }
    match builtin {
        Eval => "eval",
        Escape => "escape",
        Unescape => "unescape",
        EncodeURI => "encodeURI",
        EncodeURIComponent => "encodeURIComponent",
        DecodeURI => "decodeURI",
        DecodeURIComponent => "decodeURIComponent",
        Array => "Array",
        ArrayBuffer => "ArrayBuffer",
        ArrayBufferIsView => "isView",
        Object => "Object",
        String => "String",
        Symbol => "Symbol",
        Number => "Number",
        Date => "Date",
        DateGetYear => "getYear",
        DateSetYear => "setYear",
        RegExp => "RegExp",
        RegExpTest => "test",
        RegExpExec => "exec",
        _ => "",
    }
}

fn intl_constructor_short_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::IntlCollator => "Collator",
        Builtin::IntlDateTimeFormat => "DateTimeFormat",
        Builtin::IntlDisplayNames => "DisplayNames",
        Builtin::IntlDurationFormat => "DurationFormat",
        Builtin::IntlListFormat => "ListFormat",
        Builtin::IntlLocale => "Locale",
        Builtin::IntlNumberFormat => "NumberFormat",
        Builtin::IntlPluralRules => "PluralRules",
        Builtin::IntlRelativeTimeFormat => "RelativeTimeFormat",
        Builtin::IntlSegmenter => "Segmenter",
        _ => return None,
    })
}

fn metadata_builtin_name(builtin: Builtin) -> Option<&'static str> {
    crate::builtin_meta::intl::short_name(builtin)
        .or_else(|| crate::builtin_meta::methods::short_name(builtin))
        .or_else(|| crate::builtin_meta::methods::function_name(builtin))
        .or_else(|| data_view_name::data_view_name(builtin))
        .or_else(|| error_name(builtin))
        .or_else(|| generator_name(builtin))
        .or_else(|| typed_array_name(builtin))
        .or_else(|| crate::builtin_meta::constructor_name(builtin))
}
