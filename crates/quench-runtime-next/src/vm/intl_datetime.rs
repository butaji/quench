use super::intl_datetime_parts::{self, DateTimeFields, DateTimePartOptions};
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
const SECONDS_PER_MINUTE: i32 = 60;
const MINUTES_PER_HOUR: i32 = 60;
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
        let options = self.date_time_options(p, args.get(1).copied(), DateTimeDefaults::Format)?;
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
    ) -> Result<(bool, bool, Value), JsError> {
        let options = match options.filter(|value| !value.is_undefined()) {
            Some(value) if value.is_null() => {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            Some(value) => self.box_object(value)?,
            None => self
                .heap
                .alloc(Cell::Object(Self::empty_object(self.object_proto))),
        };
        let mut has_date = false;
        let mut has_time = false;
        let mut any = false;
        let resolved = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
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
                "calendar" | "numberingSystem" => {
                    let text = self.to_string(p, value)?;
                    if text.is_empty() {
                        return Err(self.range_error(p, format!("invalid {key}").into()));
                    }
                    Some(self.heap.alloc(Cell::String(text.into())))
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
                if !has_date && !has_date_style {
                    has_date = true;
                    self.set_default_date_components(resolved)?;
                }
                if !has_time && !has_time_style {
                    has_time = true;
                    self.set_default_time_components(resolved)?;
                }
            }
            _ => {}
        }
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
        let locale = args.first().copied().unwrap_or(Value::UNDEFINED);
        let options = self.date_time_options(p, args.get(1).copied(), defaults)?;
        let prototype_atom = self.intern_atom("prototype");
        let constructor_prototype = self.get_property(p, constructor, prototype_atom)?;
        let formatter = self
            .heap
            .alloc(Cell::Object(Self::empty_object(constructor_prototype)));
        let locale = self.collator_locale(p, Some(locale))?;
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
        let milliseconds = match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => *milliseconds,
            Some(Cell::TemporalInstant {
                epoch_nanoseconds, ..
            }) => (*epoch_nanoseconds as f64) / NANOSECONDS_PER_MILLISECOND,
            _ => self.to_number(p, value)?,
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
        let Some(date) = date_in_time_zone(milliseconds, zone.as_deref()) else {
            return Err(self.range_error(p, "Invalid time value".into()));
        };
        let mut fields = DateTimeFields::from_date(&date);
        fields.has_date = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_DATE_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        fields.has_time = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_TIME_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        Ok(fields)
    }

    fn format_intl_date_time(&self, formatter: Value, fields: DateTimeFields) -> String {
        let options = self.date_time_part_options(formatter);
        intl_datetime_parts::format_parts(fields, &options)
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    fn date_time_part_options(&self, formatter: Value) -> DateTimePartOptions {
        let resolved = self
            .date_time_slot(formatter, DATE_TIME_FORMAT_RESOLVED_SLOT)
            .unwrap_or(Value::UNDEFINED);
        let text = |name| {
            self.date_time_option(resolved, name)
                .and_then(|value| self.string_value(value))
        };
        let mut options = DateTimePartOptions {
            weekday: text("weekday"),
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
        if options.year.is_none()
            && options.month.is_none()
            && options.day.is_none()
            && options.weekday.is_none()
            && options.hour.is_none()
            && options.minute.is_none()
            && options.second.is_none()
        {
            options.year = Some("numeric".into());
            options.month = Some("numeric".into());
            options.day = Some("numeric".into());
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
        let options = self.date_time_part_options(this);
        let parts = intl_datetime_parts::format_parts(fields, &options);
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
        let start = args.first().copied().unwrap_or(Value::UNDEFINED);
        let end = args.get(1).copied().unwrap_or(Value::UNDEFINED);
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
        let text = if start_text == end_text {
            start_text
        } else {
            format!("{start_text} – {end_text}")
        };
        let text = self.heap.alloc(Cell::String(text.into()));
        if native == Native::IntlDateTimeFormatFormatRangeToParts {
            let text = self.string_value(text).unwrap_or_default();
            self.date_time_parts_array(vec![("literal".into(), text)])
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
        let mut entries = Vec::with_capacity(parts.len());
        for (kind, value) in parts {
            let entry = self
                .heap
                .alloc(Cell::Object(Self::empty_object(self.object_proto)));
            let kind = self.heap.alloc(Cell::String(kind.into()));
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_date_time_property(entry, "type", kind)?;
            self.set_date_time_property(entry, "value", value)?;
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
        let has_hour = self.date_time_option(resolved, "hour").is_some();
        if has_hour {
            if let Some(value) = self.date_time_option(resolved, "hourCycle") {
                self.set_date_time_property(result, "hourCycle", value)?;
            }
            if let Some(value) = self.date_time_option(resolved, "hour12") {
                self.set_date_time_property(result, "hour12", value)?;
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
    };
    match cell? {
        Cell::TemporalPlainDate {
            year, month, day, ..
        } => {
            fields.year = *year;
            fields.month = *month;
            fields.day = *day;
        }
        Cell::TemporalPlainDateTime { date, time, .. } => {
            (fields.year, fields.month, fields.day) = *date;
            (
                fields.hour,
                fields.minute,
                fields.second,
                fields.millisecond,
            ) = (time[0], time[1], time[2], time[3]);
            fields.has_time = true;
        }
        Cell::TemporalPlainMonthDay {
            month,
            day,
            reference_iso_year,
            ..
        } => {
            fields.year = *reference_iso_year;
            fields.month = *month;
            fields.day = *day;
        }
        Cell::TemporalPlainYearMonth {
            year,
            month,
            reference_iso_day,
            ..
        } => {
            fields.year = *year;
            fields.month = *month;
            fields.day = *reference_iso_day;
        }
        _ => return None,
    }
    Some(fields)
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
