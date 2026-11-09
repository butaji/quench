use icu_collator::provider::CollationTailoringV1;
use icu_collator::{
    options::{AlternateHandling, CaseLevel, CollatorOptions as IcuOptions, Strength},
    preferences::{CollationCaseFirst, CollationNumericOrdering, CollationType},
    Collator, CollatorBorrowed, CollatorPreferences,
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

pub fn canonical_locale_identifier(locale: &str) -> Option<String> {
    icu_locale_core::Locale::try_from_str(locale)
        .ok()
        .map(|locale| locale.to_string())
}

pub fn compare_collator(
    left: &str,
    right: &str,
    locale: &str,
    options: &CollatorOptions<'_>,
) -> f64 {
    resolve_collator(locale, options).compare(left, right)
}

/// A collator resolved for one locale and set of options, which its comparisons reuse
/// without looking it up again (`recent_collator`).
pub struct ResolvedCollator {
    collator: Option<std::rc::Rc<RecentCollator>>,
    ignore_punctuation: bool,
    sensitivity: String,
}

/// The collator `compare_collator` compares by under `locale` and `options`.
pub fn resolve_collator(locale: &str, options: &CollatorOptions<'_>) -> ResolvedCollator {
    ResolvedCollator {
        collator: recent_collator(locale, options),
        ignore_punctuation: options.ignore_punctuation,
        sensitivity: options.sensitivity.to_owned(),
    }
}

impl ResolvedCollator {
    /// The order of `left` and `right`: ICU's, or the lexical order where it gives none.
    pub fn compare(&self, left: &str, right: &str) -> f64 {
        self.icu_ordering(left, right)
            .unwrap_or_else(|| lexical_compare(left, right, self.ignore_punctuation, &self.sensitivity))
    }

    fn icu_ordering(&self, left: &str, right: &str) -> Option<f64> {
        if !self.ignore_punctuation
            && ((left.is_empty() && punctuation_only(right)) || (right.is_empty() && punctuation_only(left)))
        {
            return None;
        }
        let collator = self.collator.as_ref()?;
        if let Some(order) = collator.ascii.as_ref().and_then(|weights| weights.compare(left.as_bytes(), right.as_bytes())) {
            return Some(ordering_number(order));
        }
        Some(ordering_number(collator.icu.compare(left, right)))
    }
}

/// A collator with its printable-ASCII weights where they decide its comparisons
/// (`AsciiWeights::of`).
struct RecentCollator {
    icu: CollatorBorrowed<'static>,
    ascii: Option<AsciiWeights>,
}

/// The collation weights of printable ASCII (0x20..=0x7E) under a tertiary-strength
/// collation in which each such character is one collation element with a primary weight,
/// with no contraction among them: the root collation, and English's. Comparing two such
/// strings is then comparing their primary weights in order, the shorter first on a tie, and,
/// for equal primaries, their full weights in order (ASCII has one secondary weight). The
/// weights are ranks the collator itself gives the characters, so they agree with it.
struct AsciiWeights {
    /// Rank of each character's primary weight; 0 for a character the weights do not cover.
    primary: [u8; 128],
    /// Rank of each character among all of them.
    full: [u8; 128],
}

impl AsciiWeights {
    /// Whether the weights decide comparisons under `locale` and `options`: English or root
    /// with no extension, and default options.
    fn covers(locale: &str, options: &CollatorOptions<'_>) -> bool {
        let language = locale.split('-').next().unwrap_or_default();
        let default = !options.ignore_punctuation
            && options.sensitivity == "variant"
            && options.usage == "sort"
            && !options.numeric
            && options.case_first == "false";
        default && matches!(language, "en" | "und") && !locale.contains("-u-")
    }

    fn of(icu: &CollatorBorrowed<'static>, locale: &str, options: &CollatorOptions<'_>) -> Option<Self> {
        if !Self::covers(locale, options) {
            return None;
        }
        let text = |unit: u8| char::from(unit).to_string();
        let mut units = (0x20u8..=0x7E).collect::<Vec<_>>();
        units.sort_by(|left, right| icu.compare(&text(*left), &text(*right)));
        let base = Collator::try_new(CollatorPreferences::default(), {
            let mut base = IcuOptions::default();
            base.strength = Some(Strength::Primary);
            base
        })
        .ok()?;
        let mut weights = Self { primary: [0; 128], full: [0; 128] };
        let mut primary = 0;
        for (index, unit) in units.iter().enumerate() {
            if index > 0 && base.compare(&text(units[index - 1]), &text(*unit)) != std::cmp::Ordering::Equal {
                primary += 1;
            }
            weights.primary[usize::from(*unit)] = primary + 1;
            weights.full[usize::from(*unit)] = index as u8 + 1;
        }
        Some(weights)
    }

    /// The order of `left` and `right`, `None` unless both are printable ASCII.
    fn compare(&self, left: &[u8], right: &[u8]) -> Option<std::cmp::Ordering> {
        let weight = |unit: u8| self.primary.get(usize::from(unit)).copied().filter(|weight| *weight != 0);
        if !left.iter().chain(right).all(|unit| weight(*unit).is_some()) {
            return None;
        }
        let primaries = left.iter().map(|unit| self.primary[usize::from(*unit)]);
        let order = primaries.cmp(right.iter().map(|unit| self.primary[usize::from(*unit)]));
        let full = |unit: &u8| self.full[usize::from(*unit)];
        Some(order.then_with(|| left.iter().map(full).cmp(right.iter().map(full))))
    }
}

fn build_collator(locale: &str, options: &CollatorOptions<'_>) -> Option<CollatorBorrowed<'static>> {
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
    Collator::try_new(preferences, icu_options(options)).ok()
}


/// Collators built recently, most recent first: building one resolves the locale's fallback
/// chain and loads its data, while a program compares strings with only a few collations
/// (`String.prototype.localeCompare` builds the default one for every call). A collator is
/// a pure function of its locale and options, so a cached one compares exactly as a new one.
const RECENT_COLLATORS: usize = 8;

struct CollatorKey {
    locale: String,
    ignore_punctuation: bool,
    sensitivity: String,
    usage: String,
    numeric: bool,
    case_first: String,
}

impl CollatorKey {
    fn matches(&self, locale: &str, options: &CollatorOptions<'_>) -> bool {
        self.locale == locale
            && self.ignore_punctuation == options.ignore_punctuation
            && self.sensitivity == options.sensitivity
            && self.usage == options.usage
            && self.numeric == options.numeric
            && self.case_first == options.case_first
    }
}

thread_local! {
    static COLLATORS: std::cell::RefCell<Vec<(CollatorKey, Option<std::rc::Rc<RecentCollator>>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// `icu_ordering`'s collator for `locale` and `options`, `None` when ICU has none for them.
fn recent_collator(
    locale: &str,
    options: &CollatorOptions<'_>,
) -> Option<std::rc::Rc<RecentCollator>> {
    COLLATORS.with(|recent| {
        let mut recent = recent.borrow_mut();
        if let Some(index) = recent.iter().position(|(key, _)| key.matches(locale, options)) {
            let entry = recent.remove(index);
            let collator = entry.1.clone();
            recent.insert(0, entry);
            return collator;
        }
        let collator = build_collator(locale, options)
            .map(|icu| std::rc::Rc::new(RecentCollator { ascii: AsciiWeights::of(&icu, locale, options), icu }));
        let key = CollatorKey {
            locale: locale.to_owned(),
            ignore_punctuation: options.ignore_punctuation,
            sensitivity: options.sensitivity.to_owned(),
            usage: options.usage.to_owned(),
            numeric: options.numeric,
            case_first: options.case_first.to_owned(),
        };
        recent.truncate(RECENT_COLLATORS - 1);
        recent.insert(0, (key, collator.clone()));
        collator
    })
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
    f64::from(ordering as i8)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ASCII weights order every pair of printable strings up to two characters long,
    /// and longer mixed ones, as the collator does.
    #[test]
    fn ascii_weights_agree_with_the_collator() {
        let options = CollatorOptions { ignore_punctuation: false, sensitivity: "variant", usage: "sort", numeric: false, case_first: "false" };
        for locale in ["en-US", "en", "und"] {
            let icu = build_collator(locale, &options).unwrap();
            let weights = AsciiWeights::of(&icu, locale, &options).unwrap();
            let mut texts = vec![String::new()];
            for first in 0x20u8..=0x7E {
                texts.push(char::from(first).to_string());
                for second in 0x20u8..=0x7E {
                    texts.push([char::from(first), char::from(second)].iter().collect());
                }
            }
            let mut seed = 0x2545_f491_u32;
            for _ in 0..4000 {
                let length = 3 + seed as usize % 6;
                let text = (0..length).map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    char::from(b"aAbB-_ .0129zZ'xX"[seed as usize % 17])
                });
                texts.push(text.collect());
            }
            for (index, left) in texts.iter().enumerate().step_by(23) {
                for right in texts.iter().skip(index % 5).step_by(7) {
                    assert_eq!(weights.compare(left.as_bytes(), right.as_bytes()), Some(icu.compare(left, right)), "{locale}: {left:?} {right:?}");
                }
            }
        }
        assert!(AsciiWeights::of(&build_collator("da", &options).unwrap(), "da", &options).is_none());
    }
}
