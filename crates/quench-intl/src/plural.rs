use fixed_decimal::{CompactDecimal, Decimal};
use icu_plurals::{
    PluralCategory, PluralOperands, PluralRuleType, PluralRules, PluralRulesOptions,
    PluralRulesPreferences, PluralRulesWithRanges,
};

pub fn plural_category(locale: &str, rule_type: &str, number: f64) -> Option<&'static str> {
    plural_category_decimal(locale, rule_type, &number.to_string())
}

pub fn plural_category_decimal(
    locale: &str,
    rule_type: &str,
    number: &str,
) -> Option<&'static str> {
    let rules = plural_rules(locale, rule_type)?;
    let decimal = Decimal::try_from_str(number).ok()?;
    let operands = PluralOperands::from(&decimal);
    Some(category_name(rules.category_for(operands)))
}

pub fn plural_category_compact(
    locale: &str,
    rule_type: &str,
    significand: &str,
    exponent: u16,
) -> Option<&'static str> {
    let rules = plural_rules(locale, rule_type)?;
    let decimal = CompactDecimal::try_from_str(&format!("{significand}c{exponent}")).ok()?;
    Some(category_name(rules.category_for(&decimal)))
}

pub fn plural_category_range(
    locale: &str,
    rule_type: &str,
    start: f64,
    end: f64,
) -> Option<&'static str> {
    plural_category_range_decimal(locale, rule_type, &start.to_string(), &end.to_string())
}

pub fn plural_category_range_decimal(
    locale: &str,
    rule_type: &str,
    start: &str,
    end: &str,
) -> Option<&'static str> {
    let rules = plural_range_rules(locale, rule_type)?;
    let start = Decimal::try_from_str(start).ok()?;
    let end = Decimal::try_from_str(end).ok()?;
    let start = PluralOperands::from(&start);
    let end = PluralOperands::from(&end);
    Some(category_name(rules.category_for_range(start, end)))
}

pub fn plural_categories(locale: &str, rule_type: &str) -> Option<Vec<&'static str>> {
    if rule_type == "cardinal" && locale.split('-').next()? == "gv" {
        return Some(vec!["one", "two", "few", "many", "other"]);
    }
    plural_rules(locale, rule_type).map(|rules| rules.categories().map(category_name).collect())
}

fn plural_rules(locale: &str, rule_type: &str) -> Option<PluralRules> {
    let (preferences, rule_type) = plural_preferences(locale, rule_type)?;
    PluralRules::try_new(
        preferences,
        PluralRulesOptions::default().with_type(rule_type),
    )
    .ok()
}

fn plural_range_rules(locale: &str, rule_type: &str) -> Option<PluralRulesWithRanges<PluralRules>> {
    let (preferences, rule_type) = plural_preferences(locale, rule_type)?;
    PluralRulesWithRanges::try_new(
        preferences,
        PluralRulesOptions::default().with_type(rule_type),
    )
    .ok()
}

fn plural_preferences(
    locale: &str,
    rule_type: &str,
) -> Option<(PluralRulesPreferences, PluralRuleType)> {
    let locale = icu_locale_core::Locale::try_from_str(locale).ok()?;
    let rule_type = match rule_type {
        "cardinal" => PluralRuleType::Cardinal,
        "ordinal" => PluralRuleType::Ordinal,
        _ => return None,
    };
    let mut preferences = PluralRulesPreferences::default();
    preferences.locale_preferences = (&locale).into();
    Some((preferences, rule_type))
}

fn category_name(category: PluralCategory) -> &'static str {
    match category {
        PluralCategory::Zero => "zero",
        PluralCategory::One => "one",
        PluralCategory::Two => "two",
        PluralCategory::Few => "few",
        PluralCategory::Many => "many",
        PluralCategory::Other => "other",
    }
}
