use super::*;

const DEFAULT_NUMBER_FORMAT_LOCALE: &str = "en-US";
const NUMBER_FORMAT_MAX_FRACTION_DIGITS: f64 = 100.0;
const NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS: f64 = 21.0;
const NUMBER_FORMAT_LOCALE_SLOT: &str = "\0rqj:intl-number-format-locale";
const NUMBER_FORMAT_STYLE_SLOT: &str = "\0rqj:intl-number-format-style";
const NUMBER_FORMAT_CURRENCY_SLOT: &str = "\0rqj:intl-number-format-currency";
const NUMBER_FORMAT_NUMBERING_SYSTEM_SLOT: &str = "\0rqj:intl-number-format-numbering-system";
const NUMBER_FORMAT_UNIT_SLOT: &str = "\0rqj:intl-number-format-unit";
const NUMBER_FORMAT_UNIT_DISPLAY_SLOT: &str = "\0rqj:intl-number-format-unit-display";
const NUMBER_FORMAT_GROUPING_SLOT: &str = "\0rqj:intl-number-format-use-grouping";
const NUMBER_FORMAT_MIN_INTEGER_SLOT: &str = "\0rqj:intl-number-format-min-integer";
const NUMBER_FORMAT_SIGN_DISPLAY_SLOT: &str = "\0rqj:intl-number-format-sign-display";
const NUMBER_FORMAT_BOUND_SLOT: &str = "\0rqj:intl-number-format-bound";
const NUMBER_FORMAT_MIN_FRACTION_SLOT: &str = "\0rqj:intl-number-format-min-fraction";
const NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT: &str = "\0rqj:intl-number-format-max-significant";
const NUMBER_FORMAT_MAX_FRACTION_SLOT: &str = "\0rqj:intl-number-format-max-fraction";
const NUMBER_FORMAT_ROUNDING_MODE_SLOT: &str = "\0rqj:intl-number-format-rounding-mode";

struct NumberFormatOptions {
    style: String,
    currency: Option<String>,
    numbering_system: Option<String>,
    minimum_fraction_digits: usize,
    maximum_fraction_digits: Option<usize>,
    maximum_significant_digits: Option<usize>,
    unit: Option<String>,
    unit_display: String,
    use_grouping: bool,
    minimum_integer_digits: usize,
    sign_display: String,
    rounding_mode: String,
}

impl<H: Host> Vm<H> {
    pub(super) fn intl_format_primitive(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self
            .intl_number_format_constructors
            .get(&self.realm.globals)
            .copied()
            .ok_or_else(|| JsError("Intl.NumberFormat intrinsic is not installed".into()))?;
        let formatter = self.construct_value(p, constructor, args)?;
        let format_atom = self.intern_atom("format");
        let format = self.get_property(p, formatter, format_atom)?;
        self.call_value(p, format, formatter, &[value])
    }

    pub(super) fn install_intl_number_format_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let intl = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.install_intl_namespace_for_realm(program, intl, global, object_prototype)?;
        let constructor = self.native_with_realm(Native::IntlNumberFormat, global, global);
        self.intl_number_format_constructors
            .insert(global, constructor);
        self.set_builtin_function_name(constructor, "NumberFormat")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
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
        let resolved_options =
            self.native_with_realm(Native::IntlNumberFormatResolvedOptions, global, global);
        self.set_builtin_function_name(resolved_options, "resolvedOptions")?;
        self.set_builtin_value_named(prototype, "resolvedOptions", resolved_options)?;
        let format_getter =
            self.native_with_realm(Native::IntlNumberFormatFormatGetter, global, global);
        self.set_builtin_function_name(format_getter, "get format")?;
        let format_atom = self.intern_atom("format");
        self.set_builtin_value_named(prototype, "format", format_getter)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::string(format_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(format_getter),
                setter: None,
            },
        );
        let format_to_parts =
            self.native_with_realm(Native::IntlNumberFormatFormatToParts, global, global);
        self.set_builtin_function_name(format_to_parts, "formatToParts")?;
        self.set_builtin_value_named(prototype, "formatToParts", format_to_parts)?;
        self.set_builtin_value_named(intl, "NumberFormat", constructor)?;
        self.install_intl_collator_for_realm(program, intl, global, object_prototype)?;
        self.install_intl_date_time_format_for_realm(intl, global, object_prototype)?;
        self.install_intl_display_names_for_realm(program, intl, global, object_prototype)?;
        self.install_intl_duration_format_for_realm(intl, global, object_prototype)?;
        self.install_intl_list_format_for_realm(intl, global, object_prototype)?;
        self.set_builtin_value_named(global, "Intl", intl)?;
        let _ = program;
        Ok(())
    }

    pub(super) fn intl_number_format_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let locale = self.number_format_locale(p, args.first().copied())?;
        let options = self.number_format_options(p, args.get(1).copied())?;
        let prototype = self.number_format_instance_prototype(p, new_target)?;
        let formatter = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(formatter, NUMBER_FORMAT_LOCALE_SLOT, &locale)?;
        self.set_hidden_string(formatter, NUMBER_FORMAT_STYLE_SLOT, &options.style)?;
        self.set_hidden_string(
            formatter,
            NUMBER_FORMAT_CURRENCY_SLOT,
            options.currency.as_deref().unwrap_or(""),
        )?;
        self.set_hidden_string(
            formatter,
            NUMBER_FORMAT_NUMBERING_SYSTEM_SLOT,
            options.numbering_system.as_deref().unwrap_or(""),
        )?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_MIN_FRACTION_SLOT,
            Value::number(options.minimum_fraction_digits as f64),
        )?;
        self.set_hidden_string(
            formatter,
            NUMBER_FORMAT_UNIT_SLOT,
            options.unit.as_deref().unwrap_or(""),
        )?;
        self.set_hidden_string(
            formatter,
            NUMBER_FORMAT_UNIT_DISPLAY_SLOT,
            &options.unit_display,
        )?;
        self.set_hidden_string(
            formatter,
            NUMBER_FORMAT_SIGN_DISPLAY_SLOT,
            &options.sign_display,
        )?;
        self.set_hidden_string(
            formatter,
            NUMBER_FORMAT_ROUNDING_MODE_SLOT,
            &options.rounding_mode,
        )?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_GROUPING_SLOT,
            if options.use_grouping {
                Value::TRUE
            } else {
                Value::FALSE
            },
        )?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_MIN_INTEGER_SLOT,
            Value::number(options.minimum_integer_digits as f64),
        )?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT,
            options
                .maximum_significant_digits
                .map_or(Value::UNDEFINED, |digits| Value::number(digits as f64)),
        )?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_MAX_FRACTION_SLOT,
            options
                .maximum_fraction_digits
                .map_or(Value::UNDEFINED, |digits| Value::number(digits as f64)),
        )?;
        Ok(formatter)
    }

    fn number_format_instance_prototype(
        &mut self,
        p: &ResidualProgram,
        new_target: Value,
    ) -> Result<Value, JsError> {
        let prototype_key = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_key)?;
        Ok(if self.is_object_like(prototype) {
            prototype
        } else {
            self.object_proto
        })
    }

    pub(super) fn intl_number_format_format_getter(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        if self.hidden_string(this, NUMBER_FORMAT_LOCALE_SLOT).is_none() {
            return Err(self.type_error(p, "incompatible NumberFormat receiver".into()));
        }
        if let Some(bound) = self.hidden_value(this, NUMBER_FORMAT_BOUND_SLOT) {
            return Ok(bound);
        }
        let function = self.native_with_realm(
            Native::IntlNumberFormatFormat,
            Value::NULL,
            self.realm.globals,
        );
        let bound = self.bind_function(p, function, &[this])?;
        self.override_builtin_function_name(bound, "")?;
        self.set_hidden_value(this, NUMBER_FORMAT_BOUND_SLOT, bound)?;
        Ok(bound)
    }

    fn number_format_locale(
        &mut self,
        p: &ResidualProgram,
        locales: Option<Value>,
    ) -> Result<String, JsError> {
        let Some(locales) = locales.filter(|value| !value.is_undefined()) else {
            return Ok(DEFAULT_NUMBER_FORMAT_LOCALE.into());
        };
        if locales.is_null() {
            return Err(self.type_error(p, "locales must not be null".into()));
        }
        if matches!(self.heap.get(locales), Some(Cell::Array { .. })) {
            return self.number_format_locale_list(p, locales);
        }
        let locale = self.to_string(p, locales)?;
        if !valid_locale_identifier(&locale) {
            return Err(self.range_error(p, "invalid locale identifier".into()));
        }
        Ok(locale)
    }

    fn number_format_locale_list(
        &mut self,
        p: &ResidualProgram,
        locales: Value,
    ) -> Result<String, JsError> {
        let length = self.array_like_length(p, locales)?;
        let mut first = None;
        for index in 0..length {
            let locale = self.get_index(p, locales, Value::number(index as f64))?;
            if !matches!(self.heap.get(locale), Some(Cell::String(_)))
                && !self.is_object_like(locale)
            {
                return Err(self.type_error(p, "locale list elements must be strings".into()));
            }
            let locale = self.to_string(p, locale)?;
            if !valid_locale_identifier(&locale) {
                return Err(self.range_error(p, "invalid locale identifier".into()));
            }
            first.get_or_insert(locale);
        }
        Ok(first.unwrap_or_else(|| DEFAULT_NUMBER_FORMAT_LOCALE.into()))
    }

    fn number_format_options(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<NumberFormatOptions, JsError> {
        let Some(options) = options.filter(|value| !value.is_undefined()) else {
            return Ok(NumberFormatOptions {
                style: "decimal".into(),
                currency: None,
                numbering_system: None,
                minimum_fraction_digits: 0,
                maximum_fraction_digits: None,
                maximum_significant_digits: None,
                unit: None,
                unit_display: "short".into(),
                use_grouping: true,
                minimum_integer_digits: 1,
                sign_display: "auto".into(),
                rounding_mode: "halfExpand".into(),
            });
        };
        if options.is_null() {
            return Err(self.type_error(p, "options must not be null".into()));
        }
        let options = self.box_object(options)?;
        let mut style = "decimal".to_owned();
        let mut currency = None;
        let mut numbering_system = None;
        let mut unit = None;
        let mut unit_display = "short".to_owned();
        let mut use_grouping = true;
        let mut minimum_integer_digits = 1;
        let mut minimum_fraction_digits = 0;
        let mut maximum_fraction_digits = None;
        let mut sign_display = "auto".to_owned();
        let mut rounding_mode = "halfExpand".to_owned();
        let mut maximum_significant_digits = None;
        for key in quench_intl::NUMBER_FORMAT_OPTION_KEYS {
            let atom = self.intern_atom(key);
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                continue;
            }
            match *key {
                "localeMatcher" => {
                    let value = self.to_string(p, value)?;
                    if !matches!(value.as_str(), "lookup" | "best fit") {
                        return Err(self.range_error(p, "invalid localeMatcher".into()));
                    }
                }
                "style" => {
                    style = self.to_string(p, value)?;
                    if !matches!(style.as_str(), "decimal" | "percent" | "currency" | "unit") {
                        return Err(self.range_error(p, "invalid style".into()));
                    }
                }
                "currency" => {
                    let value = self.to_string(p, value)?.to_ascii_uppercase();
                    if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                        return Err(self.range_error(p, "invalid currency".into()));
                    }
                    currency = Some(value);
                }
                "numberingSystem" => {
                    let value = self.to_string(p, value)?.to_ascii_lowercase();
                    if !quench_intl::valid_unicode_type(&value) {
                        return Err(self.range_error(p, "invalid numberingSystem".into()));
                    }
                    numbering_system = quench_intl::NUMBERING_SYSTEMS
                        .contains(&value.as_str())
                        .then_some(value);
                }
                "unit" => {
                    let value = self.to_string(p, value)?;
                    if value.is_empty()
                        || !value
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    {
                        return Err(self.range_error(p, "invalid unit".into()));
                    }
                    unit = Some(value);
                }
                "unitDisplay" => {
                    unit_display = self.to_string(p, value)?;
                    if !matches!(unit_display.as_str(), "long" | "short" | "narrow") {
                        return Err(self.range_error(p, "invalid unitDisplay".into()));
                    }
                }
                "signDisplay" => {
                    sign_display = self.to_string(p, value)?;
                    if !matches!(
                        sign_display.as_str(),
                        "auto" | "never" | "always" | "exceptZero" | "negative"
                    ) {
                        return Err(self.range_error(p, "invalid signDisplay".into()));
                    }
                }
                "useGrouping" => use_grouping = self.truthy(value),
                "minimumIntegerDigits" => {
                    minimum_integer_digits = self.number_format_option_integer(
                        p,
                        value,
                        1.0,
                        21.0,
                        "minimumIntegerDigits",
                    )? as usize;
                }
                "minimumFractionDigits" => {
                    minimum_fraction_digits = self.number_format_option_integer(
                        p,
                        value,
                        0.0,
                        NUMBER_FORMAT_MAX_FRACTION_DIGITS,
                        "minimumFractionDigits",
                    )? as usize;
                }
                "maximumFractionDigits" => {
                    maximum_fraction_digits = Some(self.number_format_option_integer(
                        p,
                        value,
                        0.0,
                        NUMBER_FORMAT_MAX_FRACTION_DIGITS,
                        "maximumFractionDigits",
                    )? as usize);
                }
                "roundingMode" => {
                    rounding_mode = self.to_string(p, value)?;
                    if !matches!(
                        rounding_mode.as_str(),
                        "ceil"
                            | "floor"
                            | "expand"
                            | "trunc"
                            | "halfCeil"
                            | "halfFloor"
                            | "halfExpand"
                            | "halfTrunc"
                            | "halfEven"
                    ) {
                        return Err(self.range_error(p, "invalid roundingMode".into()));
                    }
                }
                "maximumSignificantDigits" => {
                    maximum_significant_digits = Some(self.number_format_option_integer(
                        p,
                        value,
                        1.0,
                        NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS,
                        "maximumSignificantDigits",
                    )? as usize);
                }
                _ => {}
            }
        }
        if style == "currency" && currency.is_none() {
            return Err(self.type_error(p, "currency is required".into()));
        }
        if style == "unit" && unit.is_none() {
            return Err(self.type_error(p, "unit is required".into()));
        }
        Ok(NumberFormatOptions {
            style,
            currency,
            numbering_system,
            minimum_fraction_digits,
            maximum_fraction_digits,
            maximum_significant_digits,
            unit,
            unit_display,
            use_grouping,
            minimum_integer_digits,
            sign_display,
            rounding_mode,
        })
    }

    fn number_format_option_integer(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        minimum: f64,
        maximum: f64,
        name: &str,
    ) -> Result<u32, JsError> {
        let number = self.to_number(p, value)?;
        let integer = number.trunc();
        if !integer.is_finite() || integer < minimum || integer > maximum {
            return Err(self.range_error(p, format!("invalid {name}").into()));
        }
        Ok(integer as u32)
    }

    pub(super) fn intl_number_format_format(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(locale) = self.hidden_string(this, NUMBER_FORMAT_LOCALE_SLOT) else {
            return Err(self.type_error(p, "incompatible NumberFormat receiver".into()));
        };
        let Some(style) = self.hidden_string(this, NUMBER_FORMAT_STYLE_SLOT) else {
            return Err(self.type_error(p, "incompatible NumberFormat receiver".into()));
        };
        let minimum_fraction_digits = self
            .hidden_value(this, NUMBER_FORMAT_MIN_FRACTION_SLOT)
            .and_then(Value::as_number)
            .unwrap_or_default() as usize;
        let maximum_fraction_digits = self
            .hidden_value(this, NUMBER_FORMAT_MAX_FRACTION_SLOT)
            .and_then(Value::as_number)
            .map(|digits| digits as usize);
        let rounding_mode = self
            .hidden_string(this, NUMBER_FORMAT_ROUNDING_MODE_SLOT)
            .unwrap_or_else(|| "halfExpand".into());
        let maximum_significant_digits = self
            .hidden_value(this, NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT)
            .and_then(Value::as_number)
            .map(|digits| digits as usize);
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let formatted = if let Some(Cell::BigInt(value)) = self.heap.get(value) {
            quench_intl::format_bigint(
                value,
                &quench_intl::BigIntFormatOptions {
                    locale: &locale,
                    style: &style,
                    minimum_fraction_digits,
                    maximum_significant_digits,
                },
            )
        } else if let Some(number) = value.as_number() {
            self.format_number_value(
                number,
                &style,
                self.hidden_string(this, NUMBER_FORMAT_UNIT_SLOT).as_deref(),
                &self
                    .hidden_string(this, NUMBER_FORMAT_UNIT_DISPLAY_SLOT)
                    .unwrap_or_else(|| "short".into()),
                self.hidden_value(this, NUMBER_FORMAT_GROUPING_SLOT)
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                self.hidden_value(this, NUMBER_FORMAT_MIN_INTEGER_SLOT)
                    .and_then(Value::as_number)
                    .unwrap_or(1.0) as usize,
                minimum_fraction_digits,
                maximum_fraction_digits,
                &rounding_mode,
            )
        } else if matches!(self.heap.get(value), Some(Cell::String(_))) {
            let number = self.to_number(p, value)?;
            self.format_number_value(
                number,
                &style,
                self.hidden_string(this, NUMBER_FORMAT_UNIT_SLOT).as_deref(),
                &self
                    .hidden_string(this, NUMBER_FORMAT_UNIT_DISPLAY_SLOT)
                    .unwrap_or_else(|| "short".into()),
                self.hidden_value(this, NUMBER_FORMAT_GROUPING_SLOT)
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                self.hidden_value(this, NUMBER_FORMAT_MIN_INTEGER_SLOT)
                    .and_then(Value::as_number)
                    .unwrap_or(1.0) as usize,
                minimum_fraction_digits,
                maximum_fraction_digits,
                &rounding_mode,
            )
        } else {
            return Err(self.type_error(p, "NumberFormat requires a Number or BigInt".into()));
        };
        let sign_display = self
            .hidden_string(this, NUMBER_FORMAT_SIGN_DISPLAY_SLOT)
            .unwrap_or_else(|| "auto".into());
        let formatted = match sign_display.as_str() {
            "never" => formatted.strip_prefix('-').unwrap_or(&formatted).to_owned(),
            "always" if !formatted.starts_with('-') && !formatted.starts_with('+') => {
                format!("+{formatted}")
            }
            _ => formatted,
        };
        Ok(self.heap.alloc(Cell::String(formatted.into())))
    }

    pub(super) fn intl_number_format_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let Some(locale) = self.hidden_string(this, NUMBER_FORMAT_LOCALE_SLOT) else {
            return Err(self.type_error(p, "incompatible NumberFormat receiver".into()));
        };
        let Some(style) = self.hidden_string(this, NUMBER_FORMAT_STYLE_SLOT) else {
            return Err(self.type_error(p, "incompatible NumberFormat receiver".into()));
        };
        let result = self.object();
        let locale_value = self.heap.alloc(Cell::String(locale.clone().into()));
        self.set_named(p, result, "locale", locale_value)?;
        let numbering_system = self
            .hidden_string(this, NUMBER_FORMAT_NUMBERING_SYSTEM_SLOT)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| quench_intl::default_numbering_system(&locale).into());
        let numbering_system_value = self.heap.alloc(Cell::String(numbering_system.into()));
        self.set_named(p, result, "numberingSystem", numbering_system_value)?;
        let style_value = self.heap.alloc(Cell::String(style.clone().into()));
        self.set_named(p, result, "style", style_value)?;
        for (name, slot, fallback) in [
            ("minimumIntegerDigits", NUMBER_FORMAT_MIN_INTEGER_SLOT, 1.0),
            (
                "minimumFractionDigits",
                NUMBER_FORMAT_MIN_FRACTION_SLOT,
                0.0,
            ),
        ] {
            let value = self
                .hidden_value(this, slot)
                .and_then(Value::as_number)
                .unwrap_or(fallback);
            self.set_named(p, result, name, Value::number(value))?;
        }
        let maximum_fraction_digits = self
            .hidden_value(this, NUMBER_FORMAT_MAX_FRACTION_SLOT)
            .and_then(Value::as_number)
            .unwrap_or(if style == "currency" { 2.0 } else { 3.0 });
        self.set_named(
            p,
            result,
            "maximumFractionDigits",
            Value::number(maximum_fraction_digits),
        )?;
        if style == "currency" {
            let currency = self
                .hidden_string(this, NUMBER_FORMAT_CURRENCY_SLOT)
                .unwrap_or_default();
            let currency = self.heap.alloc(Cell::String(currency.into()));
            self.set_named(p, result, "currency", currency)?;
            let display = self.heap.alloc(Cell::String("symbol".into()));
            self.set_named(p, result, "currencyDisplay", display)?;
            let sign = self.heap.alloc(Cell::String("standard".into()));
            self.set_named(p, result, "currencySign", sign)?;
        }
        if style == "unit" {
            let unit = self
                .hidden_string(this, NUMBER_FORMAT_UNIT_SLOT)
                .unwrap_or_default();
            let unit = self.heap.alloc(Cell::String(unit.into()));
            self.set_named(p, result, "unit", unit)?;
            let unit_display = self
                .hidden_string(this, NUMBER_FORMAT_UNIT_DISPLAY_SLOT)
                .unwrap_or_else(|| "short".into());
            let unit_display = self.heap.alloc(Cell::String(unit_display.into()));
            self.set_named(p, result, "unitDisplay", unit_display)?;
        }
        Ok(result)
    }

    pub(super) fn intl_number_format_format_to_parts(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let formatted = self.intl_number_format_format(p, this, args)?;
        let Some(Cell::String(formatted)) = self.heap.get(formatted) else {
            return Err(JsError("NumberFormat output is not a string".into()));
        };
        let text = formatted.to_string();
        let mut parts = Vec::new();
        let unit = self
            .hidden_string(this, NUMBER_FORMAT_UNIT_SLOT)
            .unwrap_or_default();
        let style = self
            .hidden_string(this, NUMBER_FORMAT_STYLE_SLOT)
            .unwrap_or_default();
        let unit_suffix = if style == "unit" {
            let label = format_number_unit(
                &unit,
                &self
                    .hidden_string(this, NUMBER_FORMAT_UNIT_DISPLAY_SLOT)
                    .unwrap_or_else(|| "short".into()),
                2.0,
            );
            Some(if label.is_empty() {
                label
            } else {
                format!(" {label}")
            })
        } else if style == "percent" {
            Some("%".into())
        } else {
            None
        };
        let numeric_text = unit_suffix
            .as_deref()
            .and_then(|suffix| text.strip_suffix(suffix))
            .unwrap_or(&text);
        let (integer, fraction) = numeric_text.split_once('.').unwrap_or((numeric_text, ""));
        if integer.starts_with('-') {
            parts.push(self.number_format_part("minusSign", "-", None)?);
        }
        let integer = integer.trim_start_matches('-');
        let mut start = 0;
        for (index, character) in integer.char_indices() {
            if character == ',' {
                if start < index {
                    parts.push(self.number_format_part("integer", &integer[start..index], None)?);
                }
                parts.push(self.number_format_part("group", ",", None)?);
                start = index + character.len_utf8();
            }
        }
        if start < integer.len() || integer.is_empty() {
            parts.push(self.number_format_part("integer", &integer[start..], None)?);
        }
        if !fraction.is_empty() {
            parts.push(self.number_format_part("decimal", ".", None)?);
            parts.push(self.number_format_part("fraction", fraction, None)?);
        }
        if let Some(suffix) = unit_suffix {
            if style == "percent" {
                parts.push(self.number_format_part("percentSign", &suffix, None)?);
            } else {
                parts.push(self.number_format_part("literal", " ", None)?);
                parts.push(self.number_format_part("unit", suffix.trim(), Some(&unit))?);
            }
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(parts),
        }))
    }

    fn number_format_part(
        &mut self,
        kind: &str,
        value: &str,
        unit: Option<&str>,
    ) -> Result<Value, JsError> {
        let object = self.object();
        self.set_intl_string_property(object, "type", kind)?;
        self.set_intl_string_property(object, "value", value)?;
        if let Some(unit) = unit {
            self.set_intl_string_property(object, "unit", unit)?;
        }
        Ok(object)
    }

    pub(super) fn set_intl_string_property(
        &mut self,
        object: Value,
        key: &str,
        value: &str,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(key);
        let value = self.heap.alloc(Cell::String(value.into()));
        self.set_property(object, atom, value)
    }

    fn format_number_value(
        &self,
        value: f64,
        style: &str,
        unit: Option<&str>,
        unit_display: &str,
        use_grouping: bool,
        minimum_integer_digits: usize,
        minimum_fraction_digits: usize,
        maximum_fraction_digits: Option<usize>,
        rounding_mode: &str,
    ) -> String {
        let number = if style == "percent" {
            value * 100.0
        } else {
            value
        };
        let negative = number.is_sign_negative();
        let absolute = number.abs();
        let mut text = if absolute.fract() == 0.0 {
            format!("{absolute:.0}")
        } else {
            absolute.to_string()
        };
        let (integer, raw_fraction) = text.split_once('.').unwrap_or((&text, ""));
        let fraction = match maximum_fraction_digits {
            Some(maximum) if rounding_mode == "trunc" => {
                &raw_fraction[..raw_fraction.len().min(maximum)]
            }
            _ => raw_fraction,
        };
        let integer = format!("{:0>width$}", integer, width = minimum_integer_digits);
        let integer = if use_grouping {
            group_decimal_integer(&integer)
        } else {
            integer
        };
        text = if fraction.is_empty() && minimum_fraction_digits == 0 {
            integer
        } else {
            format!("{integer}.{fraction:0<minimum_fraction_digits$}")
        };
        if negative {
            text.insert(0, '-');
        }
        if style == "percent" {
            text.push('%');
        } else if style == "unit" {
            let label = format_number_unit(unit.unwrap_or("unit"), unit_display, absolute);
            if unit_display == "narrow" {
                text.push_str(&label);
            } else {
                text.push(' ');
                text.push_str(&label);
            }
        }
        quench_intl::localize_digits(text, "latn")
    }

    pub(super) fn set_hidden_string(
        &mut self,
        object: Value,
        key: &str,
        value: &str,
    ) -> Result<(), JsError> {
        let value = self.heap.alloc(Cell::String(value.into()));
        self.set_hidden_value(object, key, value)
    }

    pub(super) fn set_hidden_value(
        &mut self,
        object: Value,
        key: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(key);
        self.set_property(object, atom, value)
    }

    pub(super) fn hidden_value(&self, object: Value, key: &str) -> Option<Value> {
        self.lookup_atom(key)
            .and_then(|atom| self.own_property(object, atom))
    }

    pub(super) fn hidden_string(&self, object: Value, key: &str) -> Option<String> {
        let value = self.hidden_value(object, key)?;
        match self.heap.get(value) {
            Some(Cell::String(value)) => Some(value.to_string()),
            _ => None,
        }
    }
}

pub(super) fn valid_locale_identifier(locale: &str) -> bool {
    quench_intl::canonical_locale_identifier(locale).is_some()
}

pub(super) fn is_supported_locale(locale: &str) -> bool {
    !locale
        .split('-')
        .next()
        .is_some_and(|language| language.eq_ignore_ascii_case("zxx"))
}

pub(super) fn group_decimal_integer(value: &str) -> String {
    let mut output = String::with_capacity(value.len() + value.len() / 3);
    for (index, character) in value.chars().enumerate() {
        if index > 0 && (value.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(character);
    }
    output
}

fn format_number_unit(unit: &str, display: &str, value: f64) -> String {
    let singular = value.abs() == 1.0;
    let (long, short, narrow) = match unit {
        "year" => (if singular { "year" } else { "years" }, "yr", "y"),
        "month" => (if singular { "month" } else { "months" }, "mo", "m"),
        "week" => (if singular { "week" } else { "weeks" }, "wk", "w"),
        "day" => (
            if singular { "day" } else { "days" },
            if singular { "day" } else { "days" },
            "d",
        ),
        "hour" => (if singular { "hour" } else { "hours" }, "hr", "h"),
        "minute" => (if singular { "minute" } else { "minutes" }, "min", "m"),
        "second" => (if singular { "second" } else { "seconds" }, "sec", "s"),
        "millisecond" => (
            if singular {
                "millisecond"
            } else {
                "milliseconds"
            },
            "ms",
            "ms",
        ),
        "microsecond" => (
            if singular {
                "microsecond"
            } else {
                "microseconds"
            },
            "μs",
            "μs",
        ),
        "nanosecond" => (
            if singular {
                "nanosecond"
            } else {
                "nanoseconds"
            },
            "ns",
            "ns",
        ),
        _ => (unit, unit, unit),
    };
    match display {
        "long" => long.into(),
        "narrow" => narrow.into(),
        _ => short.into(),
    }
}
