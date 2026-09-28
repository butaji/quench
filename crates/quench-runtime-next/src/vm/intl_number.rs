use super::*;

const DEFAULT_NUMBER_FORMAT_LOCALE: &str = "en-US";
const NUMBER_FORMAT_MAX_FRACTION_DIGITS: f64 = 100.0;
const NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS: f64 = 21.0;
const NUMBER_FORMAT_LOCALE_LANGUAGE_MIN_LENGTH: usize = 2;
const NUMBER_FORMAT_LOCALE_LANGUAGE_MAX_LENGTH: usize = 8;
const NUMBER_FORMAT_LOCALE_SUBTAG_MAX_LENGTH: usize = 8;
const NUMBER_FORMAT_LOCALE_SLOT: &str = "\0rqj:intl-number-format-locale";
const NUMBER_FORMAT_STYLE_SLOT: &str = "\0rqj:intl-number-format-style";
const NUMBER_FORMAT_MIN_FRACTION_SLOT: &str = "\0rqj:intl-number-format-min-fraction";
const NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT: &str = "\0rqj:intl-number-format-max-significant";

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_number_format_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let intl = self.heap.alloc(Cell::Object(Self::empty_object(object_prototype)));
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
        let format = self.native_with_realm(Native::IntlNumberFormatFormat, global, global);
        self.set_builtin_function_name(format, "format")?;
        self.set_builtin_value_named(prototype, "format", format)?;
        self.set_builtin_value_named(intl, "NumberFormat", constructor)?;
        self.install_intl_collator_for_realm(program, intl, global, object_prototype)?;
        self.install_intl_date_time_format_for_realm(intl, global, object_prototype)?;
        self.install_intl_display_names_for_realm(program, intl, global, object_prototype)?;
        let supported_values = self.native_with_realm(Native::IntlSupportedValuesOf, global, global);
        self.set_builtin_function_name(supported_values, "supportedValuesOf")?;
        self.set_builtin_value_named(intl, "supportedValuesOf", supported_values)?;
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
        let (style, minimum_fraction_digits, maximum_significant_digits) =
            self.number_format_options(p, args.get(1).copied())?;
        let prototype = self.number_format_instance_prototype(p, new_target)?;
        let formatter = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(formatter, NUMBER_FORMAT_LOCALE_SLOT, &locale)?;
        self.set_hidden_string(formatter, NUMBER_FORMAT_STYLE_SLOT, &style)?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_MIN_FRACTION_SLOT,
            Value::number(minimum_fraction_digits as f64),
        )?;
        self.set_hidden_value(
            formatter,
            NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT,
            maximum_significant_digits
                .map_or(Value::UNDEFINED, |digits| Value::number(digits as f64)),
        )?;
        Ok(formatter)
    }

    pub(super) fn intl_supported_values_of(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let key = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        if key != "timeZone" {
            return Err(self.range_error(p, "invalid key".into()));
        }
        let mut values = chrono_tz::TZ_VARIANTS
            .iter()
            .map(|timezone| timezone.name().to_owned())
            .collect::<Vec<_>>();
        values.sort_unstable();
        values.dedup();
        let values = values
            .into_iter()
            .map(|timezone| self.heap.alloc(Cell::String(timezone.into())))
            .collect::<Vec<_>>();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
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
    ) -> Result<(String, usize, Option<usize>), JsError> {
        let Some(options) = options.filter(|value| !value.is_undefined()) else {
            return Ok(("decimal".into(), 0, None));
        };
        if options.is_null() {
            return Err(self.type_error(p, "options must not be null".into()));
        }
        let options = self.box_object(options)?;
        let mut style = "decimal".to_owned();
        let mut currency = None;
        let mut minimum_fraction_digits = 0;
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
                "minimumFractionDigits" => {
                    minimum_fraction_digits = self.number_format_option_integer(
                        p,
                        value,
                        0.0,
                        NUMBER_FORMAT_MAX_FRACTION_DIGITS,
                        "minimumFractionDigits",
                    )? as usize;
                }
                "maximumSignificantDigits" => {
                    maximum_significant_digits = Some(
                        self.number_format_option_integer(
                            p,
                            value,
                            1.0,
                            NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS,
                            "maximumSignificantDigits",
                        )? as usize,
                    );
                }
                _ => {}
            }
        }
        if style == "currency" && currency.is_none() {
            return Err(self.type_error(p, "currency is required".into()));
        }
        Ok((style, minimum_fraction_digits, maximum_significant_digits))
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
        let maximum_significant_digits = self
            .hidden_value(this, NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT)
            .and_then(Value::as_number)
            .map(|digits| digits as usize);
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(Cell::BigInt(value)) = self.heap.get(value) else {
            return Err(self.type_error(p, "NumberFormat currently requires a BigInt".into()));
        };
        let formatted = quench_intl::format_bigint(
            value,
            &quench_intl::BigIntFormatOptions {
                locale: &locale,
                style: &style,
                minimum_fraction_digits,
                maximum_significant_digits,
            },
        );
        Ok(self.heap.alloc(Cell::String(formatted.into())))
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

    fn set_hidden_value(&mut self, object: Value, key: &str, value: Value) -> Result<(), JsError> {
        let atom = self.intern_atom(key);
        self.set_property(object, atom, value)
    }

    fn hidden_value(&self, object: Value, key: &str) -> Option<Value> {
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
    let mut subtags = locale.split('-');
    let Some(language) = subtags.next() else {
        return false;
    };
    (NUMBER_FORMAT_LOCALE_LANGUAGE_MIN_LENGTH..=NUMBER_FORMAT_LOCALE_LANGUAGE_MAX_LENGTH)
        .contains(&language.len())
        && language.bytes().all(|byte| byte.is_ascii_alphabetic())
        && subtags.all(|subtag| {
            !subtag.is_empty()
                && subtag.len() <= NUMBER_FORMAT_LOCALE_SUBTAG_MAX_LENGTH
                && subtag.bytes().all(|byte| byte.is_ascii_alphanumeric())
        })
}

pub(super) fn is_supported_locale(locale: &str) -> bool {
    locale
        .split('-')
        .next()
        .is_some_and(|language| language.eq_ignore_ascii_case("en"))
}
