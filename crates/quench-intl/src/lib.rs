mod bigint;
mod calendar;
mod collator;
mod digits;
mod locale_canonicalization;
mod locale_data;
mod locale_case;
mod plural;
mod supported_values;

pub use bigint::{format_bigint, BigIntFormatOptions};
pub use calendar::{
    calendar_date_add, calendar_date_difference, calendar_date_to_iso,
    calendar_date_to_iso_with_overflow, calendar_fields_from_iso, calendar_month_from_code,
    calendar_year_from_era, CalendarDate, CalendarDifferenceUnit,
};
pub use collator::{canonical_locale_identifier, compare_collator, CollatorOptions};
pub use digits::localize_digits;
pub use locale_canonicalization::{
    canonical_region, canonicalize_locale_identifier, language_alias, titlecase_script,
};
pub use locale_data::{
    calendar_alias, default_numbering_system, sanitize_datetime_locale, valid_calendar,
    valid_numbering_system, valid_unicode_type, CALENDARS, NUMBERING_SYSTEMS,
};
pub use locale_case::locale_case;
pub use plural::{
    plural_categories, plural_category, plural_category_compact, plural_category_decimal,
    plural_category_range, plural_category_range_decimal,
};
pub use supported_values::{
    collation_supported, currency_fraction_digits, supported_time_zones, COLLATIONS, CURRENCIES,
    UNITS,
};

pub const NUMBER_FORMAT_OPTION_KEYS: &[&str] = &[
    "localeMatcher",
    "numberingSystem",
    "style",
    "currency",
    "currencyDisplay",
    "currencySign",
    "unit",
    "unitDisplay",
    "notation",
    "minimumIntegerDigits",
    "minimumFractionDigits",
    "maximumFractionDigits",
    "minimumSignificantDigits",
    "maximumSignificantDigits",
    "roundingIncrement",
    "roundingMode",
    "roundingPriority",
    "trailingZeroDisplay",
    "compactDisplay",
    "useGrouping",
    "signDisplay",
];
