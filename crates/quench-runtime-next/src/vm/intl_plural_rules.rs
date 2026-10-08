use super::*;

const DEFAULT_PLURAL_RULES_LOCALE: &str = "en-US";
const PLURAL_RULES_LOCALE_SLOT: &str = "\0quench:intl-plural-rules-locale";
const PLURAL_RULES_TYPE_SLOT: &str = "\0quench:intl-plural-rules-type";
const PLURAL_RULES_NOTATION_SLOT: &str = "\0quench:intl-plural-rules-notation";
const PLURAL_RULES_COMPACT_DISPLAY_SLOT: &str = "\0quench:intl-plural-rules-compact-display";
const PLURAL_RULES_MIN_INTEGER_SLOT: &str = "\0quench:intl-plural-rules-minimum-integer";
const PLURAL_RULES_MIN_FRACTION_SLOT: &str = "\0quench:intl-plural-rules-minimum-fraction";
const PLURAL_RULES_MAX_FRACTION_SLOT: &str = "\0quench:intl-plural-rules-maximum-fraction";
const PLURAL_RULES_MIN_SIGNIFICANT_SLOT: &str = "\0quench:intl-plural-rules-minimum-significant";
const PLURAL_RULES_MAX_SIGNIFICANT_SLOT: &str = "\0quench:intl-plural-rules-maximum-significant";
const PLURAL_RULES_ROUNDING_INCREMENT_SLOT: &str = "\0quench:intl-plural-rules-rounding-increment";
const PLURAL_RULES_ROUNDING_MODE_SLOT: &str = "\0quench:intl-plural-rules-rounding-mode";
const PLURAL_RULES_ROUNDING_PRIORITY_SLOT: &str = "\0quench:intl-plural-rules-rounding-priority";
const PLURAL_RULES_TRAILING_ZERO_SLOT: &str = "\0quench:intl-plural-rules-trailing-zero";
const DEFAULT_MINIMUM_INTEGER_DIGITS: usize = 1;
const MAXIMUM_INTEGER_DIGITS: usize = 21;
const DEFAULT_MINIMUM_FRACTION_DIGITS: usize = 0;
const DEFAULT_MAXIMUM_FRACTION_DIGITS: usize = 3;
const MAXIMUM_FRACTION_DIGITS: usize = 100;
const MINIMUM_SIGNIFICANT_DIGITS: usize = 1;
const MAXIMUM_SIGNIFICANT_DIGITS: usize = 21;
const DEFAULT_ROUNDING_INCREMENT: usize = 1;
const MAXIMUM_ROUNDING_INCREMENT: usize = 5000;
const DEFAULT_COMPACT_MAXIMUM_SIGNIFICANT_DIGITS: usize = 3;
const COMPACT_EXPONENT_THRESHOLD: f64 = 1000.0;
const COMPACT_EXPONENT_STEP: u16 = 3;

struct PluralRulesOptions {
    rule_type: String,
    notation: String,
    compact_display: String,
    minimum_integer_digits: usize,
    minimum_fraction_digits: usize,
    maximum_fraction_digits: usize,
    minimum_significant_digits: Option<usize>,
    maximum_significant_digits: Option<usize>,
    rounding_increment: usize,
    rounding_mode: String,
    rounding_priority: String,
    trailing_zero_display: String,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_plural_rules_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlPluralRules, global, global);
        self.set_builtin_function_name(constructor, "PluralRules")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm
            .intrinsics
            .intl_plural_rules_prototypes
            .insert(global, prototype);
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.set_non_writable_property(constructor, "prototype");
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.PluralRules")?;
        for (name, native) in [
            ("select", Native::IntlPluralRulesSelect),
            ("selectRange", Native::IntlPluralRulesSelectRange),
            ("resolvedOptions", Native::IntlPluralRulesResolvedOptions),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        let supported =
            self.native_with_realm(Native::IntlPluralRulesSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.set_builtin_value_named(intl, "PluralRules", constructor)
    }

    pub(super) fn intl_plural_rules_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([new_target]), |vm| {
            let prototype = vm.intl_instance_prototype(p, new_target, Native::IntlPluralRules)?;
            vm.with_call_roots([prototype], |vm| {
                let locale = vm
                    .canonical_locale_list(p, args.first().copied())?
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| DEFAULT_PLURAL_RULES_LOCALE.into());
                let options = vm.plural_rules_options(p, args.get(1).copied())?;

                let instance = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
                vm.set_hidden_string(instance, PLURAL_RULES_LOCALE_SLOT, &locale)?;
                vm.set_hidden_string(instance, PLURAL_RULES_TYPE_SLOT, &options.rule_type)?;
                vm.set_hidden_string(instance, PLURAL_RULES_NOTATION_SLOT, &options.notation)?;
                vm.set_hidden_string(
                    instance,
                    PLURAL_RULES_COMPACT_DISPLAY_SLOT,
                    &options.compact_display,
                )?;
                for (slot, value) in [
                    (
                        PLURAL_RULES_MIN_INTEGER_SLOT,
                        options.minimum_integer_digits,
                    ),
                    (
                        PLURAL_RULES_MIN_FRACTION_SLOT,
                        options.minimum_fraction_digits,
                    ),
                    (
                        PLURAL_RULES_MAX_FRACTION_SLOT,
                        options.maximum_fraction_digits,
                    ),
                    (
                        PLURAL_RULES_ROUNDING_INCREMENT_SLOT,
                        options.rounding_increment,
                    ),
                ] {
                    vm.set_hidden_value(instance, slot, Value::number(value as f64))?;
                }
                for (slot, value) in [
                    (
                        PLURAL_RULES_MIN_SIGNIFICANT_SLOT,
                        options.minimum_significant_digits,
                    ),
                    (
                        PLURAL_RULES_MAX_SIGNIFICANT_SLOT,
                        options.maximum_significant_digits,
                    ),
                ] {
                    if let Some(value) = value {
                        vm.set_hidden_value(instance, slot, Value::number(value as f64))?;
                    }
                }
                for (slot, value) in [
                    (PLURAL_RULES_ROUNDING_MODE_SLOT, options.rounding_mode),
                    (
                        PLURAL_RULES_ROUNDING_PRIORITY_SLOT,
                        options.rounding_priority,
                    ),
                    (
                        PLURAL_RULES_TRAILING_ZERO_SLOT,
                        options.trailing_zero_display,
                    ),
                ] {
                    vm.set_hidden_string(instance, slot, &value)?;
                }
                Ok(instance)
            })
        })
    }

    fn plural_rules_options(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<PluralRulesOptions, JsError> {
        let options = match options.filter(|value| !value.is_undefined()) {
            None => self
                .heap
                .alloc(Cell::Object(Self::empty_object(Value::NULL))),
            Some(value) if value.is_null() => {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            Some(value) => self.box_object(value)?,
        };
        self.with_call_roots([options], |vm| {
            let locale_matcher =
                vm.plural_option_string(p, options, "localeMatcher", "best fit")?;
            validate_plural_option(p, &locale_matcher, &["lookup", "best fit"], "localeMatcher")?;
            let rule_type = vm.plural_option_string(p, options, "type", "cardinal")?;
            validate_plural_option(p, &rule_type, &["cardinal", "ordinal"], "type")?;
            let notation = vm.plural_option_string(p, options, "notation", "standard")?;
            validate_plural_option(
                p,
                &notation,
                &["standard", "compact", "scientific", "engineering"],
                "notation",
            )?;
            let compact_display = vm.plural_option_string(p, options, "compactDisplay", "short")?;
            validate_plural_option(p, &compact_display, &["short", "long"], "compactDisplay")?;

            let minimum_integer_digits = vm.plural_option_integer(
                p,
                options,
                "minimumIntegerDigits",
                DEFAULT_MINIMUM_INTEGER_DIGITS,
                DEFAULT_MINIMUM_INTEGER_DIGITS,
                MAXIMUM_INTEGER_DIGITS,
            )?;
            let minimum_fraction = vm.plural_option_optional_integer(
                p,
                options,
                "minimumFractionDigits",
                DEFAULT_MINIMUM_FRACTION_DIGITS,
                MAXIMUM_FRACTION_DIGITS,
            )?;
            let maximum_fraction = vm.plural_option_optional_integer(
                p,
                options,
                "maximumFractionDigits",
                DEFAULT_MINIMUM_FRACTION_DIGITS,
                MAXIMUM_FRACTION_DIGITS,
            )?;
            let minimum_significant = vm.plural_option_optional_integer(
                p,
                options,
                "minimumSignificantDigits",
                MINIMUM_SIGNIFICANT_DIGITS,
                MAXIMUM_SIGNIFICANT_DIGITS,
            )?;
            let maximum_significant = vm.plural_option_optional_integer(
                p,
                options,
                "maximumSignificantDigits",
                MINIMUM_SIGNIFICANT_DIGITS,
                MAXIMUM_SIGNIFICANT_DIGITS,
            )?;
            let rounding_increment = vm
                .plural_option_optional_integer(
                    p,
                    options,
                    "roundingIncrement",
                    DEFAULT_ROUNDING_INCREMENT,
                    MAXIMUM_ROUNDING_INCREMENT,
                )?
                .unwrap_or(DEFAULT_ROUNDING_INCREMENT);
            let rounding_mode =
                vm.plural_option_string(p, options, "roundingMode", "halfExpand")?;
            validate_plural_option(
                p,
                &rounding_mode,
                &[
                    "ceil",
                    "floor",
                    "expand",
                    "trunc",
                    "halfCeil",
                    "halfFloor",
                    "halfExpand",
                    "halfTrunc",
                    "halfEven",
                ],
                "roundingMode",
            )?;
            let rounding_priority =
                vm.plural_option_string(p, options, "roundingPriority", "auto")?;
            validate_plural_option(
                p,
                &rounding_priority,
                &["auto", "morePrecision", "lessPrecision"],
                "roundingPriority",
            )?;
            let trailing_zero_display =
                vm.plural_option_string(p, options, "trailingZeroDisplay", "auto")?;
            validate_plural_option(
                p,
                &trailing_zero_display,
                &["auto", "stripIfInteger"],
                "trailingZeroDisplay",
            )?;

            let minimum_fraction = minimum_fraction.unwrap_or(DEFAULT_MINIMUM_FRACTION_DIGITS);
            let maximum_fraction =
                maximum_fraction.unwrap_or(DEFAULT_MAXIMUM_FRACTION_DIGITS.max(minimum_fraction));
            if maximum_fraction < minimum_fraction {
                return Err(vm.range_error(
                    p,
                    "maximumFractionDigits is less than minimumFractionDigits".into(),
                ));
            }
            if minimum_significant
                .zip(maximum_significant)
                .is_some_and(|(min, max)| max < min)
            {
                return Err(vm.range_error(
                    p,
                    "maximumSignificantDigits is less than minimumSignificantDigits".into(),
                ));
            }
            let (minimum_significant, maximum_significant) =
                if minimum_significant.is_some() || maximum_significant.is_some() {
                    (
                        Some(minimum_significant.unwrap_or(MINIMUM_SIGNIFICANT_DIGITS)),
                        Some(maximum_significant.unwrap_or(MAXIMUM_SIGNIFICANT_DIGITS)),
                    )
                } else {
                    (None, None)
                };
            const VALID_ROUNDING_INCREMENTS: &[usize] = &[
                1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000,
            ];
            if !VALID_ROUNDING_INCREMENTS.contains(&rounding_increment) {
                return Err(vm.range_error(p, "invalid roundingIncrement".into()));
            }
            let compact_display = if notation == "compact" {
                compact_display
            } else {
                String::new()
            };
            let _ = locale_matcher;
            Ok(PluralRulesOptions {
                rule_type,
                notation,
                compact_display,
                minimum_integer_digits,
                minimum_fraction_digits: minimum_fraction,
                maximum_fraction_digits: maximum_fraction,
                minimum_significant_digits: minimum_significant,
                maximum_significant_digits: maximum_significant,
                rounding_increment,
                rounding_mode,
                rounding_priority,
                trailing_zero_display,
            })
        })
    }

    fn plural_option_string(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        key: &str,
        default: &str,
    ) -> Result<String, JsError> {
        let atom = self.intern_atom(key);
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            Ok(default.into())
        } else {
            self.to_string(p, value)
        }
    }

    fn plural_option_integer(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        key: &str,
        default: usize,
        minimum: usize,
        maximum: usize,
    ) -> Result<usize, JsError> {
        Ok(self
            .plural_option_optional_integer(p, options, key, minimum, maximum)?
            .unwrap_or(default))
    }

    fn plural_option_optional_integer(
        &mut self,
        p: &ResidualProgram,
        options: Value,
        key: &str,
        minimum: usize,
        maximum: usize,
    ) -> Result<Option<usize>, JsError> {
        let atom = self.intern_atom(key);
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(None);
        }
        let number = self.to_number(p, value)?;
        let number = if number.is_nan() { 0.0 } else { number.trunc() };
        if !number.is_finite() || number < minimum as f64 || number > maximum as f64 {
            return Err(self.range_error(p, format!("{key} is outside its permitted range")));
        }
        Ok(Some(number as usize))
    }

    pub(super) fn intl_plural_rules_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlPluralRulesSupportedLocalesOf => {
                self.intl_supported_locales_of(p, args, super::intl_number::is_supported_locale)
            }
            Native::IntlPluralRulesSelect => self.plural_rules_select(p, this, args),
            Native::IntlPluralRulesSelectRange => self.plural_rules_select_range(p, this, args),
            Native::IntlPluralRulesResolvedOptions => self.plural_rules_resolved_options(p, this),
            _ => Err(JsError("invalid Intl.PluralRules operation".into())),
        }
    }

    fn plural_rules_select(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (locale, rule_type) = self.plural_rules_receiver(p, this)?;
        let input = args.first().copied().unwrap_or(Value::UNDEFINED);
        let number = self.to_number(p, input)?;
        let number = self.round_plural_rules_number(this, number);
        let notation = self
            .hidden_string(this, PLURAL_RULES_NOTATION_SLOT)
            .unwrap_or_else(|| "standard".into());
        let category = if notation == "compact" {
            let (significand, exponent) = compact_plural_operand(&number);
            if exponent == 0 {
                quench_intl::plural_category_decimal(&locale, &rule_type, &significand)
            } else {
                quench_intl::plural_category_compact(&locale, &rule_type, &significand, exponent)
            }
        } else {
            quench_intl::plural_category_decimal(&locale, &rule_type, &number)
        }
        .unwrap_or("other");
        Ok(self.heap.alloc(Cell::String(category.into())))
    }

    fn plural_rules_select_range(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (locale, rule_type) = self.plural_rules_receiver(p, this)?;
        let start_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let end_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if start_value.is_undefined() || end_value.is_undefined() {
            return Err(self.type_error(p, "selectRange requires two arguments".into()));
        }
        let start = self.to_number(p, start_value)?;
        let end = self.to_number(p, end_value)?;
        if start.is_nan() || end.is_nan() {
            return Err(self.range_error(p, "selectRange arguments must not be NaN".into()));
        }
        let start = self.round_plural_rules_number(this, start);
        let end = self.round_plural_rules_number(this, end);
        let category =
            quench_intl::plural_category_range_decimal(&locale, &rule_type, &start, &end)
                .unwrap_or("other");
        Ok(self.heap.alloc(Cell::String(category.into())))
    }

    fn round_plural_rules_number(&self, this: Value, number: f64) -> String {
        if !number.is_finite() {
            return number.to_string();
        }
        let notation = self
            .hidden_string(this, PLURAL_RULES_NOTATION_SLOT)
            .unwrap_or_else(|| "standard".into());
        let maximum_significant = self
            .hidden_value(this, PLURAL_RULES_MAX_SIGNIFICANT_SLOT)
            .and_then(Value::as_number)
            .map(|value| value as usize)
            .or_else(|| {
                (notation == "compact").then_some(DEFAULT_COMPACT_MAXIMUM_SIGNIFICANT_DIGITS)
            });
        if let Some(maximum_significant) = maximum_significant {
            let exponent = if number == 0.0 {
                0
            } else {
                number.abs().log10().floor() as i32
            };
            let precision = maximum_significant as i32 - exponent - 1;
            let mode = self
                .hidden_string(this, PLURAL_RULES_ROUNDING_MODE_SLOT)
                .unwrap_or_else(|| "halfExpand".into());
            let rounded = super::intl_number::round_number_at_precision(
                number,
                precision,
                1,
                &mode,
                number.is_sign_negative(),
            );
            let minimum_significant = self
                .hidden_value(this, PLURAL_RULES_MIN_SIGNIFICANT_SLOT)
                .and_then(Value::as_number)
                .map_or(MINIMUM_SIGNIFICANT_DIGITS, |value| value as usize);
            return if minimum_significant > 1 {
                let visible_fraction_digits = (minimum_significant as i32 - exponent - 1)
                    .max(0)
                    .min(MAXIMUM_FRACTION_DIGITS as i32)
                    as usize;
                let visible_fraction_digits = if self
                    .hidden_string(this, PLURAL_RULES_TRAILING_ZERO_SLOT)
                    .as_deref()
                    == Some("stripIfInteger")
                    && rounded.fract() == 0.0
                {
                    0
                } else {
                    visible_fraction_digits
                };
                format!("{rounded:.visible_fraction_digits$}")
            } else {
                rounded.to_string()
            };
        }
        let max_fraction = self.plural_rules_slot_number(
            this,
            PLURAL_RULES_MAX_FRACTION_SLOT,
            DEFAULT_MAXIMUM_FRACTION_DIGITS,
        );
        let increment =
            self.plural_rules_slot_number(this, PLURAL_RULES_ROUNDING_INCREMENT_SLOT, 1);
        let mode = self
            .hidden_string(this, PLURAL_RULES_ROUNDING_MODE_SLOT)
            .unwrap_or_else(|| "halfExpand".into());
        let rounded = super::intl_number::round_number_at_precision(
            number,
            max_fraction as i32,
            increment,
            &mode,
            number.is_sign_negative(),
        );
        let min_fraction = self.plural_rules_slot_number(
            this,
            PLURAL_RULES_MIN_FRACTION_SLOT,
            DEFAULT_MINIMUM_FRACTION_DIGITS,
        );
        format!("{:.*}", min_fraction.min(max_fraction), rounded)
    }

    fn plural_rules_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let (locale, rule_type) = self.plural_rules_receiver(p, this)?;
        let notation = self
            .hidden_string(this, PLURAL_RULES_NOTATION_SLOT)
            .unwrap_or_else(|| "standard".into());
        let minimum_fraction =
            self.plural_rules_slot_number(this, PLURAL_RULES_MIN_FRACTION_SLOT, 0);
        let maximum_fraction = self.plural_rules_slot_number(
            this,
            PLURAL_RULES_MAX_FRACTION_SLOT,
            DEFAULT_MAXIMUM_FRACTION_DIGITS,
        );
        let result = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        for (key, value) in [
            ("locale", locale.clone()),
            ("type", rule_type.clone()),
            ("notation", notation.clone()),
        ] {
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_plural_result_property(result, key, value)?;
        }
        for (key, value) in [
            (
                "minimumIntegerDigits",
                self.plural_rules_slot_number(this, PLURAL_RULES_MIN_INTEGER_SLOT, 1),
            ),
            ("minimumFractionDigits", minimum_fraction),
            ("maximumFractionDigits", maximum_fraction),
        ] {
            self.set_plural_result_property(result, key, Value::number(value as f64))?;
        }
        for (key, slot) in [
            (
                "minimumSignificantDigits",
                PLURAL_RULES_MIN_SIGNIFICANT_SLOT,
            ),
            (
                "maximumSignificantDigits",
                PLURAL_RULES_MAX_SIGNIFICANT_SLOT,
            ),
        ] {
            if let Some(value) = self.hidden_value(this, slot) {
                self.set_plural_result_property(result, key, value)?;
            }
        }
        let categories = quench_intl::plural_categories(&locale, &rule_type)
            .unwrap_or_else(|| vec!["other"])
            .into_iter()
            .map(|category| self.heap.alloc(Cell::String(category.into())))
            .collect::<Vec<_>>();
        let categories = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(categories),
        });
        self.set_plural_result_property(result, "pluralCategories", categories)?;
        self.set_plural_result_property(
            result,
            "roundingIncrement",
            Value::number(self.plural_rules_slot_number(
                this,
                PLURAL_RULES_ROUNDING_INCREMENT_SLOT,
                1,
            ) as f64),
        )?;
        for (key, slot, default) in [
            (
                "roundingMode",
                PLURAL_RULES_ROUNDING_MODE_SLOT,
                "halfExpand",
            ),
            (
                "roundingPriority",
                PLURAL_RULES_ROUNDING_PRIORITY_SLOT,
                "auto",
            ),
            (
                "trailingZeroDisplay",
                PLURAL_RULES_TRAILING_ZERO_SLOT,
                "auto",
            ),
        ] {
            let value = self
                .hidden_string(this, slot)
                .unwrap_or_else(|| default.into());
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_plural_result_property(result, key, value)?;
        }
        if notation == "compact" {
            let display = self
                .hidden_string(this, PLURAL_RULES_COMPACT_DISPLAY_SLOT)
                .unwrap_or_else(|| "short".into());
            let display = self.heap.alloc(Cell::String(display.into()));
            self.set_plural_result_property(result, "compactDisplay", display)?;
        }
        Ok(result)
    }

    fn plural_rules_receiver(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<(String, String), JsError> {
        let locale = self
            .hidden_string(this, PLURAL_RULES_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible Intl.PluralRules receiver".into()))?;
        let rule_type = self
            .hidden_string(this, PLURAL_RULES_TYPE_SLOT)
            .unwrap_or_else(|| "cardinal".into());
        Ok((locale, rule_type))
    }

    fn plural_rules_slot_number(&self, this: Value, slot: &str, default: usize) -> usize {
        self.hidden_value(this, slot)
            .and_then(|value| value.as_number())
            .filter(|value| value.is_finite() && *value >= 0.0)
            .map(|value| value as usize)
            .unwrap_or(default)
    }

    fn set_plural_result_property(
        &mut self,
        result: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property(result, atom, value)
    }
}

fn validate_plural_option(
    _p: &ResidualProgram,
    value: &str,
    allowed: &[&str],
    name: &str,
) -> Result<(), JsError> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(JsError(format!("RangeError: invalid {name}").into()))
    }
}

fn compact_plural_operand(decimal: &str) -> (String, u16) {
    let Ok(number) = decimal.parse::<f64>() else {
        return (decimal.into(), 0);
    };
    if !number.is_finite() || number.abs() < COMPACT_EXPONENT_THRESHOLD {
        return (decimal.into(), 0);
    }
    let exponent =
        (number.abs().log10().floor() as u16 / COMPACT_EXPONENT_STEP) * COMPACT_EXPONENT_STEP;
    let visible_fraction_digits = decimal
        .split_once('.')
        .map_or(0, |(_, fraction)| fraction.len());
    let significand = number / 10_f64.powi(exponent as i32);
    (format!("{significand:.visible_fraction_digits$}"), exponent)
}
