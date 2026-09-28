use super::*;
use chrono::{Datelike, Timelike};
use std::str::FromStr;

const DATE_TIME_FORMAT_OPTIONS_SLOT: &str = "\0rqj:intl-datetime-options";
const DATE_TIME_FORMAT_RESOLVED_SLOT: &str = "\0rqj:intl-datetime-resolved-options";
const DATE_TIME_FORMAT_DATE_SLOT: &str = "\0rqj:intl-datetime-date";
const DATE_TIME_FORMAT_TIME_SLOT: &str = "\0rqj:intl-datetime-time";
const DATE_TIME_FORMAT_BOUND_SLOT: &str = "\0rqj:intl-datetime-bound-format";
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
            self.date_time_options(p, args.get(1).copied(), DateTimeDefaults::DateAndTime)?;
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
                    if zone != "UTC" && chrono_tz::Tz::from_str(&zone).is_err() {
                        return Err(self.range_error(p, "invalid timeZone".into()));
                    }
                    has_time = true;
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
        if !any || (!has_date && !has_time) {
            match defaults {
                DateTimeDefaults::Date => has_date = true,
                DateTimeDefaults::Time => has_time = true,
                DateTimeDefaults::DateAndTime => {
                    has_date = true;
                    has_time = true;
                }
            }
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
        let milliseconds = match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => *milliseconds,
            _ => self.to_number(p, value)?,
        };
        if !milliseconds.is_finite() || milliseconds.abs() > super::date::DATE_TIME_CLIP_LIMIT_MS {
            return Err(self.range_error(p, "Invalid time value".into()));
        }
        let Some(date) = super::date::date_local(milliseconds) else {
            return Err(self.range_error(p, "Invalid time value".into()));
        };
        let has_date = self
            .date_time_slot(this, DATE_TIME_FORMAT_DATE_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        let has_time = self
            .date_time_slot(this, DATE_TIME_FORMAT_TIME_SLOT)
            .is_some_and(|value| value == Value::TRUE);
        let text = format_date_time(&date, has_date, has_time);
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    fn date_time_format_to_parts(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let text = self.date_time_format(p, this, args)?;
        self.date_time_parts_array(text)
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
            self.date_time_parts_array(text)
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

    fn date_time_parts_array(&mut self, value: Value) -> Result<Value, JsError> {
        let entry = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        let text_atom = self.intern_atom("value");
        let kind_atom = self.intern_atom("type");
        let kind = self.heap.alloc(Cell::String("literal".into()));
        self.set_property(entry, kind_atom, kind)?;
        self.set_property(entry, text_atom, value)?;
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(vec![entry]),
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

fn format_date_time(
    date: &chrono::DateTime<chrono::FixedOffset>,
    has_date: bool,
    has_time: bool,
) -> String {
    match (has_date, has_time) {
        (true, true) => format!(
            "{} {}, {} {:02}:{:02}:{:02}",
            date.month(),
            date.day(),
            date.year(),
            date.hour(),
            date.minute(),
            date.second()
        ),
        (true, false) => format!("{} {}, {}", date.month(), date.day(), date.year()),
        (false, true) => format!(
            "{:02}:{:02}:{:02}",
            date.hour(),
            date.minute(),
            date.second()
        ),
        (false, false) => String::new(),
    }
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
