mod bigint;
mod calendar;
mod collator;
mod digits;
mod locale_data;

pub use bigint::{BigIntFormatOptions, format_bigint};
pub use calendar::{
    CalendarDate, calendar_date_to_iso, calendar_date_to_iso_with_overflow,
    calendar_fields_from_iso,
};
pub use collator::{CollatorOptions, compare_collator};
pub use digits::localize_digits;
pub use locale_data::{
    CALENDARS, NUMBERING_SYSTEMS, calendar_alias, default_numbering_system,
    sanitize_datetime_locale, valid_calendar,
    valid_numbering_system, valid_unicode_type,
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
