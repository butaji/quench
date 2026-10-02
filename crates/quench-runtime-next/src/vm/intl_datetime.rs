use super::intl_datetime_parts::{self, DateTimeFields, DateTimePartOptions, TemporalKind};
use super::*;
use chrono::{FixedOffset, NaiveDate, Offset, TimeZone, Utc};
use std::str::FromStr;

const DATE_TIME_FORMAT_OPTIONS_SLOT: &str = "\0rqj:intl-datetime-options";
const DATE_TIME_FORMAT_RESOLVED_SLOT: &str = "\0rqj:intl-datetime-resolved-options";
const DATE_TIME_FORMAT_DATE_SLOT: &str = "\0rqj:intl-datetime-date";
const DATE_TIME_FORMAT_TIME_SLOT: &str = "\0rqj:intl-datetime-time";
const DATE_TIME_FORMAT_BOUND_SLOT: &str = "\0rqj:intl-datetime-bound-format";
const DATE_TIME_FORMAT_TEMPORAL_DEFAULTS_SLOT: &str = "\0rqj:intl-datetime-temporal-defaults";
const INTL_LEGACY_CONSTRUCTED_SYMBOL: &str = "IntlLegacyConstructedSymbol";
const DEFAULT_TIME_ZONE_SHORT_NAME: &str = "GMT-5";
const DEFAULT_TIME_ZONE_LONG_NAME: &str = "Peru Standard Time";
const DEFAULT_TIME_ZONE_LONG_OFFSET: &str = "GMT-05:00";
const DEFAULT_TIME_ZONE: &str = "America/Lima";
const NANOSECONDS_PER_MILLISECOND: f64 = 1_000_000.0;
const MILLISECONDS_PER_SECOND: i64 = 1_000;
const SECONDS_PER_HOUR: i64 = 3_600;
const MILLISECONDS_PER_HOUR: i64 = SECONDS_PER_HOUR * MILLISECONDS_PER_SECOND;
const HOURS_PER_DAY_I64: i64 = 24;
const MILLISECONDS_PER_DAY: i64 = MILLISECONDS_PER_HOUR * HOURS_PER_DAY_I64;
const SECONDS_PER_MINUTE: i32 = 60;
const MINUTES_PER_HOUR: i32 = 60;
const DEFAULT_TIME_ZONE_OFFSET_MINUTES: i32 = -300;
const TIME_ZONE_SIGN_LENGTH: usize = 1;
const TIME_ZONE_HOUR_DIGITS: usize = 2;
const TIME_ZONE_MINUTE_DIGITS: usize = 2;
const TIME_ZONE_COMPACT_DIGITS: usize = TIME_ZONE_HOUR_DIGITS + TIME_ZONE_MINUTE_DIGITS;
const TIME_ZONE_COLON_DIGITS: usize = TIME_ZONE_COMPACT_DIGITS + 1;
const MAX_TIME_ZONE_HOUR: i32 = 23;
const MAX_TIME_ZONE_MINUTE: i32 = 59;
const DATE_TIME_OPTIONS: &[(&str, &[&str])] = &[
    ("localeMatcher", &["lookup", "best fit"]),
    ("calendar", &[]),
    ("numberingSystem", &[]),
    ("hour12", &[]),
    ("hourCycle", &["h11", "h12", "h23", "h24"]),
    ("timeZone", &[]),
    ("weekday", &["narrow", "short", "long"]),
    ("era", &["narrow", "short", "long"]),
    ("year", &["numeric", "2-digit"]),
    ("month", &["numeric", "2-digit", "narrow", "short", "long"]),
    ("day", &["numeric", "2-digit"]),
    ("dayPeriod", &["narrow", "short", "long"]),
    ("hour", &["numeric", "2-digit"]),
    ("minute", &["numeric", "2-digit"]),
    ("second", &["numeric", "2-digit"]),
    ("fractionalSecondDigits", &[]),
    (
        "timeZoneName",
        &[
            "short",
            "long",
            "shortOffset",
            "longOffset",
            "shortGeneric",
            "longGeneric",
        ],
    ),
    ("formatMatcher", &["basic", "best fit"]),
    ("dateStyle", &["full", "long", "medium", "short"]),
    ("timeStyle", &["full", "long", "medium", "short"]),
];

#[derive(Clone, Copy)]
enum DateTimeDefaults {
    Format,
    Date,
    Time,
    DateAndTime,
    Temporal(&'static [(&'static str, &'static str)]),
    TemporalZonedDateTime(&'static [(&'static str, &'static str)]),
}

impl DateTimeDefaults {
    fn has_time_components(self) -> bool {
        match self {
            Self::Time | Self::DateAndTime => true,
            Self::Temporal(components) | Self::TemporalZonedDateTime(components) => components
                .iter()
                .any(|(name, _)| matches!(*name, "hour" | "minute" | "second")),
            Self::Format | Self::Date => false,
        }
    }
}

const NUMERIC: &str = "numeric";
const PLAIN_DATE_DEFAULTS: &[(&str, &str)] = &[
    ("year", NUMERIC),
    ("month", NUMERIC),
    ("day", NUMERIC),
];
const PLAIN_MONTH_DAY_DEFAULTS: &[(&str, &str)] = &[("month", NUMERIC), ("day", NUMERIC)];
const PLAIN_YEAR_MONTH_DEFAULTS: &[(&str, &str)] = &[("year", NUMERIC), ("month", NUMERIC)];
const PLAIN_TIME_DEFAULTS: &[(&str, &str)] = &[
    ("hour", NUMERIC),
    ("minute", NUMERIC),
    ("second", NUMERIC),
];
const DATE_TIME_DEFAULTS: &[(&str, &str)] = &[
    ("year", NUMERIC),
    ("month", NUMERIC),
    ("day", NUMERIC),
    ("hour", NUMERIC),
    ("minute", NUMERIC),
    ("second", NUMERIC),
];
const ZONED_DATE_TIME_DEFAULTS: &[(&str, &str)] = &[
    ("year", NUMERIC),
    ("month", NUMERIC),
    ("day", NUMERIC),
    ("hour", NUMERIC),
    ("minute", NUMERIC),
    ("second", NUMERIC),
];

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_date_time_format_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlDateTimeFormat, global, global);
        self.realm.intrinsics.intl_datetime_format_constructors
            .insert(global, constructor);
        self.set_builtin_function_name(constructor, "DateTimeFormat")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_datetime_format_prototypes
            .insert(global, prototype);
        let fallback_symbol = self
            .heap
            .alloc(Cell::Symbol(Some(INTL_LEGACY_CONSTRUCTED_SYMBOL.into())));
        self.realm.intrinsics.intl_datetime_format_fallback_symbols
            .insert(global, fallback_symbol);
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        set_non_writable_property(self, constructor, "prototype");
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.DateTimeFormat")?;
        let getter = self.native_with_realm(Native::IntlDateTimeFormatFormatGetter, global, global);
        self.set_builtin_function_name(getter, "get format")?;
        let format_atom = self.intern_atom("format");
        self.set_builtin_value_named(prototype, "format", getter)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(format_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter: None,
            },
        );
        let resolved =
            self.native_with_realm(Native::IntlDateTimeFormatResolvedOptions, global, global);
        self.set_builtin_function_name(resolved, "resolvedOptions")?;
        self.set_builtin_value_named(prototype, "resolvedOptions", resolved)?;
        for (name, native) in [
            ("formatToParts", Native::IntlDateTimeFormatFormatToParts),
            ("formatRange", Native::IntlDateTimeFormatFormatRange),
            (
                "formatRangeToParts",
                Native::IntlDateTimeFormatFormatRangeToParts,
            ),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        let supported =
            self.native_with_realm(Native::IntlDateTimeFormatSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.set_builtin_value_named(intl, "DateTimeFormat", constructor)
    }

    pub(super) fn intl_date_time_format_call(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([this]), |vm| {
            let constructor = vm
                .realm
                .intrinsics
                .intl_datetime_format_constructors
                .get(&vm.realm.globals)
                .copied()
                .ok_or_else(|| JsError("Intl.DateTimeFormat intrinsic is not installed".into()))?;
            let formatter = vm.intl_date_time_format_construct(p, args, constructor)?;
            vm.chain_date_time_format(p, this, formatter)
        })
    }

    pub(super) fn intl_date_time_format_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([new_target]), |vm| {
            let prototype = vm.intl_instance_prototype(p, new_target, Native::IntlDateTimeFormat)?;
            vm.with_call_roots([prototype], |vm| {
                let locale = vm.collator_locale(p, args.first().copied())?;
                let options =
                    vm.date_time_options(p, args.get(1).copied(), DateTimeDefaults::Format, &locale)?;
                vm.with_call_roots([options.2], |vm| {
                    let locale = vm
                        .date_time_option(options.2, "\0locale")
                        .and_then(|value| vm.string_value(value))
                        .unwrap_or(locale);

                    let formatter = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
                    let locale_value = vm.heap.alloc(Cell::String(locale.into()));
                    vm.set_date_time_slot(formatter, DATE_TIME_FORMAT_OPTIONS_SLOT, locale_value)?;
                    vm.set_date_time_slot(
                        formatter,
                        DATE_TIME_FORMAT_DATE_SLOT,
                        if options.0 { Value::TRUE } else { Value::FALSE },
                    )?;
                    vm.set_date_time_slot(
                        formatter,
                        DATE_TIME_FORMAT_TIME_SLOT,
                        if options.1 { Value::TRUE } else { Value::FALSE },
                    )?;
                    vm.set_date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT, options.2)?;
                    Ok(formatter)
                })
            })
        })
    }

    fn date_time_options(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
        defaults: DateTimeDefaults,
        locale: &str,
    ) -> Result<(bool, bool, Value), JsError> {
        let options = match options.filter(|value| !value.is_undefined()) {
            Some(value) if value.is_null() => {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            Some(value) => self.box_object(value)?,
            None => self
                .heap
                .alloc(Cell::Object(Self::empty_object(Value::NULL))),
        };
        let locale = quench_intl::sanitize_datetime_locale(locale);
        let resolved = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        self.with_call_roots([options, resolved], |vm| {
            vm.date_time_options_from(p, options, defaults, &locale, resolved)
        })
    }

    fn date_time_options_from(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        defaults: DateTimeDefaults,
        locale: &str,
        resolved: Value,
    ) -> Result<(bool, bool, Value), JsError> {
        let locale = locale.to_owned();
        let mut has_date = false;
        let mut has_time = false;
        let mut any = false;
        let locale_calendar = locale_unicode_value(&locale, "ca")
            .map(|calendar| quench_intl::calendar_alias(&calendar))
            .filter(|calendar| quench_intl::valid_calendar(calendar))
            .unwrap_or_else(|| "gregory".into());
        let locale_numbering_system = locale_unicode_value(&locale, "nu")
            .filter(|system| quench_intl::valid_numbering_system(system))
            .unwrap_or_else(|| quench_intl::default_numbering_system(&locale).into());
        let calendar_value = self
            .heap
            .alloc(Cell::String(locale_calendar.clone().into()));
        self.set_date_time_property(resolved, "calendar", calendar_value)?;
        let numbering_value = self
            .heap
            .alloc(Cell::String(locale_numbering_system.clone().into()));
        self.set_date_time_property(resolved, "numberingSystem", numbering_value)?;
        let temporal_zoned = matches!(defaults, DateTimeDefaults::TemporalZonedDateTime(_));
        if temporal_zoned {
            let time_zone_atom = self.intern_atom("timeZone");
            if !self
                .get_property(p, options, time_zone_atom)?
                .is_undefined()
            {
                return Err(self.type_error(p, "timeZone option is not allowed".into()));
            }
        }
        for (key, allowed) in DATE_TIME_OPTIONS {
            if temporal_zoned && *key == "timeZone" {
                continue;
            }
            let atom = self.intern_atom(key);
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                continue;
            }
            any = true;
            let normalized = match key.as_ref() {
                "localeMatcher" | "formatMatcher" | "hourCycle" | "weekday" | "era" | "year"
                | "month" | "day" | "dayPeriod" | "hour" | "minute" | "second" | "timeZoneName"
                | "dateStyle" | "timeStyle" => {
                    let text = self.to_string(p, value)?;
                    if !allowed.contains(&text.as_str()) {
                        return Err(self.range_error(p, format!("invalid {key}").into()));
                    }
                    match *key {
                        "year" | "month" | "day" | "weekday" | "era" => has_date = true,
                        "dateStyle" => has_date = true,
                        "timeStyle" => has_time = true,
                        "hourCycle" | "localeMatcher" | "formatMatcher" | "timeZoneName" => {}
                        _ => has_time = true,
                    }
                    if matches!(*key, "localeMatcher" | "formatMatcher") {
                        None
                    } else {
                        Some(self.heap.alloc(Cell::String(text.into())))
                    }
                }
                "timeZone" => {
                    let zone = self.to_string(p, value)?;
                    let Some(zone) = normalize_time_zone_identifier(&zone) else {
                        return Err(self.range_error(p, "invalid timeZone".into()));
                    };
                    Some(self.heap.alloc(Cell::String(zone.into())))
                }
                "hour12" => Some(if self.truthy(value) {
                    Value::TRUE
                } else {
                    Value::FALSE
                }),
                "fractionalSecondDigits" => {
                    let digits = self.to_number(p, value)?;
                    if !digits.is_finite() || !(1.0..=3.0).contains(&digits) {
                        return Err(self.range_error(p, "invalid fractionalSecondDigits".into()));
                    }
                    Some(Value::number(digits.floor()))
                }
                "calendar" => {
                    let text = self.to_string(p, value)?;
                    if !quench_intl::valid_unicode_type(&text) {
                        return Err(self.range_error(p, format!("invalid {key}").into()));
                    }
                    let calendar = quench_intl::calendar_alias(&text);
                    quench_intl::valid_calendar(&calendar)
                        .then(|| self.heap.alloc(Cell::String(calendar.into())))
                }
                "numberingSystem" => {
                    let text = self.to_string(p, value)?.to_ascii_lowercase();
                    if !quench_intl::valid_unicode_type(&text) {
                        return Err(self.range_error(p, format!("invalid {key}").into()));
                    }
                    quench_intl::valid_numbering_system(&text)
                        .then(|| self.heap.alloc(Cell::String(text.into())))
                }
                _ => None,
            };
            if let Some(normalized) = normalized {
                let atom = self.intern_atom(key);
                self.set_property(resolved, atom, normalized)?;
            }
        }
        let fractional_seconds_only =
            self.date_time_option(resolved, "fractionalSecondDigits").is_some()
                && !has_date
                && !has_time;
        if fractional_seconds_only && defaults.has_time_components() {
            self.set_default_time_components(resolved)?;
            has_time = true;
        }
        let temporal_defaults = matches!(
            defaults,
            DateTimeDefaults::Format
                | DateTimeDefaults::Temporal(_)
                | DateTimeDefaults::TemporalZonedDateTime(_)
        ) && (!any || (!has_date && !has_time));
        let has_date_style = self.date_time_option(resolved, "dateStyle").is_some();
        let has_time_style = self.date_time_option(resolved, "timeStyle").is_some();
        match defaults {
            DateTimeDefaults::Format if !any || (!has_date && !has_time) => {
                has_date = true;
                self.set_default_date_components(resolved)?;
            }
            DateTimeDefaults::Date if !has_date && !has_date_style => {
                has_date = true;
                self.set_default_date_components(resolved)?;
            }
            DateTimeDefaults::Time if !has_time && !has_time_style => {
                has_time = true;
                self.set_default_time_components(resolved)?;
            }
            DateTimeDefaults::DateAndTime => {
                if !has_date && !has_time && !has_date_style && !has_time_style {
                    has_date = true;
                    self.set_default_date_components(resolved)?;
                    has_time = true;
                    self.set_default_time_components(resolved)?;
                }
            }
            DateTimeDefaults::Temporal(components)
            | DateTimeDefaults::TemporalZonedDateTime(components)
                if !has_date && !has_time && !has_date_style && !has_time_style =>
            {
                for (component, value) in components {
                    let value = self.heap.alloc(Cell::String((*value).into()));
                    self.set_date_time_property(resolved, component, value)?;
                    if matches!(*component, "year" | "month" | "day") {
                        has_date = true;
                    } else {
                        has_time = true;
                    }
                }
                if temporal_zoned && !any {
                    let time_zone_name = self.heap.alloc(Cell::String("short".into()));
                    self.set_date_time_property(resolved, "timeZoneName", time_zone_name)?;
                }
            }
            _ => {}
        }
        if temporal_defaults {
            self.set_date_time_property(
                resolved,
                DATE_TIME_FORMAT_TEMPORAL_DEFAULTS_SLOT,
                Value::TRUE,
            )?;
        }
        let mut resolved_locale = locale.clone();
        if self
            .date_time_option(resolved, "calendar")
            .and_then(|value| self.string_value(value))
            .is_some_and(|calendar| calendar != locale_calendar)
        {
            resolved_locale = remove_locale_unicode_key(&resolved_locale, "ca");
        }
        if self
            .date_time_option(resolved, "numberingSystem")
            .and_then(|value| self.string_value(value))
            .is_some_and(|system| system != locale_numbering_system)
        {
            resolved_locale = remove_locale_unicode_key(&resolved_locale, "nu");
        }
        if self.date_time_option(resolved, "hour").is_some()
            || self.date_time_option(resolved, "timeStyle").is_some()
            || self.date_time_option(resolved, "hour12").is_some()
            || self.date_time_option(resolved, "hourCycle").is_some()
        {
            self.resolve_date_time_hour_cycle(resolved, &mut resolved_locale)?;
        }
        let locale_atom = self.intern_atom("\0locale");
        let locale_value = self.heap.alloc(Cell::String(resolved_locale.into()));
        self.set_property(resolved, locale_atom, locale_value)?;
        if self.date_time_option(resolved, "dateStyle").is_some()
            || self.date_time_option(resolved, "timeStyle").is_some()
        {
            let forbidden_style = match defaults {
                DateTimeDefaults::Date => Some("timeStyle"),
                DateTimeDefaults::Time => Some("dateStyle"),
                DateTimeDefaults::Temporal(_) if !defaults.has_time_components() => {
                    Some("timeStyle")
                }
                DateTimeDefaults::Temporal(components)
                    if !components
                        .iter()
                        .any(|(name, _)| matches!(*name, "year" | "month" | "day")) =>
                {
                    Some("dateStyle")
                }
                _ => None,
            };
            if forbidden_style.is_some_and(|style| self.date_time_option(resolved, style).is_some())
            {
                return Err(self.type_error(
                    p,
                    "style is incompatible with required date/time components".into(),
                ));
            }

            let explicit = [
                "weekday",
                "era",
                "year",
                "month",
                "day",
                "dayPeriod",
                "hour",
                "minute",
                "second",
                "fractionalSecondDigits",
                "timeZoneName",
            ];
            if explicit
                .iter()
                .any(|key| self.date_time_option(resolved, key).is_some())
            {
                return Err(
                    self.type_error(p, "dateStyle/timeStyle with explicit components".into())
                );
            }
        }
        Ok((has_date, has_time, resolved))
    }

    fn set_default_date_components(&mut self, options: Value) -> Result<(), JsError> {
        let numeric = self.heap.alloc(Cell::String("numeric".into()));
        for name in ["year", "month", "day"] {
            self.set_date_time_property(options, name, numeric)?;
        }
        Ok(())
    }

    fn set_default_time_components(&mut self, options: Value) -> Result<(), JsError> {
        let numeric = self.heap.alloc(Cell::String("numeric".into()));
        for name in ["hour", "minute", "second"] {
            self.set_date_time_property(options, name, numeric)?;
        }
        Ok(())
    }

    fn resolve_date_time_hour_cycle(
        &mut self,
        options: Value,
        locale: &mut String,
    ) -> Result<(), JsError> {
        let option_cycle = self
            .date_time_option(options, "hourCycle")
            .and_then(|value| self.string_value(value));
        let hour12 = self
            .date_time_option(options, "hour12")
            .and_then(Value::as_bool);
        let extension_cycle = locale_unicode_value(locale, "hc");
        let cycle = if let Some(hour12) = hour12 {
            if hour12 {
                if locale.starts_with("ja") {
                    "h11"
                } else {
                    "h12"
                }
            } else {
                "h23"
            }
        } else if let Some(cycle) = option_cycle.as_deref() {
            cycle
        } else if let Some(cycle) = extension_cycle.as_deref() {
            cycle
        } else if locale.starts_with("ja") {
            "h11"
        } else if locale.starts_with("en") {
            "h12"
        } else {
            "h23"
        };
        if hour12.is_some()
            || extension_cycle
                .as_deref()
                .is_some_and(|value| value != cycle)
        {
            *locale = remove_locale_unicode_key(locale, "hc");
        }
        let cycle_value = self.heap.alloc(Cell::String(cycle.into()));
        self.set_date_time_property(options, "hourCycle", cycle_value)?;
        let is_hour12 = matches!(cycle, "h11" | "h12");
        let hour12_value = if hour12.unwrap_or(is_hour12) {
            Value::TRUE
        } else {
            Value::FALSE
        };
        self.set_date_time_property(options, "hour12", hour12_value)?;
        Ok(())
    }

    pub(super) fn date_to_locale_string(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !matches!(self.heap.get(this), Some(Cell::Date { .. })) {
            return Err(self.type_error(p, "Date method called on incompatible receiver".into()));
        }
        if matches!(self.heap.get(this), Some(Cell::Date { milliseconds, .. }) if !milliseconds.is_finite())
        {
            return Ok(self.heap.alloc(Cell::String("Invalid Date".into())));
        }
        let constructor = self.realm.intrinsics.intl_datetime_format_constructors
            .get(&self.realm.globals)
            .copied()
            .ok_or_else(|| JsError("Intl.DateTimeFormat intrinsic is not installed".into()))?;
        let defaults = match native {
            Native::DateToLocaleDateString => DateTimeDefaults::Date,
            Native::DateToLocaleTimeString => DateTimeDefaults::Time,
            _ => DateTimeDefaults::DateAndTime,
        };
        let locale_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let locale = self.collator_locale(p, Some(locale_value))?;
        let options = self.date_time_options(p, args.get(1).copied(), defaults, &locale)?;
        let formatter = self.date_time_formatter(p, constructor, locale, options)?;
        self.date_time_format(p, formatter, &[this])
    }

    pub(super) fn temporal_to_locale_string(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (defaults, zoned) = if self.temporal_plain_time_fields(this).is_some() {
            (DateTimeDefaults::Temporal(PLAIN_TIME_DEFAULTS), None)
        } else {
            match self.heap.get(this) {
                Some(Cell::TemporalPlainDate { .. }) => {
                    (DateTimeDefaults::Temporal(PLAIN_DATE_DEFAULTS), None)
                }
                Some(Cell::TemporalPlainDateTime { .. }) => {
                    (DateTimeDefaults::Temporal(DATE_TIME_DEFAULTS), None)
                }
                Some(Cell::TemporalInstant { .. }) => {
                    (DateTimeDefaults::Temporal(DATE_TIME_DEFAULTS), None)
                }
                Some(Cell::TemporalPlainMonthDay { .. }) => {
                    (DateTimeDefaults::Temporal(PLAIN_MONTH_DAY_DEFAULTS), None)
                }
                Some(Cell::TemporalPlainYearMonth { .. }) => {
                    (DateTimeDefaults::Temporal(PLAIN_YEAR_MONTH_DEFAULTS), None)
                }
                Some(Cell::TemporalZonedDateTime {
                    epoch_nanoseconds,
                    time_zone,
                    calendar,
                    ..
                }) => (
                    DateTimeDefaults::TemporalZonedDateTime(ZONED_DATE_TIME_DEFAULTS),
                    Some((*epoch_nanoseconds, time_zone.clone(), calendar.clone())),
                ),
                _ => return Err(self.type_error(p, "Invalid Temporal value".into())),
            }
        };
        let constructor = self.realm.intrinsics.intl_datetime_format_constructors
            .get(&self.realm.globals)
            .copied()
            .ok_or_else(|| JsError("Intl.DateTimeFormat intrinsic is not installed".into()))?;
        let locale = self.collator_locale(p, args.first().copied())?;
        let options = self.date_time_options(
            p,
            args.get(1).copied(),
            defaults,
            &locale,
        )?;
        let formatter = self.date_time_formatter(p, constructor, locale, options)?;
        if let Some((epoch_nanoseconds, time_zone, calendar)) = zoned {
            let resolved = self
                .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
                .unwrap_or(Value::UNDEFINED);
            let formatter_calendar = self
                .date_time_option(resolved, "calendar")
                .and_then(|value| self.string_value(value))
                .unwrap_or_else(|| "gregory".into());
            let calendar = quench_intl::calendar_alias(&calendar);
            let formatter_calendar = quench_intl::calendar_alias(&formatter_calendar);
            if calendar != "iso8601" && calendar != formatter_calendar {
                return Err(self.range_error(p, "Temporal calendar does not match formatter calendar".into()));
            }
            let time_zone = normalize_time_zone_identifier(&time_zone)
                .ok_or_else(|| self.range_error(p, "Invalid time zone".into()))?;
            let time_zone = self.heap.alloc(Cell::String(time_zone.into()));
            self.set_date_time_property(resolved, "timeZone", time_zone)?;
            let instant = self.heap.alloc(Cell::TemporalInstant {
                object: Box::new(Self::empty_object(self.object_proto)),
                epoch_nanoseconds,
            });
            self.active_call_roots.push(instant);
            let result = self.date_time_format(p, formatter, &[instant]);
            self.active_call_roots.pop();
            result
        } else {
            self.date_time_format(p, formatter, &[this])
        }
    }

    fn date_time_formatter(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        locale: String,
        options: (bool, bool, Value),
    ) -> Result<Value, JsError> {
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, constructor, prototype_atom)?;
        let formatter = self
            .heap
            .alloc(Cell::Object(Self::empty_object(prototype)));
        let locale = self
            .date_time_option(options.2, "\0locale")
            .and_then(|value| self.string_value(value))
            .unwrap_or(locale);
        let locale = self.heap.alloc(Cell::String(locale.into()));
        self.set_date_time_slot(formatter, DATE_TIME_FORMAT_OPTIONS_SLOT, locale)?;
        self.set_date_time_slot(
            formatter,
            DATE_TIME_FORMAT_DATE_SLOT,
            if options.0 { Value::TRUE } else { Value::FALSE },
        )?;
        self.set_date_time_slot(
            formatter,
            DATE_TIME_FORMAT_TIME_SLOT,
            if options.1 { Value::TRUE } else { Value::FALSE },
        )?;
        self.set_date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT, options.2)?;
        Ok(formatter)
    }

    pub(super) fn intl_date_time_format_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::IntlDateTimeFormatSupportedLocalesOf {
            return self.intl_supported_locales_of(
                p,
                args,
                super::intl_number::is_supported_locale,
            );
        }
        let receiver = self.unwrap_date_time_format_receiver(p, this)?;
        match native {
            Native::IntlDateTimeFormatFormatGetter => self.date_time_format_getter(p, receiver),
            Native::IntlDateTimeFormatFormat => self.date_time_format(p, receiver, args),
            Native::IntlDateTimeFormatFormatToParts => {
                self.date_time_format_to_parts(p, receiver, args)
            }
            Native::IntlDateTimeFormatFormatRange
            | Native::IntlDateTimeFormatFormatRangeToParts => {
                self.date_time_format_range(p, native, receiver, args)
            }
            Native::IntlDateTimeFormatResolvedOptions => {
                self.date_time_resolved_options(p, receiver)
            }
            _ => Err(JsError("invalid Intl.DateTimeFormat method".into())),
        }
    }

    fn chain_date_time_format(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        formatter: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([receiver, formatter], |vm| {
            if !vm.is_object_like(receiver) {
                return Ok(formatter);
            }
            let Some(realm) = vm.intl_datetime_format_receiver_realm(p, receiver)? else {
                return Ok(formatter);
            };
            let fallback = if vm.date_time_locale(receiver).is_some() {
                receiver
            } else {
                for slot in [
                    DATE_TIME_FORMAT_OPTIONS_SLOT,
                    DATE_TIME_FORMAT_DATE_SLOT,
                    DATE_TIME_FORMAT_TIME_SLOT,
                    DATE_TIME_FORMAT_RESOLVED_SLOT,
                ] {
                    if let Some(value) = vm.date_time_slot(formatter, slot) {
                        vm.set_date_time_slot(receiver, slot, value)?;
                    }
                }
                formatter
            };
            let symbol = vm.realm.intrinsics.intl_datetime_format_fallback_symbols[&realm];
            vm.set_symbol_property(receiver, symbol, fallback)?;
            vm.set_property_attributes(
                receiver,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
            Ok(receiver)
        })
    }

    fn unwrap_date_time_format_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if self.date_time_locale(receiver).is_some() {
            return Ok(receiver);
        }
        let Some(realm) = self.intl_datetime_format_receiver_realm(p, receiver)? else {
            return Err(self.type_error(p, "incompatible DateTimeFormat receiver".into()));
        };
        let symbol = self.realm.intrinsics.intl_datetime_format_fallback_symbols[&realm];
        let fallback = self.get_index(p, receiver, symbol)?;
        if self.date_time_locale(fallback).is_some() {
            Ok(fallback)
        } else {
            Err(self.type_error(p, "incompatible DateTimeFormat receiver".into()))
        }
    }

    fn intl_datetime_format_receiver_realm(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Option<Value>, JsError> {
        let mut prototype = self.object_get_prototype_of(p, receiver)?;
        while !prototype.is_null() {
            if let Some(realm) = self.realm.intrinsics.intl_datetime_format_prototypes
                .iter()
                .find_map(|(realm, candidate)| (*candidate == prototype).then_some(*realm))
            {
                return Ok(Some(realm));
            }
            prototype = self.object_get_prototype_of(p, prototype)?;
        }
        Ok(None)
    }

    fn date_time_format_getter(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        if self.date_time_locale(this).is_none() {
            return Err(self.type_error(p, "incompatible DateTimeFormat receiver".into()));
        }
        if let Some(bound) = self.date_time_slot(this, DATE_TIME_FORMAT_BOUND_SLOT) {
            return Ok(bound);
        }
        let function = self.native_with_realm(
            Native::IntlDateTimeFormatFormat,
            Value::NULL,
            self.realm.globals,
        );
        let bound = self.bind_function(p, function, &[this])?;
        self.override_builtin_function_name(bound, "")?;
        self.set_date_time_slot(this, DATE_TIME_FORMAT_BOUND_SLOT, bound)?;
        Ok(bound)
    }

    fn date_time_format(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(_locale) = self.date_time_locale(this) else {
            return Err(self.type_error(p, "incompatible DateTimeFormat receiver".into()));
        };
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let fields = self.date_time_fields_for_value(p, this, value)?;
        let text = self.format_intl_date_time(this, fields);
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    fn date_time_fields_for_value(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
        value: Value,
    ) -> Result<DateTimeFields, JsError> {
        let fields = temporal_date_time_fields(self.heap.get(value))
            .or_else(|| self.temporal_plain_time_fields(value));
        if let Some(mut fields) = fields {
            let resolved = self
                .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
                .unwrap_or(Value::UNDEFINED);
            let date_style = self.date_time_option(resolved, "dateStyle").is_some();
            let time_style = self.date_time_option(resolved, "timeStyle").is_some();
            let incompatible_style = (date_style || time_style)
                && !(fields.has_date && date_style || fields.has_time && time_style);
            if incompatible_style {
                return Err(self.type_error(
                    p,
                    "dateStyle/timeStyle is incompatible with this Temporal value".into(),
                ));
            }
            self.validate_temporal_options(p, formatter, &fields)?;
            fields.has_date = fields.has_date
                && self
                    .date_time_slot(formatter, DATE_TIME_FORMAT_DATE_SLOT)
                    .is_some_and(|value| value == Value::TRUE);
            fields.has_time = fields.has_time
                && (self
                    .date_time_slot(formatter, DATE_TIME_FORMAT_TIME_SLOT)
                    .is_some_and(|value| value == Value::TRUE)
                    || self
                        .date_time_option(resolved, DATE_TIME_FORMAT_TEMPORAL_DEFAULTS_SLOT)
                        .is_some_and(|value| value == Value::TRUE)
                        && matches!(fields.temporal_kind, Some(TemporalKind::PlainDateTime | TemporalKind::PlainTime))
                    || fields.temporal_kind == Some(TemporalKind::PlainTime));
            self.calendarize_date_time_fields(formatter, &mut fields);
            return Ok(fields);
        }
        if matches!(
            self.heap.get(value),
            Some(Cell::TemporalZonedDateTime { .. })
        ) {
            return Err(self.type_error(p, "Temporal.ZonedDateTime is not supported".into()));
        }
        let milliseconds = if value.is_undefined() {
            HostContext::new(&mut self.host)
                .invoke(CapabilityId::ClockMillis, None)
                .trunc()
        } else {
            match self.heap.get(value) {
                Some(Cell::Date { milliseconds, .. }) => *milliseconds,
                Some(Cell::TemporalInstant {
                    epoch_nanoseconds, ..
                }) => (*epoch_nanoseconds as f64) / NANOSECONDS_PER_MILLISECOND,
                _ => self.to_number(p, value)?,
            }
        };
        if !milliseconds.is_finite() || milliseconds.abs() > super::date::DATE_TIME_CLIP_LIMIT_MS {
            return Err(self.range_error(p, "Invalid time value".into()));
        }
        let resolved = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let zone = self
            .date_time_option(resolved, "timeZone")
            .and_then(|value| self.string_value(value));
        let date = date_in_time_zone(milliseconds, zone.as_deref())
            .map(|date| DateTimeFields::from_date(&date))
            .or_else(|| date_time_fields_at_time_clip(milliseconds, zone.as_deref()));
        let Some(mut fields) = date else {
            return Err(self.range_error(p, "Invalid time value".into()));
        };
        if matches!(self.heap.get(value), Some(Cell::TemporalInstant { .. })) {
            fields.temporal_kind = Some(TemporalKind::Instant);
        }
        fields.has_date = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_DATE_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        fields.has_time = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_TIME_SLOT)
            .is_some_and(|value| value == Value::TRUE)
            || fields.temporal_kind == Some(TemporalKind::Instant)
                && self
                    .date_time_option(resolved, DATE_TIME_FORMAT_TEMPORAL_DEFAULTS_SLOT)
                    .is_some_and(|value| value == Value::TRUE);
        self.calendarize_date_time_fields(formatter, &mut fields);
        Ok(fields)
    }

    fn calendarize_date_time_fields(&self, formatter: Value, fields: &mut DateTimeFields) {
        if !fields.has_date {
            return;
        }
        let resolved = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let calendar = self
            .date_time_option(resolved, "calendar")
            .and_then(|value| self.string_value(value))
            .unwrap_or_else(|| "gregory".into());
        let Some(date) =
            quench_intl::calendar_fields_from_iso(fields.year, fields.month, fields.day, &calendar)
        else {
            return;
        };
        fields.year = date.year;
        fields.month = date.month;
        fields.day = date.day;
        fields.calendar = Some(calendar);
        fields.month_code = Some(date.month_code);
        fields.related_year = date.related_year;
        fields.cyclic_year = date.cyclic_year;
        fields.era = date.era;
        fields.era_year = date.era_year;
    }

    fn validate_temporal_options(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
        fields: &DateTimeFields,
    ) -> Result<(), JsError> {
        let resolved = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let has = |name| self.date_time_option(resolved, name).is_some();
        let has_date = ["weekday", "era", "year", "month", "day"]
            .into_iter()
            .any(has);
        let has_time = ["dayPeriod", "hour", "minute", "second"]
            .into_iter()
            .any(has);
        let temporal_defaults = self
            .date_time_option(resolved, DATE_TIME_FORMAT_TEMPORAL_DEFAULTS_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        let has_date_style = has("dateStyle");
        if fields.temporal_kind == Some(TemporalKind::PlainMonthDay)
            && has("year")
            && !has("month")
            && !has("day")
            && !has_date_style
        {
            return Err(
                self.type_error(p, "year is incompatible with Temporal.PlainMonthDay".into())
            );
        }
        if fields.temporal_kind == Some(TemporalKind::PlainYearMonth)
            && has("day")
            && !has("year")
            && !has("month")
        {
            return Err(
                self.type_error(p, "day is incompatible with Temporal.PlainYearMonth".into())
            );
        }
        if matches!(
            fields.temporal_kind,
            Some(
                TemporalKind::PlainDate
                    | TemporalKind::PlainMonthDay
                    | TemporalKind::PlainYearMonth
            )
        ) && has_time
            && !has_date
            && !has_date_style
        {
            return Err(self.type_error(
                p,
                "time fields are incompatible with this Temporal value".into(),
            ));
        }
        if fields.temporal_kind == Some(TemporalKind::PlainTime)
            && !temporal_defaults
            && ["year", "month", "day", "weekday"].into_iter().any(has)
            && !["hour", "minute", "second", "dayPeriod", "timeStyle"]
                .into_iter()
                .any(has)
            && !has("era")
        {
            return Err(self.type_error(
                p,
                "date fields are incompatible with Temporal.PlainTime".into(),
            ));
        }
        let Some(calendar) = fields.calendar.as_deref() else {
            return Ok(());
        };
        let format_calendar = self
            .date_time_option(resolved, "calendar")
            .and_then(|value| self.string_value(value))
            .unwrap_or_else(|| "gregory".into());
        let calendar = quench_intl::calendar_alias(calendar);
        let format_calendar = quench_intl::calendar_alias(&format_calendar);
        let compatible = if matches!(
            fields.temporal_kind,
            Some(TemporalKind::PlainMonthDay | TemporalKind::PlainYearMonth)
        ) {
            calendar == format_calendar
        } else {
            matches!(calendar.as_str(), "iso8601" | "gregory") || calendar == format_calendar
        };
        if !compatible {
            return Err(self.range_error(
                p,
                "Temporal calendar does not match formatter calendar".into(),
            ));
        }
        Ok(())
    }

    fn format_intl_date_time(&self, formatter: Value, fields: DateTimeFields) -> String {
        let options = self.date_time_part_options(formatter, &fields);
        self.localize_date_time_parts(
            formatter,
            intl_datetime_parts::format_parts(&fields, &options),
        )
        .into_iter()
        .map(|(_, value)| value)
        .collect()
    }

    fn localize_date_time_parts(
        &self,
        formatter: Value,
        parts: Vec<(String, String)>,
    ) -> Vec<(String, String)> {
        let resolved = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let numbering = self
            .date_time_option(resolved, "numberingSystem")
            .and_then(|value| self.string_value(value))
            .unwrap_or_else(|| "latn".into());
        parts
            .into_iter()
            .map(|(kind, value)| {
                let value = if numbering == "arab" && kind == "literal" && value == "." {
                    "٫".into()
                } else {
                    quench_intl::localize_digits(value, &numbering)
                };
                (kind, value)
            })
            .collect()
    }

    fn date_time_part_options(
        &self,
        formatter: Value,
        fields: &DateTimeFields,
    ) -> DateTimePartOptions {
        let resolved = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let text = |name| {
            self.date_time_option(resolved, name)
                .and_then(|value| self.string_value(value))
        };
        let mut options = DateTimePartOptions {
            locale: self.date_time_locale(formatter),
            weekday: text("weekday"),
            era: text("era"),
            year: text("year"),
            month: text("month"),
            day: text("day"),
            hour: text("hour"),
            minute: text("minute"),
            second: text("second"),
            day_period: text("dayPeriod"),
            hour_cycle: text("hourCycle").or_else(|| {
                self.date_time_locale(formatter).map(|locale| {
                    if locale.starts_with("en") || locale.starts_with("ja") {
                        "h12".into()
                    } else {
                        "h23".into()
                    }
                })
            }),
            fractional_second_digits: self
                .date_time_option(resolved, "fractionalSecondDigits")
                .and_then(Value::as_number)
                .map(|digits| digits as u32),
            time_zone_name: text("timeZoneName").map(|style| {
                let zone = text("timeZone").unwrap_or_else(|| DEFAULT_TIME_ZONE.into());
                time_zone_name_for(&style, &zone, fields)
            }),
        };
        if let Some(style) = text("dateStyle") {
            intl_datetime_parts::apply_date_style(&mut options, &style);
        }
        if let Some(style) = text("timeStyle") {
            intl_datetime_parts::apply_time_style(&mut options, &style);
            let time_zone_style = match style.as_str() {
                "full" => Some("long"),
                "long" => Some("short"),
                _ => None,
            };
            if let Some(zone_style) = time_zone_style {
                let zone = text("timeZone").unwrap_or_else(|| DEFAULT_TIME_ZONE.into());
                options
                    .time_zone_name
                    .get_or_insert_with(|| time_zone_name_for(zone_style, &zone, fields));
            }
        }
        if self
            .date_time_option(resolved, DATE_TIME_FORMAT_TEMPORAL_DEFAULTS_SLOT)
            .is_some_and(|value| value == Value::TRUE)
            && matches!(
                fields.temporal_kind,
                Some(TemporalKind::Instant | TemporalKind::PlainDateTime)
            )
        {
            options.hour = Some("numeric".into());
            options.minute = Some("2-digit".into());
            options.second = Some("2-digit".into());
        }
        if fields.temporal_kind == Some(TemporalKind::PlainDateTime) {
            let date_style = text("dateStyle").is_some();
            let time_style = text("timeStyle").is_some();
            if date_style && !time_style {
                options.hour = None;
                options.minute = None;
                options.second = None;
                options.day_period = None;
            } else if time_style && !date_style {
                options.weekday = None;
                options.era = None;
                options.year = None;
                options.month = None;
                options.day = None;
            }
        }
        match fields.temporal_kind {
            Some(TemporalKind::PlainDate) => {
                options.hour = None;
                options.minute = None;
                options.second = None;
                options.day_period = None;
                options.time_zone_name = None;
            }
            Some(TemporalKind::PlainMonthDay) => {
                options.weekday = None;
                options.year = None;
                options.era = None;
                options.hour = None;
                options.minute = None;
                options.second = None;
                options.day_period = None;
            }
            Some(TemporalKind::PlainYearMonth) => {
                options.weekday = None;
                options.day = None;
                options.hour = None;
                options.minute = None;
                options.second = None;
                options.day_period = None;
            }
            Some(TemporalKind::PlainTime) => {
                options.weekday = None;
                options.era = None;
                options.year = None;
                options.month = None;
                options.day = None;
                options.time_zone_name = None;
            }
            Some(TemporalKind::PlainDateTime)
            | Some(TemporalKind::Instant | TemporalKind::ZonedDateTime)
            | None => {}
        }
        if options.year.is_none()
            && options.month.is_none()
            && options.day.is_none()
            && options.weekday.is_none()
            && options.hour.is_none()
            && options.minute.is_none()
            && options.second.is_none()
            && options.day_period.is_none()
        {
            match fields.temporal_kind {
                Some(TemporalKind::PlainTime) => {
                    options.hour = Some("numeric".into());
                    options.minute = Some("2-digit".into());
                    options.second = Some("2-digit".into());
                }
                Some(TemporalKind::PlainMonthDay) => {
                    options.month = Some("numeric".into());
                    options.day = Some("numeric".into());
                }
                Some(TemporalKind::PlainYearMonth) => {
                    options.year = Some("numeric".into());
                    options.month = Some("numeric".into());
                }
                _ => {
                    options.year = Some("numeric".into());
                    options.month = Some("numeric".into());
                    options.day = Some("numeric".into());
                }
            }
        }
        options
    }

    fn string_value(&self, value: Value) -> Option<String> {
        match self.heap.get(value) {
            Some(Cell::String(value)) => Some(value.to_string()),
            _ => None,
        }
    }

    fn temporal_plain_time_fields(&self, value: Value) -> Option<DateTimeFields> {
        let object = self.object_data(value)?;
        let mut prototype = object.proto;
        let mut branded = false;
        while !prototype.is_null() {
            if prototype == self.temporal_plain_time_proto {
                branded = true;
                break;
            }
            prototype = self.object_data(prototype)?.proto;
        }
        if !branded {
            return None;
        }
        let field = |name: &str| {
            self.lookup_atom(name)
                .and_then(|atom| self.own_property(value, atom))
                .and_then(Value::as_number)
                .map(|number| number as u32)
                .unwrap_or_default()
        };
        let mut fields = DateTimeFields::from_components(
            1970,
            1,
            1,
            field("hour"),
            field("minute"),
            field("second"),
            field("millisecond"),
        );
        fields.has_date = false;
        fields.is_temporal = true;
        fields.temporal_kind = Some(TemporalKind::PlainTime);
        Some(fields)
    }

    fn date_time_format_to_parts(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if self.date_time_locale(this).is_none() {
            return Err(self.type_error(p, "incompatible DateTimeFormat receiver".into()));
        }
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let fields = self.date_time_fields_for_value(p, this, value)?;
        let options = self.date_time_part_options(this, &fields);
        let parts = self
            .localize_date_time_parts(this, intl_datetime_parts::format_parts(&fields, &options));
        self.date_time_parts_array(parts)
    }

    fn date_time_format_range(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if self.date_time_locale(this).is_none() {
            return Err(self.type_error(p, "incompatible DateTimeFormat receiver".into()));
        }
        if args.len() < 2 || args[0].is_undefined() || args[1].is_undefined() {
            return Err(self.type_error(p, "formatRange requires two date values".into()));
        }
        let start = args.first().copied().unwrap_or(Value::UNDEFINED);
        let end = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let (start, start_temporal) = self.date_time_formattable(p, start)?;
        let (end, end_temporal) = self.date_time_formattable(p, end)?;
        if !same_datetime_range_kind(start_temporal, end_temporal) {
            return Err(self.type_error(p, "formatRange arguments have different types".into()));
        }
        let start_fields = self.date_time_fields_for_value(p, this, start)?;
        let end_fields = self.date_time_fields_for_value(p, this, end)?;
        let start_parts = self.date_time_parts_for_fields(this, &start_fields);
        let end_parts = self.date_time_parts_for_fields(this, &end_fields);
        let start_options = self.date_time_part_options(this, &start_fields);
        let end_options = self.date_time_part_options(this, &end_fields);
        let textual_month = |options: &DateTimePartOptions| {
            options
                .month
                .as_deref()
                .is_some_and(|style| matches!(style, "long" | "short" | "narrow"))
        };
        let same_date = (start_fields.year, start_fields.month, start_fields.day)
            == (end_fields.year, end_fields.month, end_fields.day);
        let same_year = start_fields.year == end_fields.year;
        let fields_have_time = start_fields.has_time && end_fields.has_time;
        let compress_date =
            same_year && (textual_month(&start_options) || textual_month(&end_options));
        let parts = merge_date_time_range_parts(
            start_parts,
            end_parts,
            same_date,
            fields_have_time,
            compress_date,
        );
        if native == Native::IntlDateTimeFormatFormatRangeToParts {
            self.date_time_parts_array_with_sources(parts)
        } else {
            let text = parts
                .into_iter()
                .map(|(_, value, _)| value)
                .collect::<String>();
            Ok(self.heap.alloc(Cell::String(text.into())))
        }
    }

    fn date_time_parts_for_fields(
        &self,
        formatter: Value,
        fields: &DateTimeFields,
    ) -> Vec<(String, String)> {
        let options = self.date_time_part_options(formatter, fields);
        self.localize_date_time_parts(
            formatter,
            intl_datetime_parts::format_parts(fields, &options),
        )
    }

    fn date_time_formattable(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<(Value, Option<TemporalKind>), JsError> {
        let temporal = temporal_date_time_fields(self.heap.get(value))
            .or_else(|| self.temporal_plain_time_fields(value));
        if let Some(fields) = temporal {
            return Ok((value, fields.temporal_kind));
        }
        let temporal_kind = match self.heap.get(value) {
            Some(Cell::TemporalInstant { .. }) => Some(TemporalKind::Instant),
            Some(Cell::TemporalZonedDateTime { .. }) => Some(TemporalKind::ZonedDateTime),
            _ => None,
        };
        if temporal_kind.is_some() {
            return Ok((value, temporal_kind));
        }
        let number = self.to_number(p, value)?;
        Ok((Value::number(number), None))
    }

    fn date_time_parts_array(&mut self, parts: Vec<(String, String)>) -> Result<Value, JsError> {
        let parts = parts
            .into_iter()
            .map(|(kind, value)| (kind, value, None))
            .collect();
        self.date_time_parts_array_with_sources(parts)
    }

    fn date_time_parts_array_with_sources(
        &mut self,
        parts: Vec<(String, String, Option<String>)>,
    ) -> Result<Value, JsError> {
        let mut entries = Vec::with_capacity(parts.len());
        for (kind, value, source) in parts {
            let entry = self
                .heap
                .alloc(Cell::Object(Self::empty_object(self.object_proto)));
            let kind = self.heap.alloc(Cell::String(kind.into()));
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_date_time_property(entry, "type", kind)?;
            self.set_date_time_property(entry, "value", value)?;
            if let Some(source) = source {
                let source = self.heap.alloc(Cell::String(source.into()));
                self.set_date_time_property(entry, "source", source)?;
            }
            entries.push(entry);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(entries),
        }))
    }

    fn date_time_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let locale = self
            .date_time_locale(this)
            .ok_or_else(|| self.type_error(p, "incompatible DateTimeFormat receiver".into()))?;
        let result = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        let locale = self.heap.alloc(Cell::String(locale.into()));
        let resolved = self
            .date_time_slot(this, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let calendar = self
            .date_time_option(resolved, "calendar")
            .unwrap_or_else(|| self.heap.alloc(Cell::String("gregory".into())));
        let numbering_system = self
            .date_time_option(resolved, "numberingSystem")
            .unwrap_or_else(|| self.heap.alloc(Cell::String("latn".into())));
        let time_zone = self
            .date_time_option(resolved, "timeZone")
            .unwrap_or_else(|| self.heap.alloc(Cell::String("America/Lima".into())));
        self.set_date_time_property(result, "locale", locale)?;
        self.set_date_time_property(result, "calendar", calendar)?;
        self.set_date_time_property(result, "numberingSystem", numbering_system)?;
        self.set_date_time_property(result, "timeZone", time_zone)?;
        let has_hour = self.date_time_option(resolved, "hour").is_some()
            || self.date_time_option(resolved, "timeStyle").is_some();
        if has_hour {
            if let Some(value) = self.date_time_option(resolved, "hourCycle") {
                self.set_date_time_property(result, "hourCycle", value)?;
            }
            if let Some(value) = self.date_time_option(resolved, "hour12") {
                self.set_date_time_property(result, "hour12", value)?;
            }
        }
        for name in [
            "dateStyle",
            "timeStyle",
            "weekday",
            "era",
            "year",
            "month",
            "day",
            "dayPeriod",
            "hour",
            "minute",
            "second",
            "fractionalSecondDigits",
            "timeZoneName",
        ] {
            if let Some(value) = self.date_time_option(resolved, name) {
                self.set_date_time_property(result, name, value)?;
            }
        }
        Ok(result)
    }

    fn date_time_option(&self, options: Value, name: &str) -> Option<Value> {
        self.lookup_atom(name)
            .and_then(|atom| self.own_property(options, atom))
    }

    fn set_date_time_property(
        &mut self,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property(object, atom, value)
    }

    fn set_date_time_slot(
        &mut self,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_date_time_property(object, name, value)
    }

    fn date_time_slot(&self, object: Value, name: &str) -> Option<Value> {
        self.lookup_atom(name)
            .and_then(|atom| self.own_property(object, atom))
    }

    fn date_time_locale(&self, object: Value) -> Option<String> {
        self.date_time_slot(object, DATE_TIME_FORMAT_OPTIONS_SLOT)
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(locale)) => Some(locale.to_string()),
                _ => None,
            })
    }
}

fn temporal_date_time_fields(cell: Option<&Cell>) -> Option<DateTimeFields> {
    let mut fields = DateTimeFields {
        year: 1970,
        month: 1,
        day: 1,
        month_code: None,
        hour: 0,
        minute: 0,
        second: 0,
        millisecond: 0,
        weekday: 0,
        has_date: true,
        has_time: false,
        is_temporal: true,
        temporal_kind: None,
        calendar: None,
        related_year: None,
        cyclic_year: None,
        era: None,
        era_year: None,
    };
    match cell? {
        Cell::TemporalPlainDate {
            year,
            month,
            day,
            calendar,
            ..
        } => {
            fields.year = *year;
            fields.month = *month;
            fields.day = *day;
            fields.temporal_kind = Some(TemporalKind::PlainDate);
            fields.calendar = Some(calendar.clone());
        }
        Cell::TemporalPlainDateTime {
            date,
            time,
            calendar,
            ..
        } => {
            (fields.year, fields.month, fields.day) = *date;
            (
                fields.hour,
                fields.minute,
                fields.second,
                fields.millisecond,
            ) = (time[0], time[1], time[2], time[3]);
            fields.has_time = true;
            fields.temporal_kind = Some(TemporalKind::PlainDateTime);
            fields.calendar = Some(calendar.clone());
        }
        Cell::TemporalPlainMonthDay {
            month,
            day,
            reference_iso_year,
            calendar,
            ..
        } => {
            fields.year = *reference_iso_year;
            fields.month = *month;
            fields.day = *day;
            fields.temporal_kind = Some(TemporalKind::PlainMonthDay);
            fields.calendar = Some(calendar.clone());
        }
        Cell::TemporalPlainYearMonth {
            year,
            month,
            reference_iso_day,
            calendar,
            ..
        } => {
            fields.year = *year;
            fields.month = *month;
            fields.day = *reference_iso_day;
            fields.temporal_kind = Some(TemporalKind::PlainYearMonth);
            fields.calendar = Some(calendar.clone());
        }
        _ => return None,
    }
    fields.weekday = super::temporal_date::iso_day_of_week(super::temporal_date::IsoDate {
        year: fields.year,
        month: fields.month,
        day: fields.day,
    });
    Some(fields)
}

fn same_datetime_range_kind(
    start_temporal: Option<TemporalKind>,
    end_temporal: Option<TemporalKind>,
) -> bool {
    start_temporal == end_temporal
}

fn merge_date_time_range_parts(
    start: Vec<(String, String)>,
    end: Vec<(String, String)>,
    same_date: bool,
    has_time: bool,
    compress_date: bool,
) -> Vec<(String, String, Option<String>)> {
    if start == end {
        return start
            .into_iter()
            .map(|(kind, value)| (kind, value, Some("shared".into())))
            .collect();
    }
    if same_date && has_time {
        return merge_same_date_time_parts(start, end);
    }
    if !compress_date {
        let mut parts = Vec::with_capacity(start.len() + end.len() + RANGE_SEPARATOR_PARTS);
        append_range_parts(&mut parts, &start, "startRange");
        parts.push(("literal".into(), " – ".into(), Some("shared".into())));
        append_range_parts(&mut parts, &end, "endRange");
        return parts;
    }
    let shared_prefix = start
        .iter()
        .zip(&end)
        .take_while(|(left, right)| left == right)
        .count();
    let shared_suffix = start[shared_prefix..]
        .iter()
        .rev()
        .zip(end[shared_prefix..].iter().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let start_end = start.len() - shared_suffix;
    let end_end = end.len() - shared_suffix;
    let mut merged = Vec::with_capacity(start.len() + end.len() + RANGE_SEPARATOR_PARTS);
    append_range_parts(&mut merged, &start[..shared_prefix], "shared");
    append_range_parts(&mut merged, &start[shared_prefix..start_end], "startRange");
    if shared_prefix < start_end || shared_prefix < end_end {
        merged.push((
            "literal".into(),
            " – ".into(),
            Some("shared".into()),
        ));
    }
    append_range_parts(&mut merged, &end[shared_prefix..end_end], "endRange");
    append_range_parts(&mut merged, &start[start_end..], "shared");
    merged
}

fn merge_same_date_time_parts(
    start: Vec<(String, String)>,
    end: Vec<(String, String)>,
) -> Vec<(String, String, Option<String>)> {
    let time_start = start
        .iter()
        .position(|(kind, _)| matches!(kind.as_str(), "hour" | "minute" | "second"))
        .unwrap_or(start.len());
    let end_time_start = end
        .iter()
        .position(|(kind, _)| matches!(kind.as_str(), "hour" | "minute" | "second"))
        .unwrap_or(end.len());
    if time_start == start.len() || end_time_start == end.len() {
        return merge_identical_parts(start, end);
    }
    let date_end = time_start.saturating_sub(DATE_TIME_SEPARATOR_PARTS);
    let mut merged = Vec::with_capacity(start.len() + end.len() + RANGE_SEPARATOR_PARTS);
    append_range_parts(&mut merged, &start[..date_end], "shared");
    append_range_parts(&mut merged, &start[date_end..time_start], "shared");
    append_range_parts(&mut merged, &start[time_start..], "startRange");
    merged.push(("literal".into(), " – ".into(), Some("shared".into())));
    append_range_parts(&mut merged, &end[end_time_start..], "endRange");
    merged
}

fn merge_identical_parts(
    start: Vec<(String, String)>,
    end: Vec<(String, String)>,
) -> Vec<(String, String, Option<String>)> {
    if start == end {
        return start
            .into_iter()
            .map(|(kind, value)| (kind, value, Some("shared".into())))
            .collect();
    }
    let mut parts = Vec::with_capacity(start.len() + end.len() + RANGE_SEPARATOR_PARTS);
    append_range_parts(&mut parts, &start, "startRange");
    parts.push(("literal".into(), " – ".into(), Some("shared".into())));
    append_range_parts(&mut parts, &end, "endRange");
    parts
}

fn append_range_parts(
    output: &mut Vec<(String, String, Option<String>)>,
    parts: &[(String, String)],
    source: &str,
) {
    output.extend(
        parts
            .iter()
            .map(|(kind, value)| (kind.clone(), value.clone(), Some(source.into()))),
    );
}

const RANGE_SEPARATOR_PARTS: usize = 1;
const DATE_TIME_SEPARATOR_PARTS: usize = 1;

fn time_zone_name_for(style: &str, zone: &str, fields: &DateTimeFields) -> String {
    let zone = quench_temporal::timezone_primary_name(zone);
    if zone.eq_ignore_ascii_case("utc") {
        return match style {
            "long" | "longGeneric" => "Coordinated Universal Time".into(),
            _ => "UTC".into(),
        };
    }
    if let Some(name) = offset_time_zone_name(style, zone) {
        return name;
    }
    if let Some(name) = time_zone_name_from_zone(style, zone, fields) {
        return name;
    }
    match style {
        "long" | "longGeneric" if zone == DEFAULT_TIME_ZONE => {
            DEFAULT_TIME_ZONE_LONG_NAME.to_string()
        }
        "long" | "longGeneric" => zone.to_string(),
        "longOffset" => DEFAULT_TIME_ZONE_LONG_OFFSET.to_string(),
        _ if zone == DEFAULT_TIME_ZONE => DEFAULT_TIME_ZONE_SHORT_NAME.to_string(),
        _ => zone.to_string(),
    }
}

fn offset_time_zone_name(style: &str, zone: &str) -> Option<String> {
    let offset = FixedOffset::from_str(zone).ok()?.local_minus_utc();
    if offset == 0 {
        return Some("GMT".into());
    }
    let sign = if offset < 0 { '-' } else { '+' };
    let absolute_offset = offset.abs();
    let hours = absolute_offset / SECONDS_PER_HOUR as i32;
    let minutes = absolute_offset % SECONDS_PER_HOUR as i32 / SECONDS_PER_MINUTE;
    Some(if style == "longOffset" {
        format!("GMT{sign}{hours:02}:{minutes:02}")
    } else if minutes == 0 {
        format!("GMT{sign}{hours}")
    } else {
        format!("GMT{sign}{hours}:{minutes:02}")
    })
}

fn time_zone_name_from_zone(
    style: &str,
    zone: &str,
    fields: &DateTimeFields,
) -> Option<String> {
    let zone = chrono_tz::Tz::from_str(zone).ok()?;
    let date = NaiveDate::from_ymd_opt(fields.year, fields.month, fields.day)?;
    let local = date.and_hms_opt(fields.hour, fields.minute, fields.second)?;
    let zoned = zone
        .from_local_datetime(&local)
        .earliest()
        .or_else(|| zone.from_local_datetime(&local).latest())?;
    let abbreviation = zoned.format("%Z").to_string();
    if matches!(style, "long" | "longGeneric") {
        return Some(match abbreviation.as_str() {
            "CET" => "Central European Standard Time".into(),
            "CEST" => "Central European Summer Time".into(),
            "EST" => "Eastern Standard Time".into(),
            "EDT" => "Eastern Daylight Time".into(),
            "PST" => "Pacific Standard Time".into(),
            "PDT" => "Pacific Daylight Time".into(),
            _ => zone.to_string(),
        });
    }
    if matches!(style, "shortOffset" | "longOffset") {
        let offset = zoned.offset().fix().local_minus_utc();
        let sign = if offset < 0 { '-' } else { '+' };
        let absolute_offset = offset.abs();
        let hours = absolute_offset / SECONDS_PER_HOUR as i32;
        let minutes = absolute_offset % SECONDS_PER_HOUR as i32 / SECONDS_PER_MINUTE;
        return Some(if style == "longOffset" || minutes != 0 {
            format!("GMT{sign}{hours:02}:{minutes:02}")
        } else {
            format!("GMT{sign}{hours}")
        });
    }
    Some(abbreviation)
}

fn locale_unicode_value(locale: &str, key: &str) -> Option<String> {
    quench_intl::unicode_extension_value(locale, key)
}

fn remove_locale_unicode_key(locale: &str, key: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_string();
    };
    let parts = extension.split('-').collect::<Vec<_>>();
    let mut retained = Vec::new();
    let mut position = 0;
    while position < parts.len() {
        let start = position;
        position += 1;
        while position < parts.len() && parts[position].len() != 2 {
            position += 1;
        }
        if parts[start] != key {
            retained.extend_from_slice(&parts[start..position]);
        }
    }
    if retained.is_empty() {
        base.to_string()
    } else {
        format!("{base}-u-{}", retained.join("-"))
    }
}

fn normalize_time_zone_identifier(zone: &str) -> Option<String> {
    if zone.eq_ignore_ascii_case("utc") {
        return Some("UTC".into());
    }
    if let Some(minutes) = time_zone_offset_minutes(zone) {
        if minutes == 0 {
            return Some("+00:00".into());
        }
        let sign = if minutes.is_negative() { '-' } else { '+' };
        let total_minutes = minutes.abs();
        return Some(format!(
            "{sign}{:02}:{:02}",
            total_minutes / MINUTES_PER_HOUR,
            total_minutes % MINUTES_PER_HOUR
        ));
    }
    quench_intl::time_zone_identifier(zone).map(str::to_owned)
}

fn date_in_time_zone(
    milliseconds: f64,
    zone: Option<&str>,
) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let Some(zone) = zone else {
        return super::date::date_local(milliseconds);
    };
    if let Some(minutes) = time_zone_offset_minutes(zone) {
        let seconds = minutes.checked_mul(SECONDS_PER_MINUTE)?;
        let offset = FixedOffset::east_opt(seconds)?;
        return Utc
            .timestamp_millis_opt(milliseconds.trunc() as i64)
            .single()
            .map(|instant| instant.with_timezone(&offset));
    }
    let timezone = chrono_tz::Tz::from_str(zone).ok().or_else(|| {
        normalize_time_zone_identifier(zone).and_then(|canonical| chrono_tz::Tz::from_str(&canonical).ok())
    })?;
    Utc.timestamp_millis_opt(milliseconds.trunc() as i64)
        .single()
        .map(|instant| instant.with_timezone(&timezone).fixed_offset())
}

fn date_time_fields_at_time_clip(milliseconds: f64, zone: Option<&str>) -> Option<DateTimeFields> {
    let offset_minutes = match zone {
        None => DEFAULT_TIME_ZONE_OFFSET_MINUTES,
        Some(zone) if zone.eq_ignore_ascii_case("utc") => 0,
        Some(zone) => time_zone_offset_minutes(zone)?,
    };
    let offset_milliseconds = i64::from(offset_minutes)
        .checked_mul(i64::from(SECONDS_PER_MINUTE))?
        .checked_mul(MILLISECONDS_PER_SECOND)?;
    let local_milliseconds = (milliseconds.trunc() as i64).checked_add(offset_milliseconds)?;
    let days = local_milliseconds.div_euclid(MILLISECONDS_PER_DAY);
    let within_day = local_milliseconds.rem_euclid(MILLISECONDS_PER_DAY);
    let date: super::temporal_date::IsoDate = quench_temporal::civil_from_days(days)?.into();
    let hour = within_day / MILLISECONDS_PER_HOUR;
    let within_hour = within_day % MILLISECONDS_PER_HOUR;
    let minute = within_hour / (i64::from(SECONDS_PER_MINUTE) * MILLISECONDS_PER_SECOND);
    let within_minute = within_hour % (i64::from(SECONDS_PER_MINUTE) * MILLISECONDS_PER_SECOND);
    Some(DateTimeFields::from_components(
        date.year,
        date.month,
        date.day,
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(within_minute / MILLISECONDS_PER_SECOND).ok()?,
        u32::try_from(within_minute % MILLISECONDS_PER_SECOND).ok()?,
    ))
}

fn time_zone_offset_minutes(zone: &str) -> Option<i32> {
    let sign = match zone.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let digits = &zone[TIME_ZONE_SIGN_LENGTH..];
    let (hours, minutes) = match digits.len() {
        TIME_ZONE_HOUR_DIGITS => (digits, "00"),
        TIME_ZONE_COMPACT_DIGITS => (
            &digits[..TIME_ZONE_HOUR_DIGITS],
            &digits[TIME_ZONE_HOUR_DIGITS..],
        ),
        TIME_ZONE_COLON_DIGITS => {
            if digits.as_bytes().get(TIME_ZONE_HOUR_DIGITS) != Some(&b':') {
                return None;
            }
            (
                &digits[..TIME_ZONE_HOUR_DIGITS],
                &digits[TIME_ZONE_HOUR_DIGITS + 1..],
            )
        }
        _ => return None,
    };
    if !hours.bytes().all(|byte| byte.is_ascii_digit())
        || !minutes.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let hours = hours.parse::<i32>().ok()?;
    let minutes = minutes.parse::<i32>().ok()?;
    if hours > MAX_TIME_ZONE_HOUR || minutes > MAX_TIME_ZONE_MINUTE {
        return None;
    }
    Some(sign * (hours * MINUTES_PER_HOUR + minutes))
}

fn set_non_writable_property<H: Host>(vm: &mut Vm<H>, object: Value, name: &str) {
    let atom = vm.intern_atom(name);
    vm.set_property_attributes(
        object,
        PropertyKey::string(atom),
        PropertyAttributes {
            writable: false,
            enumerable: false,
            configurable: false,
            accessor: false,
            getter: None,
            setter: None,
        },
    );
}
