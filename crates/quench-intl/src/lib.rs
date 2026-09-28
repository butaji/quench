mod bigint;
mod calendar;
mod collator;
mod digits;
mod locale_canonicalization;
mod locale_data;
mod supported_values;

pub use bigint::{format_bigint, BigIntFormatOptions};
pub use calendar::{
    calendar_date_to_iso, calendar_date_to_iso_with_overflow, calendar_fields_from_iso,
    CalendarDate,
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
pub use supported_values::{collation_supported, supported_time_zones, COLLATIONS, CURRENCIES, UNITS};

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
