pub(crate) fn compare(
    left: &str,
    right: &str,
    locale: &str,
    ignore_punctuation: bool,
    sensitivity: &str,
) -> f64 {
    compare_with_options(
        left,
        right,
        locale,
        &CompareSpec {
            ignore_punctuation,
            sensitivity,
            usage: "sort",
            numeric: false,
            case_first: "false",
        },
    )
}

struct CompareSpec<'a> {
    ignore_punctuation: bool,
    sensitivity: &'a str,
    usage: &'a str,
    numeric: bool,
    case_first: &'a str,
}

fn compare_with_options(left: &str, right: &str, locale: &str, spec: &CompareSpec<'_>) -> f64 {
    if let Some(ordering) = icu_ordering(left, right, locale, spec) {
        return ordering;
    }
    lexical_compare(left, right, spec.ignore_punctuation, spec.sensitivity)
}

fn lexical_compare(left: &str, right: &str, ignore_punctuation: bool, sensitivity: &str) -> f64 {
    let left = sensitivity_text(left, ignore_punctuation, sensitivity);
    let right = sensitivity_text(right, ignore_punctuation, sensitivity);
    match left.cmp(&right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}

fn icu_ordering(left: &str, right: &str, locale: &str, spec: &CompareSpec<'_>) -> Option<f64> {
    if !spec.ignore_punctuation
        && ((left.is_empty() && punctuation_only(right))
            || (right.is_empty() && punctuation_only(left)))
    {
        return None;
    }
    let collation = locale_collation(locale).unwrap_or_else(|| "standard".to_string());
    if spec.usage == "search" && !provider_has_collation(locale, "search") {
        return None;
    }
    let locale = icu_locale_core::Locale::try_from_str(locale).ok()?;
    let preferences = icu_preferences(&locale, spec.usage, spec.numeric, spec.case_first);
    let options = icu_options(spec.ignore_punctuation, spec.sensitivity);
    let collator = Collator::try_new(preferences, options).ok()?;
    Some(match collator.compare(left, right) {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    })
}

fn punctuation_only(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_punctuation() || character.is_whitespace())
}

fn icu_preferences(
    locale: &icu_locale_core::Locale,
    usage: &str,
    numeric: bool,
    case_first: &str,
) -> CollatorPreferences {
    let mut preferences = CollatorPreferences::default();
    preferences.locale_preferences = locale.into();
    preferences.numeric_ordering = Some(if numeric {
        CollationNumericOrdering::True
    } else {
        CollationNumericOrdering::False
    });
    preferences.case_first = Some(match case_first {
        "upper" => CollationCaseFirst::Upper,
        "lower" => CollationCaseFirst::Lower,
        _ => CollationCaseFirst::False,
    });
    if usage == "search" {
        preferences.collation_type = Some(CollationType::Search);
    } else if let Some(collation) = locale_collation(&locale.to_string()) {
        if let Ok(value) = collation.parse::<icu_locale_core::extensions::unicode::Value>() {
            if let Ok(collation_type) = CollationType::try_from(&value) {
                preferences.collation_type = Some(collation_type);
            }
        }
    }
    preferences
}

fn icu_options(ignore_punctuation: bool, sensitivity: &str) -> IcuOptions {
    let mut options = IcuOptions::default();
    options.strength = Some(match sensitivity {
        "base" | "case" => Strength::Primary,
        "accent" => Strength::Secondary,
        _ => Strength::Tertiary,
    });
    options.case_level = (sensitivity == "case").then_some(CaseLevel::On);
    options.alternate_handling = ignore_punctuation.then_some(AlternateHandling::Shifted);
    options
}

fn sensitivity_text(value: &str, ignore_punctuation: bool, sensitivity: &str) -> String {
    let value = comparable_text(value, ignore_punctuation);
    let value = unicode_normalization::UnicodeNormalization::nfd(value.chars())
        .filter(|character| sensitivity != "base" && sensitivity != "case" || !is_mark(*character))
        .collect::<String>();
    if sensitivity == "base" || sensitivity == "accent" {
        value.to_lowercase()
    } else {
        value
    }
}

fn is_mark(character: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&character)
}

fn comparable_text(value: &str, ignore_punctuation: bool) -> String {
    if !ignore_punctuation {
        return value.to_string();
    }
    value
        .chars()
        .filter(|character| !character.is_ascii_punctuation() && !character.is_whitespace())
        .collect()
}
