use super::*;

const DURATION_FORMAT_LOCALE_SLOT: &str = "\0rqj:intl-duration-format-locale";
const DURATION_FORMAT_NUMBERING_SLOT: &str = "\0rqj:intl-duration-format-numbering";
const DURATION_FORMAT_STYLE_SLOT: &str = "\0rqj:intl-duration-format-style";
const DURATION_FORMAT_BOUND_SLOT: &str = "\0rqj:intl-duration-format-bound";
const DURATION_FORMAT_FRACTIONAL_DIGITS_SLOT: &str = "\0rqj:intl-duration-format-fractional-digits";
const DURATION_UNITS: &[&str] = &[
    "years",
    "months",
    "weeks",
    "days",
    "hours",
    "minutes",
    "seconds",
    "milliseconds",
    "microseconds",
    "nanoseconds",
];
const DURATION_OPTION_ORDER: &[&str] = &[
    "localeMatcher",
    "numberingSystem",
    "style",
    "years",
    "yearsDisplay",
    "months",
    "monthsDisplay",
    "weeks",
    "weeksDisplay",
    "days",
    "daysDisplay",
    "hours",
    "hoursDisplay",
    "minutes",
    "minutesDisplay",
    "seconds",
    "secondsDisplay",
    "milliseconds",
    "millisecondsDisplay",
    "microseconds",
    "microsecondsDisplay",
    "nanoseconds",
    "nanosecondsDisplay",
    "fractionalDigits",
];

#[derive(Clone)]
struct DurationFormatOptions {
    locale: String,
    numbering_system: String,
    style: String,
    units: Vec<String>,
    displays: Vec<String>,
    fractional_digits: Option<usize>,
}

#[derive(Clone)]
struct DurationPart {
    kind: &'static str,
    value: String,
    unit: Option<&'static str>,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_duration_format_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlDurationFormat, global, global);
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_duration_format_constructors
            .insert(global, constructor);
        self.realm.intrinsics.intl_duration_format_prototypes
            .insert(global, prototype);
        self.set_builtin_function_name(constructor, "DurationFormat")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            immutable_duration_property(),
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.DurationFormat")?;
        let format = self.native_with_realm(Native::IntlDurationFormatFormat, global, global);
        self.set_builtin_function_name(format, "format")?;
        self.set_builtin_value_named(prototype, "format", format)?;
        for (name, native) in [
            ("formatToParts", Native::IntlDurationFormatFormatToParts),
            ("resolvedOptions", Native::IntlDurationFormatResolvedOptions),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        let supported =
            self.native_with_realm(Native::IntlDurationFormatSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.set_builtin_value_named(intl, "DurationFormat", constructor)
    }

    pub(super) fn intl_duration_format_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([new_target]), |vm| {
            let prototype = vm.intl_instance_prototype(p, new_target, Native::IntlDurationFormat)?;
            vm.with_call_roots([prototype], |vm| {
                let locales = vm.canonical_locale_list(p, args.first().copied())?;
                let requested_locale = locales.first().cloned().unwrap_or_else(|| "en-US".into());
                let options = vm.duration_format_options(p, args.get(1).copied(), &requested_locale)?;
                let instance = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
                vm.write_duration_format_slots(instance, &options)?;
                Ok(instance)
            })
        })
    }

    fn duration_format_options(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
        locale: &str,
    ) -> Result<DurationFormatOptions, JsError> {
        let options = self.get_options_object(p, value)?;
        let mut raw = FxHashMap::default();
        for name in DURATION_OPTION_ORDER {
            let atom = self.intern_atom(name);
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                continue;
            }
            let value = if *name == "fractionalDigits" {
                let number = self.to_number(p, value)?;
                if !number.is_finite() || number.fract() != 0.0 || !(0.0..=9.0).contains(&number) {
                    return Err(self.range_error(p, "invalid fractionalDigits".into()));
                }
                number.to_string()
            } else {
                self.to_string(p, value)?
            };
            raw.insert(*name, value);
        }
        self.resolve_duration_format_options(p, raw, locale)
    }

    fn resolve_duration_format_options(
        &mut self,
        p: &ResidualProgram,
        raw: FxHashMap<&'static str, String>,
        requested_locale: &str,
    ) -> Result<DurationFormatOptions, JsError> {
        if let Some(matcher) = raw.get("localeMatcher")
            && !matches!(matcher.as_str(), "lookup" | "best fit")
        {
            return Err(self.range_error(p, "invalid localeMatcher".into()));
        }
        let style = raw.get("style").cloned().unwrap_or_else(|| "short".into());
        if !matches!(style.as_str(), "long" | "short" | "narrow" | "digital") {
            return Err(self.range_error(p, "invalid style".into()));
        }
        let requested_numbering = raw.get("numberingSystem").cloned();
        if let Some(numbering) = requested_numbering.as_deref()
            && !quench_intl::valid_unicode_type(numbering)
        {
            return Err(self.range_error(p, "invalid numberingSystem".into()));
        }
        let locale_numbering = locale_numbering_system(requested_locale);
        let selected_option =
            requested_numbering.filter(|numbering| quench_intl::valid_numbering_system(numbering));
        let numbering_system = selected_option
            .clone()
            .unwrap_or_else(|| locale_numbering.clone());
        let locale_extension = locale_numbering_extension(requested_locale);
        let locale = match selected_option.as_deref() {
            Some(value) if locale_extension.as_deref() == Some(value) => {
                requested_locale.to_owned()
            }
            Some(_) => strip_numbering_extension(requested_locale),
            None if locale_extension
                .as_deref()
                .is_some_and(quench_intl::valid_numbering_system) =>
            {
                requested_locale.to_owned()
            }
            None if locale_extension.is_some() => strip_numbering_extension(requested_locale),
            None => requested_locale.to_owned(),
        };
        let mut units = Vec::with_capacity(DURATION_UNITS.len());
        let mut displays = Vec::with_capacity(DURATION_UNITS.len());
        let mut previous_numeric = false;
        for (index, unit) in DURATION_UNITS.iter().enumerate() {
            let explicit = raw.get(unit).cloned();
            let value = explicit.clone().unwrap_or_else(|| {
                default_duration_unit_style(unit, index, previous_numeric, &style)
            });
            if !valid_duration_unit_style(index, &value)
                || (previous_numeric
                    && explicit.is_some()
                    && !matches!(value.as_str(), "numeric" | "2-digit"))
            {
                return Err(self.range_error(p, "invalid unit style".into()));
            }
            previous_numeric = matches!(value.as_str(), "numeric" | "2-digit");
            let display_key = display_option_name(unit);
            let display = raw.get(display_key).cloned().unwrap_or_else(|| {
                default_duration_display(explicit.is_some(), &value, &style, unit)
            });
            if !matches!(display.as_str(), "auto" | "always") {
                return Err(self.range_error(p, "invalid display".into()));
            }
            units.push(value);
            displays.push(display);
        }
        let fractional_digits = raw
            .get("fractionalDigits")
            .and_then(|value| value.parse::<usize>().ok());
        Ok(DurationFormatOptions {
            locale,
            numbering_system,
            style,
            units,
            displays,
            fractional_digits,
        })
    }

    fn write_duration_format_slots(
        &mut self,
        instance: Value,
        options: &DurationFormatOptions,
    ) -> Result<(), JsError> {
        for (slot, value) in [
            (DURATION_FORMAT_LOCALE_SLOT, options.locale.as_str()),
            (
                DURATION_FORMAT_NUMBERING_SLOT,
                options.numbering_system.as_str(),
            ),
            (DURATION_FORMAT_STYLE_SLOT, options.style.as_str()),
        ] {
            self.set_hidden_string(instance, slot, value)?;
        }
        for ((unit, style), display) in DURATION_UNITS
            .iter()
            .zip(options.units.iter())
            .zip(options.displays.iter())
        {
            self.set_hidden_string(instance, &unit_slot(unit), style)?;
            self.set_hidden_string(instance, &display_slot(unit), display)?;
        }
        if let Some(digits) = options.fractional_digits {
            self.set_hidden_value(
                instance,
                DURATION_FORMAT_FRACTIONAL_DIGITS_SLOT,
                Value::number(digits as f64),
            )?;
        }
        Ok(())
    }

    pub(super) fn intl_duration_format_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native != Native::IntlDurationFormatFormatGetter {
            self.duration_format_locale(p, this)?;
        }
        match native {
            Native::IntlDurationFormatFormatGetter => self.duration_format_getter(p, this),
            Native::IntlDurationFormatFormat => self.duration_format_format(p, this, args),
            Native::IntlDurationFormatFormatToParts => self.duration_format_to_parts(p, this, args),
            Native::IntlDurationFormatResolvedOptions => {
                self.duration_format_resolved_options(p, this)
            }
            _ => Err(JsError("invalid Intl.DurationFormat method".into())),
        }
    }

    fn duration_format_getter(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        if let Some(bound) = self.hidden_value(this, DURATION_FORMAT_BOUND_SLOT) {
            return Ok(bound);
        }
        let function = self.native_with_realm(
            Native::IntlDurationFormatFormat,
            Value::NULL,
            self.realm.globals,
        );
        if self.duration_format_locale(p, this).is_err() {
            return Ok(function);
        }
        let bound = self.bind_function(p, function, &[this])?;
        self.set_hidden_value(this, DURATION_FORMAT_BOUND_SLOT, bound)?;
        Ok(bound)
    }

    fn duration_format_format(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let parts = self.duration_format_parts(p, this, args.first().copied())?;
        let text = parts
            .iter()
            .map(|part| part.value.as_str())
            .collect::<String>();
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    fn duration_format_to_parts(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let parts = self.duration_format_parts(p, this, args.first().copied())?;
        let mut values = Vec::with_capacity(parts.len());
        for part in parts {
            let object = self.object();
            let kind_atom = self.intern_atom("type");
            let kind = self.heap.alloc(Cell::String(part.kind.into()));
            self.set_property(object, kind_atom, kind)?;
            let value_atom = self.intern_atom("value");
            let value = self.heap.alloc(Cell::String(part.value.into()));
            self.set_property(object, value_atom, value)?;
            if let Some(unit) = part.unit {
                let unit_atom = self.intern_atom("unit");
                let unit = self.heap.alloc(Cell::String(unit.into()));
                self.set_property(object, unit_atom, unit)?;
            }
            values.push(object);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    fn duration_format_parts(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
        value: Option<Value>,
    ) -> Result<Vec<DurationPart>, JsError> {
        let input = value.unwrap_or(Value::UNDEFINED);
        let values = self.duration_record(p, input)?;
        self.validate_duration_fields(p, &values)?;
        let options = self.duration_format_read_options(p, formatter)?;
        Ok(format_duration_parts(&options, values))
    }

    fn duration_format_read_options(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
    ) -> Result<DurationFormatOptions, JsError> {
        let locale = self.duration_format_locale(p, formatter)?;
        let numbering_system = self
            .hidden_string(formatter, DURATION_FORMAT_NUMBERING_SLOT)
            .unwrap_or_else(|| "latn".into());
        let style = self
            .hidden_string(formatter, DURATION_FORMAT_STYLE_SLOT)
            .unwrap_or_else(|| "short".into());
        let units = DURATION_UNITS
            .iter()
            .map(|unit| {
                self.hidden_string(formatter, &unit_slot(unit))
                    .unwrap_or_else(|| "short".into())
            })
            .collect();
        let displays = DURATION_UNITS
            .iter()
            .map(|unit| {
                self.hidden_string(formatter, &display_slot(unit))
                    .unwrap_or_else(|| "auto".into())
            })
            .collect();
        let fractional_digits = self
            .hidden_value(formatter, DURATION_FORMAT_FRACTIONAL_DIGITS_SLOT)
            .and_then(Value::as_number)
            .map(|value| value as usize);
        Ok(DurationFormatOptions {
            locale,
            numbering_system,
            style,
            units,
            displays,
            fractional_digits,
        })
    }

    fn duration_format_resolved_options(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
    ) -> Result<Value, JsError> {
        let options = self.duration_format_read_options(p, formatter)?;
        let result = self.object();
        for (key, value) in [
            ("locale", options.locale),
            ("numberingSystem", options.numbering_system),
            ("style", options.style),
        ] {
            self.set_intl_string_property(result, key, &value)?;
        }
        for ((unit, style), display) in DURATION_UNITS
            .iter()
            .zip(options.units.iter())
            .zip(options.displays.iter())
        {
            self.set_intl_string_property(result, unit, style)?;
            self.set_intl_string_property(result, &format!("{unit}Display"), display)?;
        }
        if let Some(digits) = options.fractional_digits {
            let atom = self.intern_atom("fractionalDigits");
            self.set_property(result, atom, Value::number(digits as f64))?;
        }
        Ok(result)
    }

    fn duration_format_locale(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
    ) -> Result<String, JsError> {
        self.hidden_string(formatter, DURATION_FORMAT_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "not a DurationFormat object".into()))
    }

}

fn immutable_duration_property() -> PropertyAttributes {
    PropertyAttributes {
        writable: false,
        enumerable: false,
        configurable: false,
        accessor: false,
        getter: None,
        setter: None,
    }
}

fn unit_slot(unit: &str) -> String {
    format!("\0rqj:intl-duration-format:{unit}")
}

fn display_slot(unit: &str) -> String {
    format!("\0rqj:intl-duration-format:{unit}Display")
}

fn display_option_name(unit: &str) -> &'static str {
    match unit {
        "years" => "yearsDisplay",
        "months" => "monthsDisplay",
        "weeks" => "weeksDisplay",
        "days" => "daysDisplay",
        "hours" => "hoursDisplay",
        "minutes" => "minutesDisplay",
        "seconds" => "secondsDisplay",
        "milliseconds" => "millisecondsDisplay",
        "microseconds" => "microsecondsDisplay",
        "nanoseconds" => "nanosecondsDisplay",
        _ => unreachable!("duration unit table is closed"),
    }
}

fn default_duration_unit_style(
    unit: &str,
    index: usize,
    previous_numeric: bool,
    style: &str,
) -> String {
    if style == "digital" {
        return match unit {
            "hours" => "numeric",
            "minutes" | "seconds" => "2-digit",
            _ if index > 3 => "numeric",
            _ => "short",
        }
        .into();
    }
    if !previous_numeric {
        return "short".into();
    }
    match unit {
        "minutes" | "seconds" => "2-digit".into(),
        _ => "numeric".into(),
    }
}

fn valid_duration_unit_style(index: usize, style: &str) -> bool {
    if index < 4 {
        matches!(style, "long" | "short" | "narrow")
    } else {
        matches!(style, "long" | "short" | "narrow" | "numeric" | "2-digit")
    }
}

fn default_duration_display(explicit: bool, unit_style: &str, style: &str, unit: &str) -> String {
    if (explicit && !matches!(unit_style, "numeric" | "2-digit"))
        || (style == "digital" || matches!(unit_style, "numeric" | "2-digit"))
            && matches!(unit, "hours" | "minutes" | "seconds")
    {
        "always"
    } else {
        "auto"
    }
    .into()
}

fn locale_numbering_system(locale: &str) -> String {
    locale
        .split_once("-u-nu-")
        .and_then(|(_, ext)| ext.split('-').next())
        .filter(|system| quench_intl::valid_numbering_system(system))
        .map(str::to_owned)
        .unwrap_or_else(|| quench_intl::default_numbering_system(locale).into())
}

fn locale_numbering_extension(locale: &str) -> Option<String> {
    locale
        .split_once("-u-nu-")
        .and_then(|(_, extension)| extension.split('-').next())
        .map(str::to_owned)
}

fn strip_numbering_extension(locale: &str) -> String {
    locale
        .split_once("-u-nu-")
        .map_or_else(|| locale.into(), |(base, _)| base.into())
}

fn format_duration_parts(options: &DurationFormatOptions, fields: [f64; 10]) -> Vec<DurationPart> {
    if options.style == "digital" {
        return digital_duration_parts(options, fields);
    }
    let negative = fields.iter().any(|value| *value < 0.0);
    let mut groups: Vec<Vec<DurationPart>> = Vec::new();
    let mut need_separator = false;
    let mut show_negative = negative;
    for (index, (unit, raw)) in DURATION_UNITS.iter().zip(fields).enumerate() {
        let unit_style = &options.units[index];
        let unit_name = unit.trim_end_matches('s');
        let mut number = raw.to_string();
        let mut combined = false;
        if matches!(unit, &"seconds" | &"milliseconds" | &"microseconds")
            && options
                .units
                .get(index + 1)
                .is_some_and(|style| style == "numeric")
        {
            let exponent = match *unit {
                "seconds" => 9,
                "milliseconds" => 6,
                _ => 3,
            };
            number = duration_fraction(fields, index, exponent, options.fractional_digits);
            combined = true;
        }
        let display_required = need_separator
            && ((*unit == "minutes"
                && (options.displays[6..]
                    .iter()
                    .any(|display| display == "always")
                    || fields[6..].iter().any(|value| *value != 0.0)))
                || (*unit == "seconds" && fields[7..].iter().any(|value| *value != 0.0)));
        if raw == 0.0
            && !number.contains('.')
            && options.displays[index] == "auto"
            && !display_required
        {
            continue;
        }
        if need_separator {
            if let Some(group) = groups.last_mut() {
                group.push(DurationPart {
                    kind: "literal",
                    value: ":".into(),
                    unit: Some(unit_name),
                });
            }
        } else {
            groups.push(Vec::new());
        }
        let signed = show_negative && negative;
        show_negative = false;
        let unsigned = number.trim_start_matches('-');
        let text = unsigned.to_owned();
        let (integer, fraction) = text.split_once('.').unwrap_or((&text, ""));
        let padding = if (unit_style == "2-digit" || (*unit == "seconds" && need_separator))
            && integer.trim_start_matches('-').len() < 2
        {
            format!(
                "{}0{}",
                if signed { "-" } else { "" },
                integer.trim_start_matches('-')
            )
        } else {
            integer.to_owned()
        };
        let group = groups.last_mut().expect("duration group was just created");
        if signed {
            group.push(DurationPart {
                kind: "minusSign",
                value: "-".into(),
                unit: Some(unit_name),
            });
        }
        group.push(DurationPart {
            kind: "integer",
            value: localize_duration_number(&padding, &options.numbering_system),
            unit: Some(unit_name),
        });
        if !fraction.is_empty() {
            group.push(DurationPart {
                kind: "decimal",
                value: ".".into(),
                unit: Some(unit_name),
            });
            group.push(DurationPart {
                kind: "fraction",
                value: localize_duration_number(fraction, &options.numbering_system),
                unit: Some(unit_name),
            });
        }
        if !matches!(unit_style.as_str(), "numeric" | "2-digit") {
            if unit_style != "narrow" {
                group.push(DurationPart {
                    kind: "literal",
                    value: " ".into(),
                    unit: Some(unit_name),
                });
            }
            group.push(DurationPart {
                kind: "unit",
                value: duration_unit_label(unit, unit_style, raw.abs()),
                unit: Some(unit_name),
            });
        }
        need_separator = matches!(unit_style.as_str(), "numeric" | "2-digit");
        if combined {
            break;
        }
    }
    let mut parts = Vec::new();
    let group_count = groups.len();
    for (index, group) in groups.into_iter().enumerate() {
        if index > 0 {
            parts.push(DurationPart {
                kind: "literal",
                value: super::intl_list_format::list_separator(
                    index,
                    group_count,
                    &options.locale,
                    "unit",
                    &options.style,
                )
                .into(),
                unit: None,
            });
        }
        parts.extend(group);
    }
    parts
}

fn digital_duration_parts(options: &DurationFormatOptions, fields: [f64; 10]) -> Vec<DurationPart> {
    let negative = fields.iter().any(|value| *value < 0.0);
    let hours = fields[4].abs();
    let minutes = fields[5].abs();
    let seconds = duration_fraction(fields, 6, 9, options.fractional_digits);
    let (seconds_integer, seconds_fraction) = seconds.split_once('.').unwrap_or((&seconds, ""));
    let show_hours = options.displays[4] == "always" || hours != 0.0;
    let mut parts = Vec::new();
    for index in 0..4 {
        let value = fields[index].abs();
        if value == 0.0 && options.displays[index] == "auto" {
            continue;
        }
        if !parts.is_empty() {
            parts.push(DurationPart {
                kind: "literal",
                value: ", ".into(),
                unit: None,
            });
        }
        let number = if index == 3 {
            super::intl_number::group_decimal_integer(&value.to_string())
        } else {
            value.to_string()
        };
        parts.push(DurationPart {
            kind: "integer",
            value: number,
            unit: Some(DURATION_UNITS[index].trim_end_matches('s')),
        });
        parts.push(DurationPart {
            kind: "literal",
            value: " ".into(),
            unit: Some(DURATION_UNITS[index].trim_end_matches('s')),
        });
        parts.push(DurationPart {
            kind: "unit",
            value: if index == 3 && value != 1.0 {
                "days".into()
            } else {
                duration_unit_label(DURATION_UNITS[index], &options.units[index], value)
            },
            unit: Some(DURATION_UNITS[index].trim_end_matches('s')),
        });
    }
    let has_prefix = !parts.is_empty();
    let clock_start = parts.len();
    if show_hours {
        push_duration_number(&mut parts, hours, "hour", false, &options.numbering_system);
        parts.push(DurationPart {
            kind: "literal",
            value: ":".into(),
            unit: None,
        });
    }
    push_duration_number(
        &mut parts,
        minutes,
        "minute",
        true,
        &options.numbering_system,
    );
    parts.push(DurationPart {
        kind: "literal",
        value: ":".into(),
        unit: None,
    });
    push_duration_text(
        &mut parts,
        seconds_integer,
        "second",
        true,
        &options.numbering_system,
    );
    if !seconds_fraction.is_empty() {
        parts.push(DurationPart {
            kind: "decimal",
            value: ".".into(),
            unit: Some("second"),
        });
        parts.push(DurationPart {
            kind: "fraction",
            value: localize_duration_number(seconds_fraction, &options.numbering_system),
            unit: Some("second"),
        });
    }
    if has_prefix {
        parts.insert(
            clock_start,
            DurationPart {
                kind: "literal",
                value: ", ".into(),
                unit: None,
            },
        );
    }
    if negative {
        if let Some(first) = parts.iter().position(|part| part.kind == "integer") {
            let unit = parts[first].unit;
            parts.insert(
                first,
                DurationPart {
                    kind: "minusSign",
                    value: "-".into(),
                    unit,
                },
            );
        }
    }
    parts
}

fn duration_fraction(
    fields: [f64; 10],
    index: usize,
    exponent: usize,
    digits: Option<usize>,
) -> String {
    let mut fraction = 0i128;
    for offset in 1..=3 {
        if let Some(subunit) = fields.get(index + offset) {
            let divisor = match (exponent, offset) {
                (9, 1) => 1_000_000,
                (9, 2) => 1_000,
                (9, 3) => 1,
                (6, 1) => 1_000,
                (6, 2) => 1,
                (3, 1) => 1,
                _ => continue,
            };
            fraction += *subunit as i128 * divisor;
        }
    }
    let whole = fields[index] as i128;
    let scale = 10i128.pow(exponent as u32);
    let value = whole * scale + fraction;
    let sign = if value < 0 { "-" } else { "" };
    let absolute = value.saturating_abs();
    let integer = (absolute / scale).to_string();
    let raw_fraction = format!("{:0width$}", absolute % scale, width = exponent);
    let digit_limit = digits.unwrap_or(exponent).min(exponent);
    let mut fractional = raw_fraction[..digit_limit].to_owned();
    if digits.is_none() {
        while fractional.ends_with('0') {
            fractional.pop();
        }
    }
    if fractional.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}.{fractional}")
    }
}

fn push_duration_number(
    parts: &mut Vec<DurationPart>,
    value: f64,
    unit: &'static str,
    pad: bool,
    numbering: &str,
) {
    push_duration_text(parts, &value.to_string(), unit, pad, numbering);
}

fn push_duration_text(
    parts: &mut Vec<DurationPart>,
    value: &str,
    unit: &'static str,
    pad: bool,
    numbering: &str,
) {
    let absolute = value.trim_start_matches('-');
    let padded = if pad && absolute.len() < 2 {
        format!("0{absolute}")
    } else {
        absolute.to_owned()
    };
    parts.push(DurationPart {
        kind: "integer",
        value: localize_duration_number(&padded, numbering),
        unit: Some(unit),
    });
}

fn duration_unit_label(unit: &str, style: &str, value: f64) -> String {
    let (long, short, narrow) = match unit {
        "years" => (if value == 1.0 { "year" } else { "years" }, "yr", "y"),
        "months" => (if value == 1.0 { "month" } else { "months" }, "mo", "m"),
        "weeks" => (if value == 1.0 { "week" } else { "weeks" }, "wk", "w"),
        "days" => (
            if value == 1.0 { "day" } else { "days" },
            if value == 1.0 { "day" } else { "days" },
            "d",
        ),
        "hours" => (if value == 1.0 { "hour" } else { "hours" }, "hr", "h"),
        "minutes" => (if value == 1.0 { "minute" } else { "minutes" }, "min", "m"),
        "seconds" => (if value == 1.0 { "second" } else { "seconds" }, "sec", "s"),
        "milliseconds" => (
            if value == 1.0 {
                "millisecond"
            } else {
                "milliseconds"
            },
            "ms",
            "ms",
        ),
        "microseconds" => (
            if value == 1.0 {
                "microsecond"
            } else {
                "microseconds"
            },
            "μs",
            "μs",
        ),
        _ => (
            if value == 1.0 {
                "nanosecond"
            } else {
                "nanoseconds"
            },
            "ns",
            "ns",
        ),
    };
    match style {
        "long" => long.into(),
        "narrow" => narrow.into(),
        _ => short.into(),
    }
}

fn localize_duration_number(value: &str, numbering: &str) -> String {
    quench_intl::localize_digits(value.into(), numbering)
}
