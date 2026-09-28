use icu_collator::provider::CollationTailoringV1;
use icu_collator::{
    options::{AlternateHandling, CaseLevel, CollatorOptions as IcuOptions, Strength},
    preferences::{CollationCaseFirst, CollationNumericOrdering, CollationType},
    Collator, CollatorPreferences,
};
use icu_provider::{
    marker::DataMarkerExt, DataIdentifierBorrowed, DataMarkerAttributes, DataProvider, DataRequest,
};

pub struct CollatorOptions<'a> {
    pub ignore_punctuation: bool,
    pub sensitivity: &'a str,
    pub usage: &'a str,
    pub numeric: bool,
    pub case_first: &'a str,
}

pub fn compare_collator(
    left: &str,
    right: &str,
    locale: &str,
    options: &CollatorOptions<'_>,
) -> f64 {
    icu_ordering(left, right, locale, options).unwrap_or_else(|| {
        lexical_compare(left, right, options.ignore_punctuation, options.sensitivity)
    })
}

fn icu_ordering(
    left: &str,
    right: &str,
    locale: &str,
    options: &CollatorOptions<'_>,
) -> Option<f64> {
    if !options.ignore_punctuation
        && ((left.is_empty() && punctuation_only(right))
            || (right.is_empty() && punctuation_only(left)))
    {
        return None;
    }
    if options.usage == "search" && !provider_has_collation(locale, "search") {
        return None;
    }
    let locale = icu_locale_core::Locale::try_from_str(locale).ok()?;
    let mut preferences = CollatorPreferences::default();
    preferences.locale_preferences = (&locale).into();
    preferences.numeric_ordering = Some(if options.numeric {
        CollationNumericOrdering::True
    } else {
        CollationNumericOrdering::False
    });
    preferences.case_first = Some(match options.case_first {
        "upper" => CollationCaseFirst::Upper,
        "lower" => CollationCaseFirst::Lower,
        _ => CollationCaseFirst::False,
    });
    if options.usage == "search" {
        preferences.collation_type = Some(CollationType::Search);
    } else if let Some(collation) = locale_collation(&locale.to_string()) {
        if let Ok(value) = collation.parse::<icu_locale_core::extensions::unicode::Value>() {
            if let Ok(collation) = CollationType::try_from(&value) {
                preferences.collation_type = Some(collation);
            }
        }
    }
    let collator = Collator::try_new(preferences, icu_options(options)).ok()?;
    Some(ordering_number(collator.compare(left, right)))
}

fn provider_has_collation(locale: &str, collation: &str) -> bool {
    let Ok(locale) = icu_locale_core::Locale::try_from_str(locale) else {
        return false;
    };
    let mut preferences = CollatorPreferences::default();
    preferences.locale_preferences = (&locale).into();
    for attributes in [collation, "standard"] {
        let Ok(attributes) = DataMarkerAttributes::try_from_str(attributes) else {
            continue;
        };
        let data_locale = CollationTailoringV1::make_locale(preferences.locale_preferences);
        let request = DataRequest {
            id: DataIdentifierBorrowed::for_marker_attributes_and_locale(attributes, &data_locale),
            metadata: Default::default(),
        };
        if <icu_collator::provider::Baked as DataProvider<CollationTailoringV1>>::load(
            &icu_collator::provider::Baked,
            request,
        )
        .is_ok()
        {
            return true;
        }
    }
    false
}

fn locale_collation(locale: &str) -> Option<String> {
    let parts = locale.split('-').collect::<Vec<_>>();
    let unicode = parts
        .iter()
        .position(|part| part.eq_ignore_ascii_case("u"))?;
    let collation = parts
        .iter()
        .enumerate()
        .skip(unicode + 1)
        .find(|(_, part)| part.eq_ignore_ascii_case("co"))?
        .0;
    parts
        .get(collation + 1)
        .filter(|value| value.len() != 2)
        .map(|value| (*value).to_owned())
}

fn icu_options(options: &CollatorOptions<'_>) -> IcuOptions {
    let mut icu = IcuOptions::default();
    icu.strength = Some(match options.sensitivity {
        "base" | "case" => Strength::Primary,
        "accent" => Strength::Secondary,
        _ => Strength::Tertiary,
    });
    icu.case_level = (options.sensitivity == "case").then_some(CaseLevel::On);
    icu.alternate_handling = options
        .ignore_punctuation
        .then_some(AlternateHandling::Shifted);
    icu
}

fn lexical_compare(left: &str, right: &str, ignore_punctuation: bool, sensitivity: &str) -> f64 {
    let left = sensitivity_text(left, ignore_punctuation, sensitivity);
    let right = sensitivity_text(right, ignore_punctuation, sensitivity);
    ordering_number(left.cmp(&right))
}

fn sensitivity_text(value: &str, ignore_punctuation: bool, sensitivity: &str) -> String {
    let comparable = if ignore_punctuation {
        value
            .chars()
            .filter(|character| !character.is_ascii_punctuation() && !character.is_whitespace())
            .collect::<String>()
    } else {
        value.to_string()
    };
    let normalized = unicode_normalization::UnicodeNormalization::nfd(comparable.chars())
        .filter(|character| {
            !matches!(sensitivity, "base" | "case") || !is_combining_mark(*character)
        })
        .collect::<String>();
    if matches!(sensitivity, "base" | "accent") {
        normalized.to_lowercase()
    } else {
        normalized
    }
}

fn is_combining_mark(character: char) -> bool {
    ('\u{300}'..='\u{36f}').contains(&character)
}

fn punctuation_only(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_punctuation() || character.is_whitespace())
}

fn ordering_number(ordering: std::cmp::Ordering) -> f64 {
    match ordering {
        std::cmp::Ordering::Less => -1.0,
        std::cmp::Ordering::Equal => 0.0,
        std::cmp::Ordering::Greater => 1.0,
    }
}
