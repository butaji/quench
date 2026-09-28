use super::intl_datetime_parts::{self, DateTimeFields, DateTimePartOptions, TemporalKind};
use super::*;
use chrono::{FixedOffset, TimeZone, Utc};
use std::str::FromStr;

const DATE_TIME_FORMAT_OPTIONS_SLOT: &str = "\0rqj:intl-datetime-options";
const DATE_TIME_FORMAT_RESOLVED_SLOT: &str = "\0rqj:intl-datetime-resolved-options";
const DATE_TIME_FORMAT_DATE_SLOT: &str = "\0rqj:intl-datetime-date";
const DATE_TIME_FORMAT_TIME_SLOT: &str = "\0rqj:intl-datetime-time";
const DATE_TIME_FORMAT_BOUND_SLOT: &str = "\0rqj:intl-datetime-bound-format";
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
    ("dateStyle", &["full", "long", "medium", "short"]),
    ("timeStyle", &["full", "long", "medium", "short"]),
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
];

#[derive(Clone, Copy)]
enum DateTimeDefaults {
    Format,
    Date,
    Time,
    DateAndTime,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_date_time_format_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlDateTimeFormat, global, global);
        self.intl_datetime_format_constructors
            .insert(global, constructor);
        self.set_builtin_function_name(constructor, "DateTimeFormat")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
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
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self
            .intl_datetime_format_constructors
            .get(&self.realm.globals)
            .copied()
            .ok_or_else(|| JsError("Intl.DateTimeFormat intrinsic is not installed".into()))?;
        self.intl_date_time_format_construct(p, args, constructor)
    }

    pub(super) fn intl_date_time_format_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let locale = self.collator_locale(p, args.first().copied())?;
        let options =
            self.date_time_options(p, args.get(1).copied(), DateTimeDefaults::Format, &locale)?;
        let locale = self
            .date_time_option(options.2, "\0locale")
            .and_then(|value| self.string_value(value))
            .unwrap_or(locale);
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        };
        let formatter = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let locale_value = self.heap.alloc(Cell::String(locale.into()));
        self.set_date_time_slot(formatter, DATE_TIME_FORMAT_OPTIONS_SLOT, locale_value)?;
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
        let mut has_date = false;
        let mut has_time = false;
        let mut any = false;
        let resolved = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        let locale_calendar = locale_unicode_value(locale, "ca")
            .map(|calendar| quench_intl::calendar_alias(&calendar))
            .filter(|calendar| quench_intl::valid_calendar(calendar))
            .unwrap_or_else(|| "gregory".into());
        let locale_numbering_system = locale_unicode_value(locale, "nu")
            .filter(|system| quench_intl::valid_numbering_system(system))
            .unwrap_or_else(|| "latn".into());
        let calendar_value = self
            .heap
            .alloc(Cell::String(locale_calendar.clone().into()));
        self.set_date_time_property(resolved, "calendar", calendar_value)?;
        let numbering_value = self
            .heap
            .alloc(Cell::String(locale_numbering_system.clone().into()));
        self.set_date_time_property(resolved, "numberingSystem", numbering_value)?;
        for (key, allowed) in DATE_TIME_OPTIONS {
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
                        "hourCycle" if !matches!(text.as_str(), "h11" | "h12" | "h23" | "h24") => {}
                        "localeMatcher" | "formatMatcher" => {}
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
                    let Some(zone) = canonical_time_zone(&zone) else {
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
            _ => {}
        }
        let mut resolved_locale = quench_intl::sanitize_datetime_locale(locale);
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
        let constructor = self
            .intl_datetime_format_constructors
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
        let prototype_atom = self.intern_atom("prototype");
        let constructor_prototype = self.get_property(p, constructor, prototype_atom)?;
        let formatter = self
            .heap
            .alloc(Cell::Object(Self::empty_object(constructor_prototype)));
        let locale = self
            .date_time_option(options.2, "\0locale")
            .and_then(|value| self.string_value(value))
            .unwrap_or(locale);
        let locale_value = self.heap.alloc(Cell::String(locale.into()));
        self.set_date_time_slot(formatter, DATE_TIME_FORMAT_OPTIONS_SLOT, locale_value)?;
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
        let format_atom = self.intern_atom("format");
        let format = self.get_property(p, formatter, format_atom)?;
        self.call_value(p, format, formatter, &[this])
    }

    pub(super) fn intl_date_time_format_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlDateTimeFormatFormatGetter => self.date_time_format_getter(p, this),
            Native::IntlDateTimeFormatFormat => self.date_time_format(p, this, args),
            Native::IntlDateTimeFormatFormatToParts => {
                self.date_time_format_to_parts(p, this, args)
            }
            Native::IntlDateTimeFormatFormatRange
            | Native::IntlDateTimeFormatFormatRangeToParts => {
                self.date_time_format_range(p, native, this, args)
            }
            Native::IntlDateTimeFormatSupportedLocalesOf => {
                self.date_time_supported_locales_of(p, args)
            }
            Native::IntlDateTimeFormatResolvedOptions => self.date_time_resolved_options(p, this),
            _ => Err(JsError("invalid Intl.DateTimeFormat method".into())),
        }
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
        if let Some(fields) = fields {
            let resolved = self
                .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
                .unwrap_or(Value::UNDEFINED);
            if !fields.has_date
                && self.date_time_option(resolved, "dateStyle").is_some()
                && self.date_time_option(resolved, "timeStyle").is_none()
            {
                return Err(self.type_error(
                    p,
                    "dateStyle is incompatible with this Temporal value".into(),
                ));
            }
            if !fields.has_time
                && self.date_time_option(resolved, "timeStyle").is_some()
                && self.date_time_option(resolved, "dateStyle").is_none()
            {
                return Err(self.type_error(
                    p,
                    "timeStyle is incompatible with this Temporal value".into(),
                ));
            }
            self.validate_temporal_options(p, formatter, &fields)?;
            return Ok(DateTimeFields {
                has_date: fields.has_date
                    && self
                        .date_time_slot(formatter, DATE_TIME_FORMAT_DATE_SLOT)
                        .is_some_and(|value| value == Value::TRUE),
                has_time: fields.has_time
                    && self
                        .date_time_slot(formatter, DATE_TIME_FORMAT_TIME_SLOT)
                        .is_some_and(|value| value == Value::TRUE),
                ..fields
            });
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
        fields.has_date = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_DATE_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        fields.has_time = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_TIME_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        Ok(fields)
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
        let compatible = if matches!(
            fields.temporal_kind,
            Some(TemporalKind::PlainMonthDay | TemporalKind::PlainYearMonth)
        ) {
            calendar == format_calendar
        } else {
            matches!(calendar, "iso8601" | "gregory") || calendar == format_calendar
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
        intl_datetime_parts::format_parts(&fields, &options)
            .into_iter()
            .map(|(_, value)| value)
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
            hour12: self
                .date_time_option(resolved, "hour12")
                .and_then(Value::as_bool),
            fractional_second_digits: self
                .date_time_option(resolved, "fractionalSecondDigits")
                .and_then(Value::as_number)
                .map(|digits| digits as u32),
            time_zone_name: text("timeZoneName").map(|style| {
                let zone = text("timeZone").unwrap_or_else(|| DEFAULT_TIME_ZONE.into());
                time_zone_name_for(&style, &zone)
            }),
        };
        if let Some(style) = text("dateStyle") {
            intl_datetime_parts::apply_date_style(&mut options, &style);
        }
        if let Some(style) = text("timeStyle") {
            intl_datetime_parts::apply_time_style(&mut options, &style);
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
                options.year = None;
                options.hour = None;
                options.minute = None;
                options.second = None;
                options.day_period = None;
            }
            Some(TemporalKind::PlainYearMonth) => {
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
            Some(TemporalKind::PlainDateTime) | None => {}
        }
        if options.year.is_none()
            && options.month.is_none()
            && options.day.is_none()
            && options.weekday.is_none()
            && options.hour.is_none()
            && options.minute.is_none()
            && options.second.is_none()
        {
            match fields.temporal_kind {
                Some(TemporalKind::PlainTime) => options.hour = Some("numeric".into()),
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
        Some(DateTimeFields {
            year: 1970,
            month: 1,
            day: 1,
            hour: field("hour"),
            minute: field("minute"),
            second: field("second"),
            millisecond: field("millisecond"),
            has_date: false,
            has_time: true,
            is_temporal: true,
            temporal_kind: Some(TemporalKind::PlainTime),
            calendar: None,
        })
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
        let parts = intl_datetime_parts::format_parts(&fields, &options);
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
        let start_fields = self.date_time_fields_for_value(p, this, start)?;
        let end_fields = self.date_time_fields_for_value(p, this, end)?;
        if !same_datetime_range_kind(
            self.heap.get(start),
            self.heap.get(end),
            start_fields.temporal_kind,
            end_fields.temporal_kind,
        ) {
            return Err(self.type_error(p, "formatRange arguments have different types".into()));
        }
        let start_text = self.date_time_format(p, this, &[start])?;
        let end_text = self.date_time_format(p, this, &[end])?;
        let start_text = match self.heap.get(start_text) {
            Some(Cell::String(value)) => value.to_string(),
            _ => String::new(),
        };
        let end_text = match self.heap.get(end_text) {
            Some(Cell::String(value)) => value.to_string(),
            _ => String::new(),
        };
        let is_shared_range = start_text == end_text;
        let text = if is_shared_range {
            start_text
        } else {
            format!("{start_text} – {end_text}")
        };
        let text = self.heap.alloc(Cell::String(text.into()));
        if native == Native::IntlDateTimeFormatFormatRangeToParts {
            let start_options = self.date_time_part_options(this, &start_fields);
            let start_parts = intl_datetime_parts::format_parts(&start_fields, &start_options);
            let parts = if is_shared_range {
                start_parts
                    .into_iter()
                    .map(|(kind, value)| (kind, value, Some("shared".into())))
                    .collect()
            } else {
                let end_options = self.date_time_part_options(this, &end_fields);
                let end_parts = intl_datetime_parts::format_parts(&end_fields, &end_options);
                start_parts
                    .into_iter()
                    .map(|(kind, value)| (kind, value, Some("startRange".into())))
                    .chain(std::iter::once((
                        "literal".into(),
                        " – ".into(),
                        Some("shared".into()),
                    )))
                    .chain(
                        end_parts
                            .into_iter()
                            .map(|(kind, value)| (kind, value, Some("endRange".into()))),
                    )
                    .collect()
            };
            self.date_time_parts_array_with_sources(parts)
        } else {
            Ok(text)
        }
    }

    fn date_time_supported_locales_of(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let locales = self.collator_locale_list(p, args.first().copied())?;
        let values = locales
            .into_iter()
            .map(|locale| self.heap.alloc(Cell::String(locale.into())))
            .collect::<Vec<_>>();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
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
        hour: 0,
        minute: 0,
        second: 0,
        millisecond: 0,
        has_date: true,
        has_time: false,
        is_temporal: true,
        temporal_kind: None,
        calendar: None,
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
    Some(fields)
}

fn same_datetime_range_kind(
    start: Option<&Cell>,
    end: Option<&Cell>,
    start_temporal: Option<TemporalKind>,
    end_temporal: Option<TemporalKind>,
) -> bool {
    if start_temporal.is_some() || end_temporal.is_some() {
        return start_temporal == end_temporal;
    }
    matches!(
        (start, end),
        (Some(Cell::Date { .. }), Some(Cell::Date { .. }))
    ) || matches!(
        (start, end),
        (
            Some(Cell::TemporalInstant { .. }),
            Some(Cell::TemporalInstant { .. })
        )
    ) || (start.is_none() && end.is_none())
}

fn time_zone_name_for(style: &str, zone: &str) -> String {
    if zone.eq_ignore_ascii_case("utc") {
        return match style {
            "long" | "longGeneric" => "Coordinated Universal Time".into(),
            _ => "UTC".into(),
        };
    }
    if zone.starts_with(['+', '-']) {
        return match style {
            "longOffset" => format!("GMT{zone}"),
            _ => format!("GMT{zone}"),
        };
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

fn locale_unicode_value(locale: &str, key: &str) -> Option<String> {
    let (_, extension) = locale.split_once("-u-")?;
    let parts = extension.split('-').collect::<Vec<_>>();
    let position = parts.iter().position(|part| *part == key)?;
    parts
        .get(position + 1)
        .filter(|value| value.len() != 2)
        .map(|value| (*value).to_string())
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

fn canonical_time_zone(zone: &str) -> Option<String> {
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
    chrono_tz::Tz::from_str(zone)
        .ok()
        .map(|timezone| timezone.name().to_string())
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
    let timezone = chrono_tz::Tz::from_str(zone).ok()?;
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
