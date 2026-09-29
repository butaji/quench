mod bigint;
mod calendar;
mod collator;
mod digits;
mod locale_canonicalization;
mod locale_case;
mod locale_data;
mod plural;
mod supported_values;

pub use bigint::{BigIntFormatOptions, format_bigint};
pub use calendar::{
    CalendarDate, CalendarDifferenceDirection, CalendarDifferenceUnit, calendar_date_add,
    calendar_date_difference, calendar_date_to_iso, calendar_date_to_iso_with_overflow,
    calendar_days_in_month_for_code, calendar_fields_from_iso, calendar_month_code_for_ordinal,
    calendar_month_from_code,
    calendar_reference_date_from_code,
    calendar_uses_eras, calendar_year_from_era, calendar_year_month_reference_date,
    MAX_CALENDAR_MONTHS_PER_YEAR,
};
pub use collator::{CollatorOptions, canonical_locale_identifier, compare_collator};
pub use digits::localize_digits;
pub use locale_canonicalization::{
    canonical_region, canonicalize_locale_identifier, language_alias, titlecase_script,
};
pub use locale_case::locale_case;
pub use locale_data::{
    CALENDARS, NUMBERING_SYSTEMS, calendar_alias, default_numbering_system,
    sanitize_datetime_locale, unicode_extension_value, valid_calendar, valid_numbering_system,
    valid_unicode_type,
};
pub use plural::{
    plural_categories, plural_category, plural_category_compact, plural_category_decimal,
    plural_category_range, plural_category_range_decimal,
};
pub use supported_values::{
    COLLATIONS, CURRENCIES, UNITS, canonical_time_zone_name, collation_supported,
    currency_fraction_digits, supported_time_zones,
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
