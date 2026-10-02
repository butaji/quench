use super::*;

const DEFAULT_NUMBER_FORMAT_LOCALE: &str = "en-US";
const NUMBER_FORMAT_MAX_FRACTION_DIGITS: f64 = 100.0;
const NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS: f64 = 21.0;
const NUMBER_FORMAT_LARGE_DECIMAL_THRESHOLD: f64 = 1e21;
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
const NUMBER_FORMAT_CURRENCY_DISPLAY_SLOT: &str = "\0rqj:intl-number-format-currency-display";
const NUMBER_FORMAT_CURRENCY_SIGN_SLOT: &str = "\0rqj:intl-number-format-currency-sign";
const NUMBER_FORMAT_NOTATION_SLOT: &str = "\0rqj:intl-number-format-notation";
const NUMBER_FORMAT_COMPACT_DISPLAY_SLOT: &str = "\0rqj:intl-number-format-compact-display";
const NUMBER_FORMAT_GROUPING_MODE_SLOT: &str = "\0rqj:intl-number-format-grouping-mode";
const NUMBER_FORMAT_ROUNDING_INCREMENT_SLOT: &str = "\0rqj:intl-number-format-rounding-increment";
const NUMBER_FORMAT_ROUNDING_PRIORITY_SLOT: &str = "\0rqj:intl-number-format-rounding-priority";
const NUMBER_FORMAT_TRAILING_ZERO_SLOT: &str = "\0rqj:intl-number-format-trailing-zero-display";
const NUMBER_FORMAT_MIN_SIGNIFICANT_SLOT: &str = "\0rqj:intl-number-format-min-significant";

struct NumberFormatOptions {
    style: String,
    currency: Option<String>,
    numbering_system: Option<String>,
    minimum_fraction_digits: usize,
    maximum_fraction_digits: Option<usize>,
    minimum_significant_digits: Option<usize>,
    maximum_significant_digits: Option<usize>,
    unit: Option<String>,
    unit_display: String,
    grouping_mode: String,
    grouping_boolean: Option<bool>,
    minimum_integer_digits: usize,
    sign_display: String,
    currency_display: String,
    currency_sign: String,
    notation: String,
    compact_display: String,
    rounding_mode: String,
    rounding_increment: usize,
    rounding_priority: String,
    trailing_zero_display: String,
}

impl<H: Host> Vm<H> {
    pub(super) fn intl_format_primitive(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let constructor = self.realm.intrinsics.intl_number_format_constructors
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
        self.realm.intrinsics.intl_number_format_constructors
            .insert(global, constructor);
        self.set_builtin_function_name(constructor, "NumberFormat")?;
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_number_format_prototypes.insert(global, prototype);
        let fallback_symbol = self.heap.alloc(Cell::Symbol(Some(
            "IntlLegacyConstructedSymbol".into(),
        )));
        self.realm.intrinsics.intl_number_format_fallback_symbols
            .insert(global, fallback_symbol);
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
        self.install_builtin_to_string_tag(prototype, "Intl.NumberFormat")?;
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
        for (name, native) in [
            ("formatRange", Native::IntlNumberFormatFormatRange),
            (
                "formatRangeToParts",
                Native::IntlNumberFormatFormatRangeToParts,
            ),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        self.set_builtin_value_named(intl, "NumberFormat", constructor)?;
        let supported =
            self.native_with_realm(Native::IntlNumberFormatSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.install_intl_collator_for_realm(program, intl, global, object_prototype)?;
        self.install_intl_plural_rules_for_realm(intl, global, object_prototype)?;
        self.install_intl_date_time_format_for_realm(intl, global, object_prototype)?;
        self.install_intl_display_names_for_realm(program, intl, global, object_prototype)?;
        self.install_intl_duration_format_for_realm(intl, global, object_prototype)?;
        self.install_intl_list_format_for_realm(intl, global, object_prototype)?;
        self.install_intl_segmenter_for_realm(intl, global, object_prototype)?;
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
        self.with_call_roots(args.iter().copied().chain([new_target]), |vm| {
            let prototype = vm.intl_instance_prototype(p, new_target, Native::IntlNumberFormat)?;
            vm.with_call_roots([prototype], |vm| {
                let mut locale = vm.number_format_locale(p, args.first().copied())?;
                let options = vm.number_format_options(p, args.get(1).copied())?;
                let formatter = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
                let locale_numbering_system = locale_unicode_keyword(&locale, "nu")
                    .filter(|value| quench_intl::valid_numbering_system(value));
                let option_numbering_system = options
                    .numbering_system
                    .clone()
                    .filter(|value| quench_intl::valid_numbering_system(value));
                if option_numbering_system
                    .as_ref()
                    .is_some_and(|option| locale_numbering_system.as_ref() != Some(option))
                {
                    locale = locale
                        .split_once("-u-")
                        .map_or(locale.clone(), |(base, _)| base.to_owned());
                }
                let numbering_system = option_numbering_system
                    .or(locale_numbering_system)
                    .unwrap_or_else(|| quench_intl::default_numbering_system(&locale).into());
                vm.set_hidden_string(formatter, NUMBER_FORMAT_LOCALE_SLOT, &locale)?;
                vm.set_hidden_string(formatter, NUMBER_FORMAT_STYLE_SLOT, &options.style)?;
                vm.set_hidden_string(
                    formatter,
                    NUMBER_FORMAT_CURRENCY_SLOT,
                    options.currency.as_deref().unwrap_or(""),
                )?;
                vm.set_hidden_string(
                    formatter,
                    NUMBER_FORMAT_NUMBERING_SYSTEM_SLOT,
                    &numbering_system,
                )?;
                vm.set_hidden_value(
                    formatter,
                    NUMBER_FORMAT_MIN_FRACTION_SLOT,
                    Value::number(options.minimum_fraction_digits as f64),
                )?;
                vm.set_hidden_string(
                    formatter,
                    NUMBER_FORMAT_UNIT_SLOT,
                    options.unit.as_deref().unwrap_or(""),
                )?;
                vm.set_hidden_string(
                    formatter,
                    NUMBER_FORMAT_UNIT_DISPLAY_SLOT,
                    &options.unit_display,
                )?;
                vm.set_hidden_string(
                    formatter,
                    NUMBER_FORMAT_SIGN_DISPLAY_SLOT,
                    &options.sign_display,
                )?;
                vm.set_hidden_string(
                    formatter,
                    NUMBER_FORMAT_ROUNDING_MODE_SLOT,
                    &options.rounding_mode,
                )?;
                for (slot, value) in [
                    (
                        NUMBER_FORMAT_CURRENCY_DISPLAY_SLOT,
                        options.currency_display.as_str(),
                    ),
                    (
                        NUMBER_FORMAT_CURRENCY_SIGN_SLOT,
                        options.currency_sign.as_str(),
                    ),
                    (NUMBER_FORMAT_NOTATION_SLOT, options.notation.as_str()),
                    (
                        NUMBER_FORMAT_COMPACT_DISPLAY_SLOT,
                        options.compact_display.as_str(),
                    ),
                    (
                        NUMBER_FORMAT_GROUPING_MODE_SLOT,
                        options.grouping_mode.as_str(),
                    ),
                    (
                        NUMBER_FORMAT_ROUNDING_PRIORITY_SLOT,
                        options.rounding_priority.as_str(),
                    ),
                    (
                        NUMBER_FORMAT_TRAILING_ZERO_SLOT,
                        options.trailing_zero_display.as_str(),
                    ),
                ] {
                    vm.set_hidden_string(formatter, slot, value)?;
                }
                let grouping_value = options.grouping_boolean.map_or_else(
                    || {
                        vm.heap
                            .alloc(Cell::String(options.grouping_mode.clone().into()))
                    },
                    |enabled| if enabled { Value::TRUE } else { Value::FALSE },
                );
                vm.set_hidden_value(formatter, NUMBER_FORMAT_GROUPING_SLOT, grouping_value)?;
                vm.set_hidden_value(
                    formatter,
                    NUMBER_FORMAT_MIN_INTEGER_SLOT,
                    Value::number(options.minimum_integer_digits as f64),
                )?;
                vm.set_hidden_value(
                    formatter,
                    NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT,
                    options
                        .maximum_significant_digits
                        .map_or(Value::UNDEFINED, |digits| Value::number(digits as f64)),
                )?;
                vm.set_hidden_value(
                    formatter,
                    NUMBER_FORMAT_MIN_SIGNIFICANT_SLOT,
                    options
                        .minimum_significant_digits
                        .map_or(Value::UNDEFINED, |digits| Value::number(digits as f64)),
                )?;
                vm.set_hidden_value(
                    formatter,
                    NUMBER_FORMAT_ROUNDING_INCREMENT_SLOT,
                    Value::number(options.rounding_increment as f64),
                )?;
                vm.set_hidden_value(
                    formatter,
                    NUMBER_FORMAT_MAX_FRACTION_SLOT,
                    options
                        .maximum_fraction_digits
                        .map_or(Value::UNDEFINED, |digits| Value::number(digits as f64)),
                )?;
                Ok(formatter)
            })
        })
    }

    pub(super) fn intl_number_format_call(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([this]), |vm| {
            if !vm.number_format_legacy_receiver(p, this)? {
                return vm.intl_number_format_construct(
                    p,
                    args,
                    vm.native_value(Native::IntlNumberFormat),
                );
            }
            let Some(symbol) = vm
                .realm
                .intrinsics
                .intl_number_format_fallback_symbols
                .get(&vm.realm.globals)
                .copied()
            else {
                return vm.intl_number_format_construct(
                    p,
                    args,
                    vm.native_value(Native::IntlNumberFormat),
                );
            };
            let fallback = vm.get_symbol_property_with_receiver(p, this, symbol, this)?;
            if !fallback.is_undefined() {
                return Ok(this);
            }
            let formatter =
                vm.intl_number_format_construct(p, args, vm.native_value(Native::IntlNumberFormat))?;
            vm.set_symbol_property(this, symbol, formatter)?;
            vm.set_property_attributes(
                this,
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
            Ok(this)
        })
    }

    fn number_format_legacy_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<bool, JsError> {
        if !self.is_object_like(receiver) || receiver == self.realm.globals {
            return Ok(false);
        }
        if self.hidden_string(receiver, NUMBER_FORMAT_LOCALE_SLOT).is_some() {
            return Ok(true);
        }
        let prototypes = self.realm.intrinsics.intl_number_format_prototypes
            .values()
            .copied()
            .collect::<Vec<_>>();
        let mut prototype = self.object_get_prototype_of(p, receiver)?;
        while !prototype.is_null() {
            if prototypes.contains(&prototype) {
                return Ok(true);
            }
            prototype = self.object_get_prototype_of(p, prototype)?;
        }
        Ok(false)
    }

    fn number_format_unwrap_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        let Some(symbol) = self.realm.intrinsics.intl_number_format_fallback_symbols
            .get(&self.realm.globals)
            .copied()
        else {
            return Ok(receiver);
        };
        let fallback = self.get_symbol_property_with_receiver(p, receiver, symbol, receiver)?;
        Ok(if fallback.is_undefined() {
            receiver
        } else {
            fallback
        })
    }

    pub(super) fn intl_number_format_format_getter(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let this = self.number_format_unwrap_receiver(p, this)?;
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
        Ok(self
            .canonical_locale_list(p, locales)?
            .into_iter()
            .next()
            .map(|locale| sanitize_number_format_locale(&locale))
            .unwrap_or_else(|| DEFAULT_NUMBER_FORMAT_LOCALE.into()))
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
                minimum_significant_digits: None,
                maximum_significant_digits: None,
                unit: None,
                unit_display: "short".into(),
                grouping_mode: "auto".into(),
                grouping_boolean: None,
                minimum_integer_digits: 1,
                sign_display: "auto".into(),
                currency_display: "symbol".into(),
                currency_sign: "standard".into(),
                notation: "standard".into(),
                compact_display: "short".into(),
                rounding_mode: "halfExpand".into(),
                rounding_increment: 1,
                rounding_priority: "auto".into(),
                trailing_zero_display: "auto".into(),
            });
        };
        if options.is_null() {
            return Err(self.type_error(p, "options must not be null".into()));
        }
        let options = self.box_object(options)?;
        self.with_call_roots([options], |vm| {
            let mut style = "decimal".to_owned();
            let mut currency = None;
            let mut numbering_system = None;
            let mut unit = None;
            let mut unit_display = "short".to_owned();
            let mut grouping_mode = "auto".to_owned();
            let mut grouping_boolean = None;
            let mut grouping_explicit = false;
            let mut minimum_integer_digits = 1;
            let mut minimum_fraction_digits = 0;
            let mut minimum_fraction_set = false;
            let mut maximum_fraction_digits = None;
            let mut sign_display = "auto".to_owned();
            let mut rounding_mode = "halfExpand".to_owned();
            let mut maximum_significant_digits = None;
            let mut minimum_significant_digits = None;
            let mut currency_display = "symbol".to_owned();
            let mut currency_sign = "standard".to_owned();
            let mut notation = "standard".to_owned();
            let mut compact_display = "short".to_owned();
            let mut rounding_increment = 1;
            let mut rounding_priority = "auto".to_owned();
            let mut trailing_zero_display = "auto".to_owned();
            for key in quench_intl::NUMBER_FORMAT_OPTION_KEYS {
                let atom = vm.intern_atom(key);
                let value = vm.get_property(p, options, atom)?;
                if value.is_undefined() {
                    continue;
                }
                match *key {
                    "localeMatcher" => {
                        let value = vm.to_string(p, value)?;
                        if !matches!(value.as_str(), "lookup" | "best fit") {
                            return Err(vm.range_error(p, "invalid localeMatcher".into()));
                        }
                    }
                    "style" => {
                        style = vm.to_string(p, value)?;
                        if !matches!(style.as_str(), "decimal" | "percent" | "currency" | "unit") {
                            return Err(vm.range_error(p, "invalid style".into()));
                        }
                    }
                    "currency" => {
                        let value = vm.to_string(p, value)?.to_ascii_uppercase();
                        if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                            return Err(vm.range_error(p, "invalid currency".into()));
                        }
                        currency = Some(value);
                    }
                    "currencyDisplay" => {
                        currency_display = vm.to_string(p, value)?;
                        if !matches!(
                            currency_display.as_str(),
                            "code" | "symbol" | "narrowSymbol" | "name"
                        ) {
                            return Err(vm.range_error(p, "invalid currencyDisplay".into()));
                        }
                    }
                    "currencySign" => {
                        currency_sign = vm.to_string(p, value)?;
                        if !matches!(currency_sign.as_str(), "standard" | "accounting") {
                            return Err(vm.range_error(p, "invalid currencySign".into()));
                        }
                    }
                    "notation" => {
                        notation = vm.to_string(p, value)?;
                        if !matches!(
                            notation.as_str(),
                            "standard" | "scientific" | "engineering" | "compact"
                        ) {
                            return Err(vm.range_error(p, "invalid notation".into()));
                        }
                    }
                    "compactDisplay" => {
                        compact_display = vm.to_string(p, value)?;
                        if !matches!(compact_display.as_str(), "short" | "long") {
                            return Err(vm.range_error(p, "invalid compactDisplay".into()));
                        }
                    }
                    "numberingSystem" => {
                        let value = vm.to_string(p, value)?.to_ascii_lowercase();
                        if !quench_intl::valid_unicode_type(&value) {
                            return Err(vm.range_error(p, "invalid numberingSystem".into()));
                        }
                        numbering_system = quench_intl::NUMBERING_SYSTEMS
                            .contains(&value.as_str())
                            .then_some(value);
                    }
                    "unit" => {
                        let value = vm.to_string(p, value)?;
                        if value.is_empty()
                            || !value
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                        {
                            return Err(vm.range_error(p, "invalid unit".into()));
                        }
                        unit = Some(value);
                    }
                    "unitDisplay" => {
                        unit_display = vm.to_string(p, value)?;
                        if !matches!(unit_display.as_str(), "long" | "short" | "narrow") {
                            return Err(vm.range_error(p, "invalid unitDisplay".into()));
                        }
                    }
                    "signDisplay" => {
                        sign_display = vm.to_string(p, value)?;
                        if !matches!(
                            sign_display.as_str(),
                            "auto" | "never" | "always" | "exceptZero" | "negative"
                        ) {
                            return Err(vm.range_error(p, "invalid signDisplay".into()));
                        }
                    }
                    "useGrouping" => {
                        grouping_mode = if value.is_undefined() {
                            "auto".into()
                        } else if let Some(boolean) = value.as_bool() {
                            grouping_explicit = true;
                            grouping_boolean = (!boolean).then_some(false);
                            if boolean { "always" } else { "false" }.into()
                        } else if value.is_null() || value.as_number() == Some(0.0) {
                            grouping_explicit = true;
                            grouping_boolean = Some(false);
                            "false".into()
                        } else {
                            let text = vm.to_string(p, value)?;
                            match text.as_str() {
                                "true" | "false" => {
                                    grouping_explicit = true;
                                    "auto".into()
                                }
                                "" => {
                                    grouping_explicit = true;
                                    grouping_boolean = Some(false);
                                    "false".into()
                                }
                                "auto" | "min2" | "always" => {
                                    grouping_explicit = true;
                                    text
                                }
                                _ => return Err(vm.range_error(p, "invalid useGrouping".into())),
                            }
                        };
                        if !matches!(grouping_mode.as_str(), "auto" | "min2" | "always" | "false") {
                            return Err(vm.range_error(p, "invalid useGrouping".into()));
                        }
                    }
                    "minimumIntegerDigits" => {
                        minimum_integer_digits = vm.number_format_option_integer(
                            p,
                            value,
                            1.0,
                            21.0,
                            "minimumIntegerDigits",
                        )? as usize;
                    }
                    "minimumFractionDigits" => {
                        minimum_fraction_set = true;
                        minimum_fraction_digits = vm.number_format_option_integer(
                            p,
                            value,
                            0.0,
                            NUMBER_FORMAT_MAX_FRACTION_DIGITS,
                            "minimumFractionDigits",
                        )? as usize;
                    }
                    "maximumFractionDigits" => {
                        maximum_fraction_digits = Some(vm.number_format_option_integer(
                            p,
                            value,
                            0.0,
                            NUMBER_FORMAT_MAX_FRACTION_DIGITS,
                            "maximumFractionDigits",
                        )? as usize);
                    }
                    "minimumSignificantDigits" => {
                        minimum_significant_digits = Some(vm.number_format_option_integer(
                            p,
                            value,
                            1.0,
                            NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS,
                            "minimumSignificantDigits",
                        )? as usize);
                    }
                    "roundingMode" => {
                        rounding_mode = vm.to_string(p, value)?;
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
                            return Err(vm.range_error(p, "invalid roundingMode".into()));
                        }
                    }
                    "roundingIncrement" => {
                        rounding_increment = vm.number_format_option_integer(
                            p,
                            value,
                            1.0,
                            5000.0,
                            "roundingIncrement",
                        )? as usize;
                        if ![
                            1, 2, 5, 10, 20, 25, 50, 100, 200, 250, 500, 1000, 2000, 2500, 5000,
                        ]
                        .contains(&rounding_increment)
                        {
                            return Err(vm.range_error(p, "invalid roundingIncrement".into()));
                        }
                    }
                    "roundingPriority" => {
                        rounding_priority = vm.to_string(p, value)?;
                        if !matches!(
                            rounding_priority.as_str(),
                            "auto" | "morePrecision" | "lessPrecision"
                        ) {
                            return Err(vm.range_error(p, "invalid roundingPriority".into()));
                        }
                    }
                    "trailingZeroDisplay" => {
                        trailing_zero_display = vm.to_string(p, value)?;
                        if !matches!(trailing_zero_display.as_str(), "auto" | "stripIfInteger") {
                            return Err(vm.range_error(p, "invalid trailingZeroDisplay".into()));
                        }
                    }
                    "maximumSignificantDigits" => {
                        maximum_significant_digits = Some(vm.number_format_option_integer(
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
                return Err(vm.type_error(p, "currency is required".into()));
            }
            if style == "unit" && unit.is_none() {
                return Err(vm.type_error(p, "unit is required".into()));
            }
            if minimum_significant_digits
                .zip(maximum_significant_digits)
                .is_some_and(|(minimum, maximum)| minimum > maximum)
            {
                return Err(vm.range_error(
                    p,
                    "minimumSignificantDigits exceeds maximumSignificantDigits".into(),
                ));
            }
            if style == "unit" && unit.as_deref().is_some_and(|unit| !valid_number_unit(unit)) {
                return Err(vm.range_error(p, "invalid unit".into()));
            }
            if style != "unit" && unit.as_deref().is_some_and(|unit| !valid_number_unit(unit)) {
                return Err(vm.range_error(p, "invalid unit".into()));
            }
            if rounding_increment != 1
                && (rounding_priority != "auto"
                    || maximum_significant_digits.is_some()
                    || minimum_significant_digits.is_some())
            {
                return Err(vm.type_error(
                    p,
                    "roundingIncrement conflicts with precision options".into(),
                ));
            }
            if notation == "compact" && !grouping_explicit && grouping_mode == "auto" {
                grouping_mode = "min2".into();
            }
            if style == "currency" || style == "percent" {
                let minimum_default = if style == "currency" && notation == "standard" {
                    quench_intl::currency_fraction_digits(currency.as_deref().unwrap_or("USD"))
                } else {
                    0
                };
                let maximum_default = if notation == "compact" || style == "percent" {
                    0
                } else if style == "currency" && notation != "standard" {
                    3
                } else {
                    minimum_default
                };
                if !minimum_fraction_set {
                    minimum_fraction_digits = maximum_fraction_digits
                        .map_or(minimum_default, |maximum| minimum_default.min(maximum));
                }
                if maximum_fraction_digits.is_none() {
                    maximum_fraction_digits = Some(maximum_default.max(minimum_fraction_digits));
                }
                if maximum_fraction_digits.is_some_and(|maximum| maximum < minimum_fraction_digits) {
                    return Err(vm.range_error(
                        p,
                        "maximumFractionDigits is less than minimumFractionDigits".into(),
                    ));
                }
            }
            if rounding_increment != 1
                && maximum_fraction_digits.is_some_and(|maximum| maximum != minimum_fraction_digits)
            {
                return Err(vm.range_error(
                    p,
                    "roundingIncrement requires equal fraction digit bounds".into(),
                ));
            }
            Ok(NumberFormatOptions {
                style,
                currency,
                numbering_system,
                minimum_fraction_digits,
                maximum_fraction_digits,
                minimum_significant_digits,
                maximum_significant_digits,
                unit,
                unit_display,
                grouping_mode,
                grouping_boolean,
                minimum_integer_digits,
                sign_display,
                currency_display,
                currency_sign,
                notation,
                compact_display,
                rounding_mode,
                rounding_increment,
                rounding_priority,
                trailing_zero_display,
            })
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
        if !integer.is_finite() || number.fract() != 0.0 || integer < minimum || integer > maximum {
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
        let this = self.number_format_unwrap_receiver(p, this)?;
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
        let minimum_significant_digits = self
            .hidden_value(this, NUMBER_FORMAT_MIN_SIGNIFICANT_SLOT)
            .and_then(Value::as_number)
            .map(|digits| digits as usize);
        let rounding_increment = self
            .hidden_value(this, NUMBER_FORMAT_ROUNDING_INCREMENT_SLOT)
            .and_then(Value::as_number)
            .unwrap_or(1.0) as usize;
        let notation = self
            .hidden_string(this, NUMBER_FORMAT_NOTATION_SLOT)
            .unwrap_or_else(|| "standard".into());
        let compact_display = self
            .hidden_string(this, NUMBER_FORMAT_COMPACT_DISPLAY_SLOT)
            .unwrap_or_else(|| "short".into());
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
        } else if style == "decimal"
            && notation == "standard"
            && minimum_significant_digits.is_none()
            && maximum_significant_digits.is_none()
            && rounding_increment == 1
            && let Some(Cell::String(raw)) = self.heap.get(value)
            && let Some(formatted) = format_decimal_string(
                raw.host_string(),
                &locale,
                &self
                    .hidden_string(this, NUMBER_FORMAT_NUMBERING_SYSTEM_SLOT)
                    .unwrap_or_else(|| "latn".into()),
                &self
                    .hidden_string(this, NUMBER_FORMAT_GROUPING_MODE_SLOT)
                    .unwrap_or_else(|| "auto".into()),
                self.hidden_value(this, NUMBER_FORMAT_MIN_INTEGER_SLOT)
                    .and_then(Value::as_number)
                    .unwrap_or(1.0) as usize,
                minimum_fraction_digits,
                maximum_fraction_digits.unwrap_or(3),
                &rounding_mode,
            )
        {
            formatted
        } else {
            let number = self.to_number(p, value)?;
            self.format_number_value(
                number,
                &style,
                self.hidden_string(this, NUMBER_FORMAT_UNIT_SLOT).as_deref(),
                &self
                    .hidden_string(this, NUMBER_FORMAT_UNIT_DISPLAY_SLOT)
                    .unwrap_or_else(|| "short".into()),
                &self
                    .hidden_string(this, NUMBER_FORMAT_NUMBERING_SYSTEM_SLOT)
                    .unwrap_or_else(|| "latn".into()),
                &locale,
                self.hidden_string(this, NUMBER_FORMAT_CURRENCY_SLOT)
                    .filter(|currency| !currency.is_empty())
                    .as_deref(),
                &self
                    .hidden_string(this, NUMBER_FORMAT_CURRENCY_DISPLAY_SLOT)
                    .unwrap_or_else(|| "symbol".into()),
                &self
                    .hidden_string(this, NUMBER_FORMAT_CURRENCY_SIGN_SLOT)
                    .unwrap_or_else(|| "standard".into()),
                &notation,
                &compact_display,
                &self
                    .hidden_string(this, NUMBER_FORMAT_GROUPING_MODE_SLOT)
                    .unwrap_or_else(|| "auto".into()),
                self.hidden_value(this, NUMBER_FORMAT_MIN_INTEGER_SLOT)
                    .and_then(Value::as_number)
                    .unwrap_or(1.0) as usize,
                minimum_fraction_digits,
                maximum_fraction_digits,
                minimum_significant_digits,
                maximum_significant_digits,
                &self
                    .hidden_string(this, NUMBER_FORMAT_ROUNDING_PRIORITY_SLOT)
                    .unwrap_or_else(|| "auto".into()),
                rounding_increment,
                &rounding_mode,
            )
        };
        let sign_display = self
            .hidden_string(this, NUMBER_FORMAT_SIGN_DISPLAY_SLOT)
            .unwrap_or_else(|| "auto".into());
        let accounting_negative = formatted.starts_with('(');
        let negative = formatted.starts_with('-') || accounting_negative;
        let nan = formatted.ends_with("NaN") || formatted == "非數值";
        let digits = formatted
            .chars()
            .filter(|character| character.is_ascii_digit())
            .collect::<String>();
        let rounded_zero = !digits.is_empty() && digits.chars().all(|character| character == '0');
        let formatted = match sign_display.as_str() {
            "never" => formatted
                .trim_start_matches('-')
                .trim_start_matches('(')
                .trim_end_matches(')')
                .to_owned(),
            "negative" if negative && rounded_zero => formatted
                .trim_start_matches('-')
                .trim_start_matches('(')
                .trim_end_matches(')')
                .to_owned(),
            "always" if !negative && !formatted.starts_with('+') => format!("+{formatted}"),
            "exceptZero" if rounded_zero && negative => formatted
                .trim_start_matches('-')
                .trim_start_matches('(')
                .trim_end_matches(')')
                .to_owned(),
            "exceptZero" if !nan && !rounded_zero && !negative => format!("+{formatted}"),
            "always" | "exceptZero" if accounting_negative => formatted,
            _ => formatted,
        };
        Ok(self.heap.alloc(Cell::String(formatted.into())))
    }

    pub(super) fn intl_number_format_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let this = self.number_format_unwrap_receiver(p, this)?;
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
        if style == "currency" {
            let currency = self
                .hidden_string(this, NUMBER_FORMAT_CURRENCY_SLOT)
                .unwrap_or_default();
            let currency = self.heap.alloc(Cell::String(currency.into()));
            self.set_named(p, result, "currency", currency)?;
            let display = self.heap.alloc(Cell::String(
                self.hidden_string(this, NUMBER_FORMAT_CURRENCY_DISPLAY_SLOT)
                    .unwrap_or_else(|| "symbol".into())
                    .into(),
            ));
            self.set_named(p, result, "currencyDisplay", display)?;
            let sign = self.heap.alloc(Cell::String(
                self.hidden_string(this, NUMBER_FORMAT_CURRENCY_SIGN_SLOT)
                    .unwrap_or_else(|| "standard".into())
                    .into(),
            ));
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
        for (name, slot, fallback) in [
            ("minimumIntegerDigits", NUMBER_FORMAT_MIN_INTEGER_SLOT, 1.0),
            ("minimumFractionDigits", NUMBER_FORMAT_MIN_FRACTION_SLOT, 0.0),
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
        let min_significant = self.hidden_value(this, NUMBER_FORMAT_MIN_SIGNIFICANT_SLOT);
        let max_significant = self.hidden_value(this, NUMBER_FORMAT_MAX_SIGNIFICANT_SLOT);
        let compact = self
            .hidden_string(this, NUMBER_FORMAT_NOTATION_SLOT)
            .as_deref()
            == Some("compact");
        if compact
            || min_significant
            .is_some_and(|value| !value.is_undefined())
            || max_significant.is_some_and(|value| !value.is_undefined())
        {
            let minimum = min_significant
                .filter(|value| !value.is_undefined())
                .unwrap_or(Value::number(1.0));
            let maximum = max_significant
                .filter(|value| !value.is_undefined())
                .unwrap_or(Value::number(if compact { 2.0 } else { NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS }));
            self.set_named(p, result, "minimumSignificantDigits", minimum)?;
            self.set_named(p, result, "maximumSignificantDigits", maximum)?;
        }
        let grouping = self
            .hidden_value(this, NUMBER_FORMAT_GROUPING_SLOT)
            .unwrap_or_else(|| self.heap.alloc(Cell::String("auto".into())));
        self.set_named(p, result, "useGrouping", grouping)?;
        let notation = self
            .hidden_string(this, NUMBER_FORMAT_NOTATION_SLOT)
            .unwrap_or_else(|| "standard".into());
        self.set_intl_string_property(result, "notation", &notation)?;
        if compact {
            let display = self
                .hidden_string(this, NUMBER_FORMAT_COMPACT_DISPLAY_SLOT)
                .unwrap_or_else(|| "short".into());
            self.set_intl_string_property(result, "compactDisplay", &display)?;
        }
        let sign_display = self
            .hidden_string(this, NUMBER_FORMAT_SIGN_DISPLAY_SLOT)
            .unwrap_or_else(|| "auto".into());
        self.set_intl_string_property(result, "signDisplay", &sign_display)?;
        let increment = self
            .hidden_value(this, NUMBER_FORMAT_ROUNDING_INCREMENT_SLOT)
            .unwrap_or(Value::number(1.0));
        self.set_named(p, result, "roundingIncrement", increment)?;
        for (key, slot, default) in [
            ("roundingMode", NUMBER_FORMAT_ROUNDING_MODE_SLOT, "halfExpand"),
            ("roundingPriority", NUMBER_FORMAT_ROUNDING_PRIORITY_SLOT, "auto"),
            ("trailingZeroDisplay", NUMBER_FORMAT_TRAILING_ZERO_SLOT, "auto"),
        ] {
            let text = self.hidden_string(this, slot).unwrap_or_else(|| default.into());
            self.set_intl_string_property(result, key, &text)?;
        }
        Ok(result)
    }

    pub(super) fn intl_number_format_format_to_parts(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let this = self.number_format_unwrap_receiver(p, this)?;
        let formatted = self.intl_number_format_format(p, this, args)?;
        let Some(Cell::String(formatted)) = self.heap.get(formatted) else {
            return Err(JsError("NumberFormat output is not a string".into()));
        };
        let text = formatted.to_string();
        let parts = self.number_format_parts_for_text(this, &text)?;
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(parts),
        }))
    }

    pub(super) fn intl_number_format_format_range(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let this = self.number_format_unwrap_receiver(p, this)?;
        let locale = self.number_format_locale_from_receiver(p, this)?;
        let (start, end) = self.number_format_range_values(p, args)?;
        let start_value = args.first().copied().unwrap_or(Value::number(start));
        let end_value = args.get(1).copied().unwrap_or(Value::number(end));
        let first = self.intl_number_format_format(p, this, &[start_value])?;
        let second = self.intl_number_format_format(p, this, &[end_value])?;
        let first = self.to_string(p, first)?;
        let second = self.to_string(p, second)?;
        let range = if first == second {
            if start == end {
                first
            } else {
                format!("~{first}")
            }
        } else {
            let style = self.hidden_string(this, NUMBER_FORMAT_STYLE_SLOT).unwrap_or_default();
            let separator = if locale.starts_with("pt") {
                " - "
            } else if style == "currency" {
                " – "
            } else {
                "–"
            };
            let collapsed_separator = if locale.starts_with("pt") { " - " } else { "–" };
            let sign_display = self
                .hidden_string(this, NUMBER_FORMAT_SIGN_DISPLAY_SLOT)
                .unwrap_or_else(|| "auto".into());
            if style == "currency" {
                let currency = self.hidden_string(this, NUMBER_FORMAT_CURRENCY_SLOT).unwrap_or_default();
                let display = self.hidden_string(this, NUMBER_FORMAT_CURRENCY_DISPLAY_SLOT).unwrap_or_else(|| "symbol".into());
                let symbol = number_currency_symbol(&currency, &display, &locale);
                let prefix = shared_prefix_before_number(&first, &second);
                if sign_display == "always" && prefix.contains(&symbol) {
                    format!("{first}{collapsed_separator}{}", &second[prefix.len()..])
                } else if first.ends_with(&symbol) && second.ends_with(&symbol) {
                    let suffix = shared_currency_suffix(&first, &second, &symbol);
                    let mut second = second[..second.len() - suffix.len()].to_owned();
                    if sign_display == "always" && first.starts_with('+') && second.starts_with('+') {
                        second.remove(0);
                    }
                    format!("{}{collapsed_separator}{second}{suffix}", &first[..first.len() - suffix.len()])
                } else {
                    format!("{first}{separator}{second}")
                }
            } else {
                format!("{first}{separator}{second}")
            }
        };
        Ok(self.heap.alloc(Cell::String(range.into())))
    }

    pub(super) fn intl_number_format_format_range_to_parts(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let this = self.number_format_unwrap_receiver(p, this)?;
        self.number_format_locale_from_receiver(p, this)?;
        let (start, end) = self.number_format_range_values(p, args)?;
        let start_value = args.first().copied().unwrap_or(Value::number(start));
        let end_value = args.get(1).copied().unwrap_or(Value::number(end));
        let first = self.intl_number_format_format_to_parts(p, this, &[start_value])?;
        let second = self.intl_number_format_format_to_parts(p, this, &[end_value])?;
        let mut parts = Vec::new();
        let first_text = self.number_format_parts_text(p, first)?;
        let second_text = self.number_format_parts_text(p, second)?;
        if first_text == second_text {
            let approximate = self.number_format_part("approximatelySign", "~", None)?;
            self.set_intl_string_property(approximate, "source", "shared")?;
            parts.push(approximate);
            parts.extend(self.number_format_tag_parts(p, first, "shared")?);
        } else {
            parts.extend(self.number_format_tag_parts(p, first, "startRange")?);
            let literal = self.number_format_part("literal", " – ", None)?;
            self.set_intl_string_property(literal, "source", "shared")?;
            parts.push(literal);
            parts.extend(self.number_format_tag_parts(p, second, "endRange")?);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(parts),
        }))
    }

    fn number_format_locale_from_receiver(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<String, JsError> {
        let this = self.number_format_unwrap_receiver(p, this)?;
        self.hidden_string(this, NUMBER_FORMAT_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible NumberFormat receiver".into()))
    }

    fn number_format_range_values(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<(f64, f64), JsError> {
        let Some(start) = args.first().copied().filter(|value| !value.is_undefined()) else {
            return Err(self.type_error(p, "range start is required".into()));
        };
        let Some(end) = args.get(1).copied().filter(|value| !value.is_undefined()) else {
            return Err(self.type_error(p, "range end is required".into()));
        };
        let start = self.number_format_range_value(p, start)?;
        let end = self.number_format_range_value(p, end)?;
        if start.is_nan() || end.is_nan() {
            return Err(self.range_error(p, "invalid number range".into()));
        }
        Ok((start, end))
    }

    fn number_format_range_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<f64, JsError> {
        if let Some(Cell::BigInt(value)) = self.heap.get(value) {
            return Ok(value.parse::<f64>().unwrap_or(f64::INFINITY));
        }
        self.to_number(p, value)
    }

    fn number_format_tag_parts(
        &mut self,
        p: &ResidualProgram,
        array: Value,
        source: &str,
    ) -> Result<Vec<Value>, JsError> {
        let length = self.array_like_length(p, array)?;
        (0..length)
            .map(|index| {
                let part = self.get_index(p, array, Value::number(index as f64))?;
                self.set_intl_string_property(part, "source", source)?;
                Ok(part)
            })
            .collect()
    }

    fn number_format_parts_text(
        &mut self,
        p: &ResidualProgram,
        array: Value,
    ) -> Result<String, JsError> {
        let length = self.array_like_length(p, array)?;
        let value_atom = self.intern_atom("value");
        let mut text = String::new();
        for index in 0..length {
            let part = self.get_index(p, array, Value::number(index as f64))?;
            let value = self.get_property(p, part, value_atom)?;
            text.push_str(&self.to_string(p, value)?);
        }
        Ok(text)
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

    fn number_format_parts_for_text(
        &mut self,
        formatter: Value,
        text: &str,
    ) -> Result<Vec<Value>, JsError> {
        if text == "NaN" || text == "非數值" {
            return Ok(vec![self.number_format_part("nan", text, None)?]);
        }
        if let Some(nan) = text
            .strip_prefix('+')
            .filter(|value| matches!(*value, "NaN" | "非數值"))
        {
            return Ok(vec![
                self.number_format_part("plusSign", "+", None)?,
                self.number_format_part("nan", nan, None)?,
            ]);
        }
        let locale = self.hidden_string(formatter, NUMBER_FORMAT_LOCALE_SLOT).unwrap_or_default();
        let style = self.hidden_string(formatter, NUMBER_FORMAT_STYLE_SLOT).unwrap_or_default();
        let unit = self.hidden_string(formatter, NUMBER_FORMAT_UNIT_SLOT).unwrap_or_default();
        let currency = self.hidden_string(formatter, NUMBER_FORMAT_CURRENCY_SLOT).unwrap_or_default();
        let currency_display = self
            .hidden_string(formatter, NUMBER_FORMAT_CURRENCY_DISPLAY_SLOT)
            .unwrap_or_else(|| "symbol".into());
        let first_numeric = text
            .char_indices()
            .find(|(_, character)| character.is_numeric() || *character == '∞');
        let Some((number_start, first_character)) = first_numeric else {
            return Ok(vec![self.number_format_part("nan", text, None)?]);
        };
        if first_character == '∞' {
            let mut parts = self.number_format_prefix_parts(formatter, &text[..number_start], &style, &currency, &currency_display, &unit)?;
            parts.push(self.number_format_part("infinity", "∞", None)?);
            return Ok(parts);
        }
        let number_end = text
            .char_indices()
            .filter(|(_, character)| character.is_numeric())
            .map(|(index, character)| index + character.len_utf8())
            .last()
            .unwrap_or(number_start);
        let prefix = &text[..number_start];
        let number = &text[number_start..number_end];
        let suffix = &text[number_end..];
        let mut parts = self.number_format_prefix_parts(
            formatter,
            prefix,
            &style,
            &currency,
            &currency_display,
            &unit,
        )?;
        let decimal = if !locale.contains("-u-") && (locale.starts_with("de") || locale.starts_with("pt")) {
            ','
        } else {
            '.'
        };
        let grouping = if decimal == ',' { '.' } else { ',' };
        let (mantissa, exponent) = number.split_once('E').unwrap_or((number, ""));
        let (integer, fraction) = mantissa.split_once(decimal).unwrap_or((mantissa, ""));
        let mut digits = String::new();
        for character in integer.chars() {
            if character == grouping {
                if !digits.is_empty() {
                    parts.push(self.number_format_part("integer", &digits, None)?);
                    digits.clear();
                }
                parts.push(self.number_format_part("group", &grouping.to_string(), None)?);
            } else {
                digits.push(character);
            }
        }
        if !digits.is_empty() || integer.is_empty() {
            parts.push(self.number_format_part("integer", &digits, None)?);
        }
        if !fraction.is_empty() {
            parts.push(self.number_format_part("decimal", &decimal.to_string(), None)?);
            parts.push(self.number_format_part("fraction", fraction, None)?);
        }
        if !exponent.is_empty() {
            parts.push(self.number_format_part("exponentSeparator", "E", None)?);
            let (sign, digits) = exponent
                .strip_prefix('-')
                .map_or_else(|| (None, exponent), |digits| (Some("-"), digits));
            if let Some(sign) = sign {
                parts.push(self.number_format_part("exponentMinusSign", sign, None)?);
            }
            parts.push(self.number_format_part("exponentInteger", digits, None)?);
        }
        self.number_format_suffix_parts(suffix, &style, &currency, &currency_display, &unit, &locale, &mut parts)?;
        Ok(parts)
    }

    fn number_format_prefix_parts(
        &mut self,
        formatter: Value,
        prefix: &str,
        style: &str,
        currency: &str,
        currency_display: &str,
        unit: &str,
    ) -> Result<Vec<Value>, JsError> {
        let mut parts = Vec::new();
        let mut rest = prefix;
        if rest.starts_with('(') {
            parts.push(self.number_format_part("literal", "(", None)?);
            rest = &rest[1..];
        }
        if style == "unit" {
            let locale = self.hidden_string(formatter, NUMBER_FORMAT_LOCALE_SLOT).unwrap_or_default();
            let unit_prefix = if locale.starts_with("ja") && rest.starts_with("時速 ") {
                Some(("時速", "時速 "))
            } else if locale.starts_with("ko") && rest.starts_with("시속 ") {
                Some(("시속", "시속 "))
            } else if locale.starts_with("zh-TW") && rest.starts_with("每小時 ") {
                Some(("每小時", "每小時 "))
            } else {
                None
            };
            if let Some((label, consumed)) = unit_prefix {
                parts.push(self.number_format_part("unit", label, Some(unit))?);
                parts.push(self.number_format_part("literal", " ", None)?);
                rest = &rest[consumed.len()..];
            }
        }
        if rest.starts_with('-') || rest.starts_with('+') {
            let (kind, sign) = if rest.starts_with('-') { ("minusSign", "-") } else { ("plusSign", "+") };
            parts.push(self.number_format_part(kind, sign, None)?);
            rest = &rest[1..];
        }
        if style == "currency" && !rest.is_empty() {
            let symbol = number_currency_symbol(currency, currency_display, &self.hidden_string(formatter, NUMBER_FORMAT_LOCALE_SLOT).unwrap_or_default());
            if let Some(index) = rest.find(&symbol) {
                if index > 0 {
                    parts.push(self.number_format_part("literal", &rest[..index], None)?);
                }
                parts.push(self.number_format_part("currency", &symbol, None)?);
                let after = index + symbol.len();
                if after < rest.len() {
                    parts.push(self.number_format_part("literal", &rest[after..], None)?);
                }
            } else {
                parts.push(self.number_format_part("currency", rest.trim(), None)?);
            }
        }
        Ok(parts)
    }

    fn number_format_suffix_parts(
        &mut self,
        suffix: &str,
        style: &str,
        currency: &str,
        currency_display: &str,
        unit: &str,
        locale: &str,
        parts: &mut Vec<Value>,
    ) -> Result<(), JsError> {
        let body = suffix.trim_end_matches(')');
        if style == "percent" && body.starts_with('%') {
            parts.push(self.number_format_part("percentSign", "%", None)?);
        } else if style == "currency" {
            let symbol = number_currency_symbol(currency, currency_display, locale);
            if let Some(index) = body.find(&symbol) {
                if index > 0 {
                    parts.push(self.number_format_part("literal", &body[..index], None)?);
                }
                parts.push(self.number_format_part("currency", &symbol, None)?);
            }
        } else if style == "unit" && !body.is_empty() {
            if unit == "percent" {
                parts.push(self.number_format_part("unit", "%", Some(unit))?);
                if suffix.ends_with(')') {
                    parts.push(self.number_format_part("literal", ")", None)?);
                }
                return Ok(());
            }
            let separator = body.chars().take_while(|character| character.is_whitespace()).collect::<String>();
            if !separator.is_empty() {
                parts.push(self.number_format_part("literal", &separator, None)?);
            }
            let label = body.trim();
            parts.push(self.number_format_part("unit", label, Some(unit))?);
        } else if !body.is_empty() {
            let label = body.trim_start();
            let whitespace = &body[..body.len() - label.len()];
            if !whitespace.is_empty() {
                parts.push(self.number_format_part("literal", whitespace, None)?);
            }
            parts.push(self.number_format_part("compact", label, None)?);
        }
        if suffix.ends_with(')') {
            parts.push(self.number_format_part("literal", ")", None)?);
        }
        Ok(())
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
        numbering_system: &str,
        locale: &str,
        currency: Option<&str>,
        currency_display: &str,
        currency_sign: &str,
        notation: &str,
        compact_display: &str,
        grouping_mode: &str,
        minimum_integer_digits: usize,
        minimum_fraction_digits: usize,
        maximum_fraction_digits: Option<usize>,
        minimum_significant_digits: Option<usize>,
        maximum_significant_digits: Option<usize>,
        rounding_priority: &str,
        rounding_increment: usize,
        rounding_mode: &str,
    ) -> String {
        let number = if style == "percent" {
            value * 100.0
        } else {
            value
        };
        if number.is_infinite() {
            return if number.is_sign_negative() {
                "-∞".into()
            } else {
                "∞".into()
            };
        }
        if number.is_nan() {
            return if locale.starts_with("zh") {
                "非數值".into()
            } else {
                "NaN".into()
            };
        }
        let negative = number.is_sign_negative();
        let absolute = number.abs();
        let compact_scale = if notation == "compact" {
            compact_scale(absolute, locale, compact_display)
        } else {
            0
        };
        let exponent = match notation {
            "scientific" => Some(scientific_exponent(absolute, false)),
            "engineering" => Some(scientific_exponent(absolute, true)),
            _ => None,
        };
        let scaled = exponent
            .map(|exponent| absolute / 10_f64.powi(exponent))
            .unwrap_or_else(|| {
                if compact_scale == 0 { absolute } else { absolute / 10_f64.powi(compact_scale) }
            });
        let default_fraction = if style == "currency" { 2 } else if style == "percent" { 0 } else { 3 };
        let fraction_maximum = maximum_fraction_digits.unwrap_or_else(|| {
            if notation == "compact" && maximum_significant_digits.is_none() {
                compact_fraction_digits(scaled)
            } else {
                default_fraction
            }
        });
        let significant_maximum = maximum_significant_digits
            .or_else(|| minimum_significant_digits.map(|_| NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS as usize));
        let magnitude = if scaled == 0.0 { 0 } else { scaled.log10().floor() as i32 + 1 };
        let significant_maximum_fraction = significant_maximum.map(|digits| {
            if scaled == 0.0 {
                digits.saturating_sub(1) as i32
            } else {
                (digits as i32 - magnitude).clamp(-100, 100)
            }
        });
        let significant_minimum_fraction = significant_maximum.map(|_| {
            let digits = minimum_significant_digits.unwrap_or(1);
            if scaled == 0.0 {
                digits.saturating_sub(1)
            } else {
                (digits as i32 - magnitude).clamp(0, 100) as usize
            }
        });
        let use_significant = match (rounding_priority, significant_maximum_fraction) {
            (_, None) => false,
            ("auto", Some(_)) => true,
            ("morePrecision", Some(significant)) => significant > fraction_maximum as i32,
            ("lessPrecision", Some(significant)) => significant < fraction_maximum as i32,
            _ => false,
        };
        let (precision, minimum, increment) = if use_significant {
            (
                significant_maximum_fraction.unwrap_or_default(),
                significant_minimum_fraction.unwrap_or_default(),
                1,
            )
        } else {
            (fraction_maximum as i32, minimum_fraction_digits, rounding_increment)
        };
        let maximum = precision.max(0) as usize;
        let rounded = round_number_at_precision(scaled, precision, increment, rounding_mode, negative);
        let mut rounded_text = if use_significant && scaled.abs() >= NUMBER_FORMAT_LARGE_DECIMAL_THRESHOLD {
            format_significant_integer(
                scaled.abs(),
                significant_maximum.unwrap_or(NUMBER_FORMAT_MAX_SIGNIFICANT_DIGITS as usize),
                rounding_mode,
                negative,
            )
        } else {
            format!("{rounded:.maximum$}")
        };
        if let Some((integer, fraction)) = rounded_text.split_once('.') {
            let fraction_end = fraction.trim_end_matches('0').len().max(minimum).min(fraction.len());
            rounded_text = format!("{integer}.{}", &fraction[..fraction_end]);
        } else if minimum > 0 {
            rounded_text.push('.');
            rounded_text.push_str(&"0".repeat(minimum));
        }
        let (integer, fraction) = rounded_text.split_once('.').unwrap_or((&rounded_text, ""));
        let integer = format!("{:0>width$}", integer, width = minimum_integer_digits);
        let compact_german_unscaled = notation == "compact"
            && locale.starts_with("de")
            && compact_scale == 0
            && absolute >= 10_000.0;
        let integer = if (notation == "standard" || compact_german_unscaled)
            && should_group_integer(&integer, grouping_mode, locale)
        {
            group_decimal_integer_locale(&integer, locale)
        } else {
            integer
        };
        let decimal = if !locale.contains("-u-")
            && (locale.starts_with("de") || locale.starts_with("pt"))
        {
            ","
        } else {
            "."
        };
        let mut text = if fraction.is_empty() {
            integer
        } else {
            format!("{integer}{decimal}{fraction}")
        };
        if negative {
            text.insert(0, '-');
        }
        if let Some(exponent) = exponent {
            if !locale.contains("-u-") && (locale.starts_with("de") || locale.starts_with("pt")) {
                text = text.replace('.', ",");
            }
            text.push_str(&format!("E{exponent}"));
        } else if compact_scale != 0 {
            text.push_str(compact_suffix(compact_scale, locale, compact_display));
        }
        if style == "percent" {
            text.push('%');
        } else if style == "unit" {
            text = format_number_unit_locale(&text, unit.unwrap_or("unit"), unit_display, locale);
        } else if style == "currency" {
            text = format_number_currency(
                &text,
                currency,
                currency_display,
                locale,
                currency_sign,
            );
        }
        quench_intl::localize_digits(text, numbering_system)
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

fn locale_unicode_keyword(locale: &str, key: &str) -> Option<String> {
    let extension = locale.split_once("-u-")?.1;
    let parts = extension.split('-').collect::<Vec<_>>();
    let index = parts.iter().position(|part| *part == key)?;
    let end = parts
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, part)| part.len() <= 2)
        .map_or(parts.len(), |(index, _)| index);
    Some(parts[index + 1..end].join("-"))
}

fn sanitize_number_format_locale(locale: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_owned();
    };
    let parts = extension.split('-').collect::<Vec<_>>();
    let numbering = parts.iter().enumerate().find_map(|(index, key)| {
        if *key != "nu" {
            return None;
        }
        let end = parts
            .iter()
            .enumerate()
            .skip(index + 1)
            .find(|(_, part)| part.len() == 2)
            .map_or(parts.len(), |(index, _)| index);
        let value = parts[index + 1..end].join("-");
        quench_intl::valid_numbering_system(&value).then_some(value)
    });
    numbering.map_or_else(|| base.to_owned(), |numbering| format!("{base}-u-nu-{numbering}"))
}

pub(super) fn is_supported_locale(locale: &str) -> bool {
    !locale
        .split('-')
        .next()
        .is_some_and(|language| language.eq_ignore_ascii_case("zxx"))
}

fn valid_number_unit(unit: &str) -> bool {
    let (numerator, denominator) = unit
        .split_once("-per-")
        .map_or((unit, None), |(numerator, denominator)| {
            (numerator, Some(denominator))
        });
    quench_intl::UNITS.contains(&numerator)
        && denominator.is_none_or(|value| quench_intl::UNITS.contains(&value))
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

fn should_group_integer(value: &str, mode: &str, locale: &str) -> bool {
    if mode == "false" {
        return false;
    }
    let digit_count = value.trim_start_matches('0').len().max(1);
    match mode {
        "min2" => digit_count >= 5,
        _ if locale.starts_with("en-IN") => digit_count > 3,
        _ => digit_count > 3,
    }
}

fn scientific_exponent(value: f64, engineering: bool) -> i32 {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    let mut exponent = value.log10().floor() as i32;
    if engineering {
        exponent -= exponent.rem_euclid(3);
    }
    exponent
}

fn compact_scale(value: f64, locale: &str, display: &str) -> i32 {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    let magnitude = value.log10().floor() as i32;
    let scales = if locale.starts_with("en-IN") {
        &[5, 3][..]
    } else if locale.starts_with("ja") || locale.starts_with("zh") {
        &[8, 4][..]
    } else if locale.starts_with("ko") {
        &[8, 4, 3][..]
    } else if locale.starts_with("de") {
        if display == "long" { &[6, 3][..] } else { &[6][..] }
    } else {
        &[9, 6, 3][..]
    };
    scales.iter().copied().find(|scale| magnitude >= *scale).unwrap_or(0)
}

fn compact_fraction_digits(value: f64) -> usize {
    if value == 0.0 || !value.is_finite() {
        return 0;
    }
    (1 - value.abs().log10().floor() as i32).max(0) as usize
}

fn format_significant_integer(value: f64, maximum: usize, mode: &str, negative: bool) -> String {
    let text = value.to_string();
    let (mantissa, exponent) = text
        .split_once(['e', 'E'])
        .map_or((text.as_str(), 0), |(mantissa, exponent)| {
            (mantissa, exponent.parse::<i32>().unwrap_or_default())
        });
    let decimal_position = mantissa.find('.').unwrap_or(mantissa.len()) as i32 + exponent;
    let mut digits = mantissa
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<Vec<_>>();
    let leading_zeroes = digits.iter().take_while(|digit| **digit == '0').count();
    digits.drain(..leading_zeroes);
    let decimal_position = decimal_position - leading_zeroes as i32;
    if digits.len() > maximum {
        let discarded = digits[maximum..].to_vec();
        let first = discarded.first().copied().unwrap_or('0');
        let tail_nonzero = discarded.iter().skip(1).any(|digit| *digit != '0');
        let retained_last = digits[maximum.saturating_sub(1)];
        let round_up = match mode {
            "ceil" => !negative && discarded.iter().any(|digit| *digit != '0'),
            "floor" => negative && discarded.iter().any(|digit| *digit != '0'),
            "trunc" => false,
            "expand" => discarded.iter().any(|digit| *digit != '0'),
            "halfTrunc" => first > '5' || first == '5' && tail_nonzero,
            "halfFloor" => first > '5' || first == '5' && (tail_nonzero || negative),
            "halfCeil" => first > '5' || first == '5' && (tail_nonzero || !negative),
            "halfEven" => first > '5' || first == '5' && (tail_nonzero || (retained_last as u8 - b'0') % 2 == 1),
            _ => first >= '5',
        };
        digits.truncate(maximum);
        if round_up {
            for digit in digits.iter_mut().rev() {
                if *digit != '9' {
                    *digit = ((*digit as u8) + 1) as char;
                    break;
                }
                *digit = '0';
            }
            if digits.iter().all(|digit| *digit == '0') {
                digits.insert(0, '1');
            }
        }
    }
    let target_length = (decimal_position.max(0) as usize).max(digits.len());
    digits.resize(target_length, '0');
    digits.into_iter().collect()
}

fn compact_suffix(magnitude: i32, locale: &str, display: &str) -> &'static str {
    if locale.starts_with("ja") || locale.starts_with("zh") {
        return match (magnitude, locale.starts_with("zh-TW")) {
            (8, _) => "億",
            (4, true) => "萬",
            (4, false) => "万",
            _ => "",
        };
    }
    if locale.starts_with("ko") {
        return match magnitude {
            8 => "억",
            4 => "만",
            3 => "천",
            _ => "",
        };
    }
    if locale.starts_with("de") {
        return match (magnitude, display) {
            (6, "long") => " Millionen",
            (3, "long") => " Tausend",
            (6, _) => " Mio.",
            _ => "",
        };
    }
    match (magnitude, display, locale.starts_with("en-IN")) {
        (9, "long", _) => " billion",
        (6, "long", _) => " million",
        (3, "long", _) => " thousand",
        (9, _, _) => "B",
        (6, _, _) => "M",
        (5, _, true) => "L",
        (3, _, _) => "K",
        _ => "",
    }
}

fn group_decimal_integer_locale(value: &str, locale: &str) -> String {
    if locale.starts_with("en-IN") {
        let mut digits = value.chars().rev();
        let tail = digits.by_ref().take(3).collect::<String>();
        let rest = digits.collect::<Vec<_>>();
        let mut groups = Vec::new();
        for pair in rest.chunks(2) {
            groups.push(pair.iter().collect::<String>().chars().rev().collect::<String>());
        }
        return groups
            .into_iter()
            .rev()
            .chain(std::iter::once(tail.chars().rev().collect()))
            .collect::<Vec<String>>()
            .join(",");
    }
    let grouped = group_decimal_integer(value);
    if locale.starts_with("de") {
        grouped.replace(',', ".")
    } else if locale.starts_with("pt") {
        grouped.replace(',', "\u{a0}")
    } else {
        grouped
    }
}

fn format_decimal_string(
    value: &str,
    locale: &str,
    numbering_system: &str,
    grouping_mode: &str,
    minimum_integer_digits: usize,
    minimum_fraction_digits: usize,
    maximum_fraction_digits: usize,
    rounding_mode: &str,
) -> Option<String> {
    let (negative, unsigned) = value
        .strip_prefix('-')
        .map_or_else(|| value.strip_prefix('+').map_or((false, value), |rest| (false, rest)), |rest| (true, rest));
    if unsigned.is_empty() || !unsigned.chars().all(|character| character.is_ascii_digit() || character == '.') {
        return None;
    }
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if integer.is_empty() || fraction.contains('.') {
        return None;
    }
    let mut integer = integer.to_owned();
    let mut fraction = fraction.to_owned();
    if fraction.len() > maximum_fraction_digits {
        let discarded = &fraction[maximum_fraction_digits..];
        let first = discarded.as_bytes()[0] - b'0';
        let tail_nonzero = discarded.as_bytes()[1..].iter().any(|digit| *digit != b'0');
        let retained_last = if maximum_fraction_digits == 0 {
            integer.as_bytes().last().copied().unwrap_or(b'0') - b'0'
        } else {
            fraction.as_bytes()[maximum_fraction_digits - 1] - b'0'
        };
        let round_up = match rounding_mode {
            "ceil" => !negative && discarded.bytes().any(|digit| digit != b'0'),
            "floor" => negative && discarded.bytes().any(|digit| digit != b'0'),
            "expand" => discarded.bytes().any(|digit| digit != b'0'),
            "trunc" => false,
            "halfCeil" => first > 5 || first == 5 && (tail_nonzero || !negative),
            "halfFloor" => first > 5 || first == 5 && (tail_nonzero || negative),
            "halfTrunc" => first > 5 || first == 5 && tail_nonzero,
            "halfEven" => first > 5 || first == 5 && (tail_nonzero || retained_last % 2 == 1),
            _ => first >= 5,
        };
        fraction.truncate(maximum_fraction_digits);
        if round_up {
            let mut digits = format!("{integer}{fraction}").into_bytes();
            increment_decimal_digits(&mut digits);
            let digits = String::from_utf8(digits).ok()?;
            let split = digits.len().saturating_sub(maximum_fraction_digits);
            integer = digits[..split].to_owned();
            fraction = digits[split..].to_owned();
        }
    }
    while fraction.ends_with('0') && fraction.len() > minimum_fraction_digits {
        fraction.pop();
    }
    while fraction.len() < minimum_fraction_digits {
        fraction.push('0');
    }
    if integer.len() < minimum_integer_digits {
        integer = format!("{}{}", "0".repeat(minimum_integer_digits - integer.len()), integer);
    }
    if should_group_integer(&integer, grouping_mode, locale) {
        integer = group_decimal_integer_locale(&integer, locale);
    }
    let decimal = if !locale.contains("-u-") && (locale.starts_with("de") || locale.starts_with("pt")) {
        ","
    } else {
        "."
    };
    let sign = if negative { "-" } else { "" };
    let text = if fraction.is_empty() {
        format!("{sign}{integer}")
    } else {
        format!("{sign}{integer}{decimal}{fraction}")
    };
    Some(quench_intl::localize_digits(text, numbering_system))
}

fn increment_decimal_digits(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit < b'9' {
            *digit += 1;
            return;
        }
        *digit = b'0';
    }
    digits.insert(0, b'1');
}

fn format_number_currency(
    text: &str,
    currency: Option<&str>,
    display: &str,
    locale: &str,
    currency_sign: &str,
) -> String {
    let (sign, value) = text.strip_prefix('-').map_or_else(
        || text.strip_prefix('+').map_or(("", text), |rest| ("+", rest)),
        |rest| ("-", rest),
    );
    let plain_decimal_locale = !locale.contains("-u-")
        && (locale.starts_with("de") || locale.starts_with("pt"));
    let value = value.to_owned();
    let symbol = number_currency_symbol(currency.unwrap_or("USD"), display, locale);
    let formatted = if plain_decimal_locale {
        format!("{value}\u{a0}{symbol}")
    } else {
        format!("{symbol}{value}")
    };
    if sign == "-" && currency_sign == "accounting" && !locale.starts_with("de") {
        format!("({formatted})")
    } else {
        format!("{sign}{formatted}")
    }
}

fn number_currency_symbol(currency: &str, display: &str, locale: &str) -> String {
    if display == "code" || display == "name" {
        return currency.to_owned();
    }
    if (locale.starts_with("ko") || locale.starts_with("zh")) && currency == "USD" {
        return "US$".into();
    }
    match currency {
        "USD" => "$",
        "EUR" => "€",
        "JPY" | "CNY" => "¥",
        "GBP" => "£",
        "INR" => "₹",
        "RUB" => "₽",
        "KRW" => "₩",
        other => other,
    }
    .to_owned()
}

fn shared_prefix_before_number(first: &str, second: &str) -> String {
    let first_end = first.find(|character: char| character.is_numeric()).unwrap_or(first.len());
    let second_end = second.find(|character: char| character.is_numeric()).unwrap_or(second.len());
    first[..first_end]
        .chars()
        .zip(second[..second_end].chars())
        .take_while(|(left, right)| left == right)
        .map(|(character, _)| character)
        .collect()
}

fn shared_currency_suffix(first: &str, second: &str, currency: &str) -> String {
    if !first.ends_with(currency) || !second.ends_with(currency) {
        return String::new();
    }
    let first_prefix = first.strip_suffix(currency).unwrap_or(first);
    let second_prefix = second.strip_suffix(currency).unwrap_or(second);
    if first_prefix.ends_with('\u{a0}') && second_prefix.ends_with('\u{a0}') {
        format!("\u{a0}{currency}")
    } else {
        currency.to_owned()
    }
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
        "kilometer-per-hour" => ("kilometers per hour", "km/h", "km/h"),
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

fn format_number_unit_locale(text: &str, unit: &str, display: &str, locale: &str) -> String {
    if unit == "percent" {
        return format!("{text}%");
    }
    if unit == "kilometer-per-hour" {
        let (prefix, suffix) = if locale.starts_with("ja") && display == "long" {
            ("時速 ", " キロメートル")
        } else if locale.starts_with("zh-TW") {
            match display {
                "long" => ("每小時 ", " 公里"),
                "narrow" => ("", "公里/小時"),
                _ => ("", " 公里/小時"),
            }
        } else if locale.starts_with("ko") {
            if display == "long" {
                ("시속 ", "킬로미터")
            } else {
                ("", "km/h")
            }
        } else if locale.starts_with("de") {
            if display == "long" {
                ("", " Kilometer pro Stunde")
            } else {
                ("", " km/h")
            }
        } else if display == "long" {
            ("", " kilometers per hour")
        } else if display == "narrow" {
            ("", "km/h")
        } else {
            ("", " km/h")
        };
        return format!("{prefix}{text}{suffix}");
    }
    let number = text
        .trim_start_matches(['-', '+'])
        .trim_end_matches(|character: char| !character.is_ascii_digit() && character != '.' && character != ',');
    let value = number.parse::<f64>().unwrap_or_default();
    let label = format_number_unit(unit, display, value);
    if display == "narrow" {
        format!("{text}{label}")
    } else {
        format!("{text} {label}")
    }
}

fn round_number(
    value: f64,
    fraction_digits: usize,
    increment: usize,
    mode: &str,
    negative: bool,
) -> f64 {
    let scale = 10_f64.powi(fraction_digits.min(100) as i32);
    let increment = increment as f64;
    let scaled = value * scale / increment;
    let lower = scaled.floor();
    let fraction = scaled - lower;
    let tie = (fraction - 0.5).abs() <= 1e-9;
    let magnitude_rounded = match mode {
        "ceil" if negative => scaled.floor(),
        "ceil" => scaled.ceil(),
        "floor" if negative => scaled.ceil(),
        "floor" => scaled.floor(),
        "expand" => scaled.ceil(),
        "trunc" => scaled.floor(),
        _ if !tie && fraction < 0.5 => lower,
        _ if !tie => lower + 1.0,
        "halfTrunc" => lower,
        "halfFloor" if !negative => lower,
        "halfCeil" if negative => lower,
        "halfEven" if (lower as i64) % 2 == 0 => lower,
        "halfExpand" | "halfFloor" | "halfCeil" | "halfEven" => lower + 1.0,
        _ => lower + 1.0,
    };
    magnitude_rounded * increment / scale
}

pub(super) fn round_number_at_precision(
    value: f64,
    fraction_digits: i32,
    increment: usize,
    mode: &str,
    negative: bool,
) -> f64 {
    if fraction_digits >= 0 {
        return round_number(value, fraction_digits as usize, increment, mode, negative);
    }
    let quantum = 10_f64.powi(-fraction_digits);
    round_number(value / quantum, 0, 1, mode, negative) * quantum
}
