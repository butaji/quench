use super::*;

const RELATIVE_LOCALE_SLOT: &str = "\0rqj:intl-relative-locale";
const RELATIVE_STYLE_SLOT: &str = "\0rqj:intl-relative-style";
const RELATIVE_NUMERIC_SLOT: &str = "\0rqj:intl-relative-numeric";
const RELATIVE_NUMBERING_SYSTEM_SLOT: &str = "\0rqj:intl-relative-numbering-system";
const RELATIVE_PARTS_MAX_FRACTION_DIGITS: usize = 3;

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_relative_time_format_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlRelativeTimeFormat, global, global);
        self.set_builtin_function_name(constructor, "RelativeTimeFormat")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.intl_relative_time_format_prototypes
            .insert(global, prototype);
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.RelativeTimeFormat")?;
        let resolved = self.native_with_realm(
            Native::IntlRelativeTimeFormatResolvedOptions,
            global,
            global,
        );
        self.set_builtin_function_name(resolved, "resolvedOptions")?;
        self.set_builtin_value_named(prototype, "resolvedOptions", resolved)?;
        for (name, native) in [
            ("format", Native::IntlRelativeTimeFormatFormat),
            ("formatToParts", Native::IntlRelativeTimeFormatFormatToParts),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        let supported = self.native_with_realm(
            Native::IntlRelativeTimeFormatSupportedLocalesOf,
            global,
            global,
        );
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.set_builtin_value_named(intl, "RelativeTimeFormat", constructor)
    }

    pub(super) fn intl_relative_time_format_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlRelativeTimeFormat => {
                Err(self.type_error(p, "RelativeTimeFormat requires 'new'".into()))
            }
            Native::IntlRelativeTimeFormatFormat | Native::IntlRelativeTimeFormatFormatToParts => {
                self.intl_relative_time_format_value(p, native, this, args)
            }
            _ => Err(JsError("invalid RelativeTimeFormat operation".into())),
        }
    }

    pub(super) fn intl_relative_time_format_supported_locales_of(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if let Some(options) = args.get(1).copied().filter(|value| !value.is_undefined()) {
            if options.is_null() {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            let options = self.box_object(options)?;
            self.string_option(
                p,
                options,
                "localeMatcher",
                "best fit",
                &["lookup", "best fit"],
            )?;
        }
        let locales = self
            .collator_locale_list(p, args.first().copied())?
            .into_iter()
            .filter(|locale| super::intl_number::is_supported_locale(locale))
            .map(|locale| self.heap.alloc(Cell::String(locale.into())))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(locales),
        }))
    }

    fn intl_relative_time_format_value(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let locale = self
            .hidden_string(this, RELATIVE_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible RelativeTimeFormat receiver".into()))?;
        let style = self
            .hidden_string(this, RELATIVE_STYLE_SLOT)
            .unwrap_or_else(|| "long".into());
        let numeric = self
            .hidden_string(this, RELATIVE_NUMERIC_SLOT)
            .unwrap_or_else(|| "always".into());
        let numbering_system = self
            .hidden_string(this, RELATIVE_NUMBERING_SYSTEM_SLOT)
            .unwrap_or_else(|| "latn".into());
        let value = self.to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if !value.is_finite() {
            return Err(self.range_error(p, "value must be finite".into()));
        }
        let unit_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let unit = self.to_string(p, unit_value)?;
        let unit = normalize_relative_unit(&unit)
            .ok_or_else(|| self.range_error(p, "invalid unit".into()))?;
        let parts = relative_time_parts(value, unit, &style, &numeric, &locale, &numbering_system);
        match native {
            Native::IntlRelativeTimeFormatFormat => Ok(self.heap.alloc(Cell::String(
                parts
                    .into_iter()
                    .map(|part| part.value)
                    .collect::<String>()
                    .into(),
            ))),
            Native::IntlRelativeTimeFormatFormatToParts => {
                let values = parts
                    .into_iter()
                    .map(|part| {
                        let object = self.object();
                        let part_type = self.heap.alloc(Cell::String(part.kind.into()));
                        self.set_named(p, object, "type", part_type)?;
                        let part_value = self.heap.alloc(Cell::String(part.value.into()));
                        self.set_named(p, object, "value", part_value)?;
                        if part.unit {
                            let part_unit = self.heap.alloc(Cell::String(unit.into()));
                            self.set_named(p, object, "unit", part_unit)?;
                        }
                        Ok(object)
                    })
                    .collect::<Result<Vec<_>, JsError>>()?;
                Ok(self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: Rc::new(values),
                }))
            }
            _ => Err(JsError("invalid RelativeTimeFormat operation".into())),
        }
    }

    pub(super) fn intl_relative_time_format_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let locales = self.collator_locale_list(p, args.first().copied())?;
        let mut locale = locales.first().cloned().unwrap_or_else(|| "en-US".into());
        let options = match args.get(1).copied().filter(|value| !value.is_undefined()) {
            Some(value) if value.is_null() => {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            Some(value) => self.box_object(value)?,
            None => self
                .heap
                .alloc(Cell::Object(Self::empty_object(Value::NULL))),
        };
        let locale_matcher = self.string_option(
            p,
            options,
            "localeMatcher",
            "best fit",
            &["lookup", "best fit"],
        )?;
        let numbering_system_atom = self.intern_atom("numberingSystem");
        let numbering_system = self.get_property(p, options, numbering_system_atom)?;
        let locale_numbering_system = locale_unicode_numbering_system(&locale);
        let option_numbering_system = if numbering_system.is_undefined() {
            None
        } else {
            let requested = self.to_string(p, numbering_system)?.to_ascii_lowercase();
            if !quench_intl::valid_unicode_type(&requested) {
                return Err(self.range_error(p, "invalid numberingSystem".into()));
            }
            Some(requested)
        };
        let numbering_system = option_numbering_system
            .as_ref()
            .filter(|value| quench_intl::NUMBERING_SYSTEMS.contains(&value.as_str()))
            .or_else(|| {
                locale_numbering_system
                    .as_ref()
                    .filter(|value| quench_intl::NUMBERING_SYSTEMS.contains(&value.as_str()))
            })
            .cloned()
            .unwrap_or_else(|| quench_intl::default_numbering_system(&locale).into());
        if locale_numbering_system.as_ref() != Some(&numbering_system)
            && option_numbering_system.as_ref().is_some()
        {
            locale = locale
                .split_once("-u-")
                .map_or(locale.clone(), |(base, _)| base.into());
        }
        let style =
            self.string_option(p, options, "style", "long", &["long", "short", "narrow"])?;
        let numeric = self.string_option(p, options, "numeric", "always", &["always", "auto"])?;
        let prototype_atom = self.intern_atom("prototype");
        let candidate = self.get_property(p, new_target, prototype_atom)?;
        let realm = self.function_realm(p, new_target)?;
        let prototype = if self.is_object_like(candidate) {
            candidate
        } else {
            self.intl_relative_time_format_prototypes
                .get(&realm)
                .copied()
                .unwrap_or(self.object_proto)
        };
        let instance = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(instance, RELATIVE_LOCALE_SLOT, &locale)?;
        self.set_hidden_string(instance, RELATIVE_STYLE_SLOT, &style)?;
        self.set_hidden_string(instance, RELATIVE_NUMERIC_SLOT, &numeric)?;
        self.set_hidden_string(instance, RELATIVE_NUMBERING_SYSTEM_SLOT, &numbering_system)?;
        let _ = locale_matcher;
        Ok(instance)
    }

    pub(super) fn intl_relative_time_format_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let locale = self
            .hidden_string(this, RELATIVE_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible RelativeTimeFormat receiver".into()))?;
        let result = self.object();
        for (property, slot) in [
            ("locale", RELATIVE_LOCALE_SLOT),
            ("style", RELATIVE_STYLE_SLOT),
            ("numeric", RELATIVE_NUMERIC_SLOT),
            ("numberingSystem", RELATIVE_NUMBERING_SYSTEM_SLOT),
        ] {
            let value = self
                .hidden_string(this, slot)
                .unwrap_or_else(|| locale.clone());
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_named(p, result, property, value)?;
        }
        Ok(result)
    }

    pub(super) fn string_option(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        name: &str,
        default: &str,
        allowed: &[&str],
    ) -> Result<String, JsError> {
        let atom = self.intern_atom(name);
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(default.into());
        }
        let value = self.to_string(p, value)?;
        if allowed.contains(&value.as_str()) {
            Ok(value)
        } else {
            Err(self.range_error(p, format!("invalid {name}").into()))
        }
    }
}

struct RelativeTimePart {
    kind: &'static str,
    value: String,
    unit: bool,
}

fn locale_unicode_numbering_system(locale: &str) -> Option<String> {
    let (_, extension) = locale.split_once("-u-")?;
    let parts = extension.split('-').collect::<Vec<_>>();
    parts
        .windows(2)
        .find_map(|pair| (pair[0] == "nu" && pair[1].len() > 2).then(|| pair[1].to_owned()))
}

fn normalize_relative_unit(unit: &str) -> Option<&'static str> {
    match unit {
        "second" | "seconds" => Some("second"),
        "minute" | "minutes" => Some("minute"),
        "hour" | "hours" => Some("hour"),
        "day" | "days" => Some("day"),
        "week" | "weeks" => Some("week"),
        "month" | "months" => Some("month"),
        "quarter" | "quarters" => Some("quarter"),
        "year" | "years" => Some("year"),
        _ => None,
    }
}

fn relative_time_parts(
    value: f64,
    unit: &str,
    style: &str,
    numeric: &str,
    locale: &str,
    numbering_system: &str,
) -> Vec<RelativeTimePart> {
    if numeric == "auto"
        && let Some(text) = relative_time_auto_phrase(value, unit)
    {
        return vec![RelativeTimePart {
            kind: "literal",
            value: text.into(),
            unit: false,
        }];
    }
    let negative = value.is_sign_negative();
    let magnitude = value.abs();
    let (prefix, suffix, word) = if locale.starts_with("pl") {
        let word = if magnitude.fract() != 0.0 {
            polish_relative_fractional_word(unit, style)
        } else {
            polish_relative_word(unit, style, polish_relative_plural(magnitude))
        };
        (
            if negative { "" } else { "za " },
            if negative { " temu" } else { "" },
            word,
        )
    } else {
        let plural = magnitude != 1.0;
        let word = relative_unit_word(unit, style, plural);
        (
            if negative { "" } else { "in " },
            if negative { " ago" } else { "" },
            word,
        )
    };
    let mut parts = Vec::new();
    if !prefix.is_empty() {
        parts.push(RelativeTimePart {
            kind: "literal",
            value: prefix.into(),
            unit: false,
        });
    }
    let text = relative_number(magnitude, locale);
    for part in relative_number_parts(&text, locale, numbering_system) {
        parts.push(part);
    }
    parts.push(RelativeTimePart {
        kind: "literal",
        value: format!(" {word}{suffix}"),
        unit: false,
    });
    parts
}

fn relative_time_auto_phrase(value: f64, unit: &str) -> Option<&'static str> {
    match unit {
        "day" => match value {
            -1.0 => Some("yesterday"),
            0.0 => Some("today"),
            1.0 => Some("tomorrow"),
            _ => None,
        },
        "year" | "quarter" | "month" | "week" => {
            let period = match unit {
                "year" => "year",
                "quarter" => "quarter",
                "month" => "month",
                _ => "week",
            };
            match value {
                -1.0 => match period {
                    "year" => Some("last year"),
                    "quarter" => Some("last quarter"),
                    "month" => Some("last month"),
                    _ => Some("last week"),
                },
                0.0 => match period {
                    "year" => Some("this year"),
                    "quarter" => Some("this quarter"),
                    "month" => Some("this month"),
                    _ => Some("this week"),
                },
                1.0 => match period {
                    "year" => Some("next year"),
                    "quarter" => Some("next quarter"),
                    "month" => Some("next month"),
                    _ => Some("next week"),
                },
                _ => None,
            }
        }
        "hour" | "minute" | "second" if value == 0.0 => match unit {
            "hour" => Some("this hour"),
            "minute" => Some("this minute"),
            _ => Some("now"),
        },
        _ => None,
    }
}

fn relative_unit_word(unit: &str, style: &str, plural: bool) -> &'static str {
    match (unit, style, plural) {
        ("second", "long", false) => "second",
        ("second", "long", true) => "seconds",
        ("second", _, _) => "sec.",
        ("minute", "long", false) => "minute",
        ("minute", "long", true) => "minutes",
        ("minute", _, _) => "min.",
        ("hour", "long", false) => "hour",
        ("hour", "long", true) => "hours",
        ("hour", _, _) => "hr.",
        ("day", "long", false) => "day",
        ("day", "long", true) => "days",
        ("day", _, false) => "day",
        ("day", _, true) => "days",
        ("week", "long", false) => "week",
        ("week", "long", true) => "weeks",
        ("week", _, _) => "wk.",
        ("month", "long", false) => "month",
        ("month", "long", true) => "months",
        ("month", _, _) => "mo.",
        ("quarter", "long", false) => "quarter",
        ("quarter", "long", true) => "quarters",
        ("quarter", _, false) => "qtr.",
        ("quarter", _, true) => "qtrs.",
        ("year", "long", false) => "year",
        ("year", "long", true) => "years",
        ("year", _, _) => "yr.",
        _ => "",
    }
}

fn polish_relative_plural(value: f64) -> usize {
    if value == 1.0 {
        return 0;
    }
    let integer = value as u64;
    if value.fract() == 0.0
        && (2..=4).contains(&(integer % 10))
        && !(12..=14).contains(&(integer % 100))
    {
        1
    } else {
        2
    }
}

fn polish_relative_word(unit: &str, style: &str, plural: usize) -> &'static str {
    if style != "long" {
        return match unit {
            "second" if style == "narrow" => "s",
            "second" => "sek.",
            "minute" => "min",
            "hour" if style == "narrow" => "g.",
            "hour" => "godz.",
            "day" if plural == 0 => "dzień",
            "day" => "dni",
            "week" if plural == 0 => "tydz.",
            "week" => "tyg.",
            "month" => "mies.",
            "quarter" => "kw.",
            "year" if plural == 0 => "rok",
            "year" if plural == 1 => "lata",
            "year" => "lat",
            _ => "",
        };
    }
    match unit {
        "second" => ["sekundę", "sekundy", "sekund"][plural],
        "minute" => ["minutę", "minuty", "minut"][plural],
        "hour" => ["godzinę", "godziny", "godzin"][plural],
        "day" => ["dzień", "dni", "dni"][plural],
        "week" => ["tydzień", "tygodnie", "tygodni"][plural],
        "month" => ["miesiąc", "miesiące", "miesięcy"][plural],
        "quarter" => ["kwartał", "kwartały", "kwartałów"][plural],
        _ => ["rok", "lata", "lat"][plural],
    }
}

fn polish_relative_fractional_word(unit: &str, style: &str) -> &'static str {
    match unit {
        "second" if style == "narrow" => "s",
        "second" if style == "short" => "sek.",
        "second" => "sekundy",
        "minute" if style != "long" => "min",
        "minute" => "minuty",
        "hour" if style == "narrow" => "g.",
        "hour" if style == "short" => "godz.",
        "hour" => "godziny",
        "day" => "dnia",
        "week" if style != "long" => "tyg.",
        "week" => "tygodnia",
        "month" if style != "long" => "mies.",
        "month" => "miesiąca",
        "quarter" if style != "long" => "kw.",
        "quarter" => "kwartału",
        _ => "roku",
    }
}

fn relative_number(value: f64, locale: &str) -> String {
    let mut text = format!("{value:.RELATIVE_PARTS_MAX_FRACTION_DIGITS$}");
    if text.contains('.') {
        while text.ends_with('0') {
            text.pop();
        }
        if text.ends_with('.') {
            text.pop();
        }
    }
    let (integer, fraction) = text
        .split_once('.')
        .map_or((text.as_str(), None), |(integer, fraction)| {
            (integer, Some(fraction))
        });
    let mut grouped = String::new();
    for (index, digit) in integer.chars().enumerate() {
        if index > 0 && (integer.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if let Some(fraction) = fraction {
        grouped.push('.');
        grouped.push_str(fraction);
    }
    if locale.starts_with("pl") {
        let grouped = grouped.replace(',', "\u{a0}").replace('.', ",");
        if value < 10_000.0 {
            grouped.replace('\u{a0}', "")
        } else {
            grouped
        }
    } else {
        grouped
    }
}

fn relative_number_parts(
    text: &str,
    locale: &str,
    numbering_system: &str,
) -> Vec<RelativeTimePart> {
    let text = quench_intl::localize_digits(text.to_owned(), numbering_system);
    let mut parts = Vec::new();
    let mut digits = String::new();
    let mut fractional = false;
    for character in text.chars() {
        if character.is_ascii_digit() || (character.is_alphanumeric() && !character.is_ascii()) {
            digits.push(character);
            continue;
        }
        flush_relative_digits(&mut parts, &mut digits, fractional);
        match character {
            ',' if !locale.starts_with("pl") => parts.push(RelativeTimePart {
                kind: "group",
                value: character.to_string(),
                unit: false,
            }),
            '\u{a0}' => parts.push(RelativeTimePart {
                kind: "group",
                value: character.to_string(),
                unit: false,
            }),
            '.' | ',' if locale.starts_with("pl") => {
                parts.push(RelativeTimePart {
                    kind: "decimal",
                    value: character.to_string(),
                    unit: false,
                });
                fractional = true;
            }
            '.' => {
                parts.push(RelativeTimePart {
                    kind: "decimal",
                    value: character.to_string(),
                    unit: false,
                });
                fractional = true;
            }
            _ => parts.push(RelativeTimePart {
                kind: "integer",
                value: character.to_string(),
                unit: true,
            }),
        }
    }
    flush_relative_digits(&mut parts, &mut digits, fractional);
    for part in &mut parts {
        part.unit = true;
    }
    parts
}

fn flush_relative_digits(parts: &mut Vec<RelativeTimePart>, digits: &mut String, fractional: bool) {
    if digits.is_empty() {
        return;
    }
    parts.push(RelativeTimePart {
        kind: if fractional { "fraction" } else { "integer" },
        value: std::mem::take(digits),
        unit: true,
    });
}
