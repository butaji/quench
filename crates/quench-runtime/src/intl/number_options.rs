fn validate_rounding_mode(value: &str) -> Result<(), VmError> {
    let valid = matches!(
        value,
        "ceil"
            | "floor"
            | "expand"
            | "trunc"
            | "halfCeil"
            | "halfFloor"
            | "halfExpand"
            | "halfTrunc"
            | "halfEven"
    );
    valid
        .then_some(())
        .ok_or_else(|| crate::value::error::throw_range_error("invalid roundingMode"))
}

fn validate_rounding_increment(raw: &RawOptions) -> Result<(), VmError> {
    if raw.rounding_increment != 1.0
        && (!raw.rounding_increment.is_finite()
            || raw.rounding_increment.fract() != 0.0
            || !matches!(
                raw.rounding_increment as u32,
                1 | 2 | 5 | 10 | 20 | 25 | 50 | 100 | 200 | 250 | 500 | 1000 | 2000 | 2500 | 5000
            ))
    {
        return Err(crate::value::error::throw_range_error(
            "invalid roundingIncrement",
        ));
    }
    if raw.rounding_increment == 1.0 {
        return Ok(());
    }
    if raw.minimum_fraction_digits >= 0.0
        && raw.maximum_fraction_digits >= 0.0
        && raw.minimum_fraction_digits != raw.maximum_fraction_digits
    {
        return Err(crate::value::error::throw_range_error(
            "roundingIncrement requires equal fraction digits",
        ));
    }
    if raw.rounding_priority != "auto"
        || raw.minimum_significant_digits >= 0.0
        || raw.maximum_significant_digits >= 0.0
    {
        return Err(crate::value::error::throw_type_error(
            "roundingIncrement requires fraction-digit rounding",
        ));
    }
    Ok(())
}

fn number_options(
    locale: String,
    raw: RawOptions,
    minimum_fraction_digits: f64,
    maximum_fraction_digits: f64,
) -> NumberOptions {
    let locale_numbering = super::numbering_system(&locale).map(str::to_owned);
    let locale = selected_locale(locale, &raw, locale_numbering.as_deref());
    let numbering_system = selected_numbering_system(&raw, locale_numbering.as_deref());
    let currency = raw
        .currency
        .as_ref()
        .filter(|_| raw.style == "currency")
        .cloned();
    let compact_grouping_default = raw.notation == "compact" && !raw.grouping_explicit;
    let grouping = selected_grouping(&raw, compact_grouping_default);
    NumberOptions {
        locale,
        numbering_system,
        style: raw.style,
        currency,
        currency_display: raw.currency_display,
        currency_sign: raw.currency_sign,
        unit: raw.unit,
        unit_display: raw.unit_display,
        grouping,
        minimum_integer_digits: raw.minimum_integer_digits.max(1.0) as u32,
        minimum_fraction_digits: minimum_fraction_digits as u32,
        maximum_fraction_digits: maximum_fraction_digits as u32,
        use_grouping: raw.use_grouping,
        grouping_min2: raw.grouping_min2 || compact_grouping_default,
        notation: raw.notation,
        compact_display: raw.compact_display,
        rounding_mode: raw.rounding_mode,
        rounding_increment: raw.rounding_increment.max(1.0) as u32,
        sign_display: raw.sign_display,
        minimum_significant_digits: significant_digits(raw.minimum_significant_digits)
            .or_else(|| significant_digits(raw.maximum_significant_digits).map(|_| 1)),
        maximum_significant_digits: significant_digits(raw.maximum_significant_digits)
            .or_else(|| significant_digits(raw.minimum_significant_digits).map(|_| 21)),
        rounding_priority: raw.rounding_priority,
        trailing_zero_display: raw.trailing_zero_display,
    }
}

fn selected_numbering_system(raw: &RawOptions, locale_numbering: Option<&str>) -> String {
    raw.numbering_system
        .as_deref()
        .or(locale_numbering)
        .unwrap_or("latn")
        .to_string()
}

fn selected_locale(locale: String, raw: &RawOptions, locale_numbering: Option<&str>) -> String {
    match raw.numbering_system.as_deref() {
        Some(option) if locale_numbering != Some(option) => locale
            .split_once("-u-")
            .map_or(locale.clone(), |(base, _)| base.to_string()),
        _ => super::number_locale(&locale),
    }
}

fn selected_grouping(raw: &RawOptions, compact_grouping_default: bool) -> String {
    if compact_grouping_default {
        "min2".to_string()
    } else {
        raw.grouping.clone()
    }
}
