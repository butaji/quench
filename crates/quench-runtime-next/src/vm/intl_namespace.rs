use super::*;

const INTL_LOCALE_SLOT: &str = "\0rqj:intl-locale";

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_namespace_for_realm(
        &mut self,
        _program: &ResidualProgram,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let get_canonical_locales =
            self.native_with_realm(Native::IntlGetCanonicalLocales, global, global);
        self.install_builtin_static_function(intl, "getCanonicalLocales", get_canonical_locales)?;

        let supported_values =
            self.native_with_realm(Native::IntlSupportedValuesOf, global, global);
        self.install_builtin_static_function(intl, "supportedValuesOf", supported_values)?;
        self.install_builtin_to_string_tag(intl, "Intl")?;
        self.install_intl_relative_time_format_for_realm(intl, global, object_prototype)?;

        let locale = self.native_with_realm(Native::IntlLocale, global, global);
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.intl_locale_prototypes.insert(global, prototype);
        self.set_builtin_function_name(locale, "Locale")?;
        self.set_builtin_value_named(locale, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            locale,
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
        self.set_builtin_value_named(prototype, "constructor", locale)?;
        self.install_builtin_to_string_tag(prototype, "Intl.Locale")?;
        for (name, native) in [
            ("toString", Native::IntlLocaleToString),
            ("maximize", Native::IntlLocaleMaximize),
            ("minimize", Native::IntlLocaleMinimize),
            ("getCalendars", Native::IntlLocaleGetCalendars),
            ("getCollations", Native::IntlLocaleGetCollations),
            ("getHourCycles", Native::IntlLocaleGetHourCycles),
            ("getNumberingSystems", Native::IntlLocaleGetNumberingSystems),
            ("getTimeZones", Native::IntlLocaleGetTimeZones),
            ("getTextInfo", Native::IntlLocaleGetTextInfo),
            ("getWeekInfo", Native::IntlLocaleGetWeekInfo),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        for (name, native) in [
            ("baseName", Native::IntlLocaleBaseNameGetter),
            ("language", Native::IntlLocaleLanguageGetter),
            ("script", Native::IntlLocaleScriptGetter),
            ("region", Native::IntlLocaleRegionGetter),
            ("variants", Native::IntlLocaleVariantsGetter),
            ("calendar", Native::IntlLocaleCalendarGetter),
            ("collation", Native::IntlLocaleCollationGetter),
            ("hourCycle", Native::IntlLocaleHourCycleGetter),
            ("caseFirst", Native::IntlLocaleCaseFirstGetter),
            ("firstDayOfWeek", Native::IntlLocaleFirstDayOfWeekGetter),
            ("numberingSystem", Native::IntlLocaleNumberingSystemGetter),
            ("numeric", Native::IntlLocaleNumericGetter),
        ] {
            let getter = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            self.set_builtin_value_named(prototype, name, getter)?;
            let atom = self.intern_atom(name);
            self.set_property_attributes(
                prototype,
                PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: true,
                    getter: Some(getter),
                    setter: None,
                },
            );
        }
        self.set_builtin_value_named(intl, "Locale", locale)?;

        Ok(())
    }

    fn install_builtin_static_function(
        &mut self,
        object: Value,
        name: &str,
        function: Value,
    ) -> Result<(), JsError> {
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    pub(super) fn intl_namespace_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlGetCanonicalLocales => self.intl_get_canonical_locales(p, args),
            Native::IntlSupportedValuesOf => self.intl_supported_values_of(p, args),
            Native::IntlLocale => Err(self.type_error(p, "Intl.Locale requires 'new'".into())),
            Native::IntlLocaleToString
            | Native::IntlLocaleMaximize
            | Native::IntlLocaleMinimize
            | Native::IntlLocaleGetCalendars
            | Native::IntlLocaleGetCollations
            | Native::IntlLocaleGetHourCycles
            | Native::IntlLocaleGetNumberingSystems
            | Native::IntlLocaleGetTimeZones
            | Native::IntlLocaleGetTextInfo
            | Native::IntlLocaleGetWeekInfo
            | Native::IntlLocaleBaseNameGetter
            | Native::IntlLocaleLanguageGetter
            | Native::IntlLocaleScriptGetter
            | Native::IntlLocaleRegionGetter
            | Native::IntlLocaleVariantsGetter
            | Native::IntlLocaleCalendarGetter
            | Native::IntlLocaleCollationGetter
            | Native::IntlLocaleHourCycleGetter
            | Native::IntlLocaleCaseFirstGetter
            | Native::IntlLocaleFirstDayOfWeekGetter
            | Native::IntlLocaleNumberingSystemGetter
            | Native::IntlLocaleNumericGetter => {
                self.intl_locale_native(p, native, this)
            }
            _ => Err(JsError("invalid Intl namespace operation".into())),
        }
    }

    fn intl_locale_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let locale = self
            .hidden_string(this, INTL_LOCALE_SLOT)
            .ok_or_else(|| self.type_error(p, "incompatible Intl.Locale receiver".into()))?;
        let base = locale_base_name(&locale);
        let parts = base.split('-').collect::<Vec<_>>();
        let language = parts.first().copied().unwrap_or_default();
        let script = parts
            .iter()
            .skip(1)
            .find(|part| part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
            .copied();
        let region = parts
            .iter()
            .skip(1)
            .find(|part| {
                part.len() == 2 || (part.len() == 3 && part.bytes().all(|byte| byte.is_ascii_digit()))
            })
            .copied();
        let variants = parts
            .iter()
            .skip(1)
            .filter(|part| {
                part.len() >= 4 && !(script == Some(part) || region == Some(part))
            })
            .copied()
            .collect::<Vec<_>>();
        let property = match native {
            Native::IntlLocaleBaseNameGetter => Some(base.clone()),
            Native::IntlLocaleLanguageGetter => Some(language.to_owned()),
            Native::IntlLocaleScriptGetter => script.map(str::to_owned),
            Native::IntlLocaleRegionGetter => region.map(str::to_owned),
            Native::IntlLocaleVariantsGetter => {
                (!variants.is_empty()).then(|| variants.join("-"))
            }
            Native::IntlLocaleCalendarGetter => unicode_locale_keyword(&locale, "ca"),
            Native::IntlLocaleCollationGetter => unicode_locale_keyword(&locale, "co"),
            Native::IntlLocaleHourCycleGetter => unicode_locale_keyword(&locale, "hc"),
            Native::IntlLocaleCaseFirstGetter => unicode_locale_keyword(&locale, "kf")
                .map(|value| if value == "true" { String::new() } else { value }),
            Native::IntlLocaleFirstDayOfWeekGetter => unicode_locale_keyword(&locale, "fw"),
            Native::IntlLocaleNumberingSystemGetter => unicode_locale_keyword(&locale, "nu"),
            _ => None,
        };
        if let Some(value) = property {
            return Ok(self.heap.alloc(Cell::String(value.into())));
        }
        if matches!(
            native,
            Native::IntlLocaleBaseNameGetter
                | Native::IntlLocaleScriptGetter
                | Native::IntlLocaleRegionGetter
                | Native::IntlLocaleVariantsGetter
                | Native::IntlLocaleCalendarGetter
                | Native::IntlLocaleCollationGetter
                | Native::IntlLocaleHourCycleGetter
                | Native::IntlLocaleCaseFirstGetter
                | Native::IntlLocaleFirstDayOfWeekGetter
                | Native::IntlLocaleNumberingSystemGetter
        ) {
            return Ok(Value::UNDEFINED);
        }
        match native {
            Native::IntlLocaleNumericGetter => Ok(if unicode_locale_keyword(&locale, "kn")
                .is_some_and(|value| value != "false")
            {
                Value::TRUE
            } else {
                Value::FALSE
            }),
            Native::IntlLocaleToString => Ok(self.heap.alloc(Cell::String(locale.into()))),
            Native::IntlLocaleMaximize => {
                let maximized = maximize_locale(&locale);
                self.new_intl_locale_object(this, maximized)
            }
            Native::IntlLocaleMinimize => {
                let minimized = minimize_locale(&locale);
                self.new_intl_locale_object(this, minimized)
            }
            Native::IntlLocaleGetCalendars => Ok(self.string_array(vec![
                unicode_locale_keyword(&locale, "ca").unwrap_or_else(|| "gregory".into()),
            ])),
            Native::IntlLocaleGetCollations => Ok(self.string_array(vec![
                unicode_locale_keyword(&locale, "co").unwrap_or_else(|| "default".into()),
            ])),
            Native::IntlLocaleGetHourCycles => Ok(self.string_array(vec![
                unicode_locale_keyword(&locale, "hc").unwrap_or_else(|| "h12".into()),
            ])),
            Native::IntlLocaleGetNumberingSystems => Ok(self.string_array(vec![
                unicode_locale_keyword(&locale, "nu").unwrap_or_else(|| "latn".into()),
            ])),
            Native::IntlLocaleGetTimeZones => {
                if region.is_none() {
                    return Ok(Value::UNDEFINED);
                }
                let zones = quench_intl::supported_time_zones()
                    .into_iter()
                    .filter(|zone| zone.starts_with("America/"))
                    .collect();
                Ok(self.string_array(zones))
            }
            Native::IntlLocaleGetTextInfo => {
                let direction = if matches!(language, "ar" | "fa" | "he" | "ps" | "ur") {
                    "rtl"
                } else {
                    "ltr"
                };
                let result = self.object();
                self.set_intl_string_property(result, "direction", direction)?;
                Ok(result)
            }
            Native::IntlLocaleGetWeekInfo => {
                let first_day = unicode_locale_keyword(&locale, "fw")
                    .as_deref()
                    .map(weekday_number)
                    .or_else(|| week_first_day(region.unwrap_or_default()));
                let first_day = first_day.unwrap_or(WEEKDAY_MONDAY);
                let weekend = if matches!(region, Some("AE" | "BH" | "DJ" | "DZ" | "EG" | "IQ" | "IR" | "JO" | "KW" | "LY" | "OM" | "QA" | "SA" | "SD" | "SY" | "YE")) {
                    vec![Value::number(WEEKDAY_FRIDAY), Value::number(WEEKDAY_SATURDAY)]
                } else {
                    vec![Value::number(WEEKDAY_SATURDAY), Value::number(WEEKDAY_SUNDAY)]
                };
                let result = self.object();
                self.set_named(p, result, "firstDay", Value::number(first_day as f64))?;
                let weekend = self.new_array(weekend);
                self.set_named(p, result, "weekend", weekend)?;
                Ok(result)
            }
            _ => Err(JsError("invalid Intl.Locale operation".into())),
        }
    }

    fn new_intl_locale_object(
        &mut self,
        source: Value,
        locale: String,
    ) -> Result<Value, JsError> {
        let prototype = self
            .object_data(source)
            .map(|object| object.proto)
            .unwrap_or(self.object_proto);
        let result = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(result, INTL_LOCALE_SLOT, &locale)?;
        Ok(result)
    }

    pub(super) fn intl_namespace_construct(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        if native != Native::IntlLocale {
            return Err(self.type_error(p, "value is not a constructor".into()));
        }
        let tag = args
            .first()
            .copied()
            .ok_or_else(|| self.type_error(p, "Intl.Locale requires a tag".into()))?;
        if !matches!(self.heap.get(tag), Some(Cell::String(_))) && !self.is_object_like(tag) {
            return Err(self.type_error(p, "locale tag must be a string".into()));
        }
        let tag = self.to_string(p, tag)?;
        let mut locale = canonical_locale(&tag)
            .ok_or_else(|| self.range_error(p, "invalid language tag".into()))?;
        if let Some(options) = args.get(1).copied().filter(|value| !value.is_undefined()) {
            if options.is_null() {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            let options = self.box_object(options)?;
            for option in [
                "language",
                "script",
                "region",
                "variants",
                "calendar",
                "collation",
                "hourCycle",
                "firstDayOfWeek",
                "caseFirst",
                "numeric",
                "numberingSystem",
            ] {
                let atom = self.intern_atom(option);
                let value = self.get_property(p, options, atom)?;
                if value.is_undefined() {
                    continue;
                }
                if matches!(option, "language" | "script" | "region" | "variants") {
                    locale = apply_locale_base_option(p, self, &locale, option, value)?;
                    continue;
                }
                if option == "numeric" {
                    locale = set_unicode_keyword(
                        &locale,
                        "kn",
                        Some(if self.truthy(value) { "true" } else { "false" }),
                    );
                    continue;
                }
                let text = self.to_string(p, value)?;
                let text = match option {
                    "calendar" => {
                        let value = option_value(p, self, option, &text)?;
                        quench_intl::calendar_alias(&value)
                    }
                    "hourCycle" => {
                        if !matches!(text.as_str(), "h11" | "h12" | "h23" | "h24") {
                            return Err(self.range_error(p, "invalid hourCycle".into()));
                        }
                        text
                    }
                    "caseFirst" => {
                        if !matches!(text.as_str(), "upper" | "lower" | "false") {
                            return Err(self.range_error(p, "invalid caseFirst".into()));
                        }
                        text
                    }
                    "firstDayOfWeek" => normalize_first_day_option(p, self, &text)?,
                    _ => option_value(p, self, option, &text)?,
                };
                let key = match option {
                    "calendar" => "ca",
                    "collation" => "co",
                    "hourCycle" => "hc",
                    "firstDayOfWeek" => "fw",
                    "caseFirst" => "kf",
                    "numberingSystem" => "nu",
                    _ => unreachable!(),
                };
                locale = set_unicode_keyword(&locale, key, Some(&text.to_ascii_lowercase()));
            }
            locale = canonical_locale(&locale)
                .ok_or_else(|| self.range_error(p, "invalid language tag".into()))?;
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            let realm = self.function_realm(p, new_target)?;
            self.intl_locale_prototypes
                .get(&realm)
                .copied()
                .unwrap_or(self.object_proto)
        };
        let instance = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(instance, INTL_LOCALE_SLOT, &locale)?;
        Ok(instance)
    }

    fn intl_get_canonical_locales(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(locales) = args.first().copied() else {
            return Ok(self.string_array(Vec::new()));
        };
        if locales.is_undefined() {
            return Ok(self.string_array(Vec::new()));
        }

        let mut canonical = Vec::<String>::new();
        if let Some(locale) = self.hidden_string(locales, INTL_LOCALE_SLOT) {
            canonical.push(
                canonical_locale(&locale)
                    .ok_or_else(|| self.range_error(p, "invalid locale identifier".into()))?,
            );
        } else if matches!(self.heap.get(locales), Some(Cell::String(_))) {
            let locale = self.to_string(p, locales)?;
            canonical.push(
                canonical_locale(&locale)
                    .ok_or_else(|| self.range_error(p, "invalid locale identifier".into()))?,
            );
        } else {
            if locales.is_null() {
                return Err(self.type_error(p, "locales must not be null".into()));
            }
            let object = self.box_object(locales)?;
            let length = self.array_like_length(p, object)?;
            for index in 0..length {
                let key = Value::number(index as f64);
                if !self.has_property(p, object, key)? {
                    continue;
                }
                let value = self.get_index(p, object, key)?;
                let locale = if self.is_object_like(value)
                    && let Some(locale) = self.hidden_string(value, INTL_LOCALE_SLOT)
                {
                    locale
                } else if matches!(self.heap.get(value), Some(Cell::String(_))) {
                    self.to_string(p, value)?
                } else if self.is_object_like(value) {
                    self.to_string(p, value)?
                } else {
                    return Err(self.type_error(p, "locale list element is not a string".into()));
                };
                let locale = canonical_locale(&locale)
                    .ok_or_else(|| self.range_error(p, "invalid locale identifier".into()))?;
                if !canonical.contains(&locale) {
                    canonical.push(locale);
                }
            }
        }
        Ok(self.string_array(canonical))
    }

    fn string_array(&mut self, values: Vec<String>) -> Value {
        let elements = values
            .into_iter()
            .map(|value| self.heap.alloc(Cell::String(value.into())))
            .collect::<Vec<_>>();
        self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(elements),
        })
    }

    fn intl_supported_values_of(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let key = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let values: &[&str] = match key.as_str() {
            "calendar" => quench_intl::CALENDARS,
            "collation" => quench_intl::COLLATIONS,
            "currency" => quench_intl::CURRENCIES,
            "numberingSystem" => quench_intl::NUMBERING_SYSTEMS,
            "unit" => quench_intl::UNITS,
            "timeZone" => {
                let values = quench_intl::supported_time_zones();
                return Ok(self.string_array(values));
            }
            _ => return Err(self.range_error(p, "invalid key".into())),
        };
        Ok(self.string_array(values.iter().map(|value| (*value).into()).collect()))
    }
}

fn canonical_locale(locale: &str) -> Option<String> {
    let locale = unique_unicode_keywords(locale);
    quench_intl::canonicalize_locale_identifier(&locale)
        .ok()
        .map(|locale| {
            if locale == "cel-gaulish" {
                "xtg".to_owned()
            } else {
                locale
            }
        })
        .map(|locale| locale.replace("-kn-true", "-kn"))
        .map(|locale| locale.replace("-kf-true", "-kf"))
}

fn unique_unicode_keywords(locale: &str) -> String {
    let Some((base, extension)) = locale.split_once("-u-") else {
        return locale.to_owned();
    };
    let parts = extension.split('-').collect::<Vec<_>>();
    let boundary = parts.iter().position(|part| part.len() == 1);
    let (unicode, trailing) = boundary.map_or((parts.as_slice(), &[][..]), |index| {
        (&parts[..index], &parts[index..])
    });
    let mut attributes = Vec::new();
    let mut index = 0;
    while index < unicode.len() && unicode[index].len() != 2 {
        attributes.push(unicode[index]);
        index += 1;
    }
    let mut keywords = std::collections::BTreeMap::new();
    while index < unicode.len() {
        let key = unicode[index];
        index += 1;
        let start = index;
        while index < unicode.len() && unicode[index].len() != 2 {
            index += 1;
        }
        keywords.entry(key).or_insert_with(|| unicode[start..index].to_vec());
    }
    if attributes.is_empty() && keywords.is_empty() {
        return locale.to_owned();
    }
    attributes.sort_unstable();
    let mut output = attributes;
    for (key, values) in keywords {
        output.push(key);
        output.extend(values);
    }
    let mut result = format!("{base}-u-{}", output.join("-"));
    result.push_str(&extension_suffix(trailing));
    result
}

fn extension_suffix(extension: &[&str]) -> String {
    if extension.is_empty() {
        String::new()
    } else {
        format!("-{}", extension.join("-"))
    }
}

fn apply_locale_base_option<H: Host>(
    p: &ResidualProgram,
    vm: &mut Vm<H>,
    locale: &str,
    option: &str,
    input: Value,
) -> Result<String, JsError> {
    let text = vm.to_string(p, input)?;
    let base = locale_base_name(locale);
    let suffix = &locale[base.len()..];
    let mut parts = base.split('-').map(str::to_owned).collect::<Vec<_>>();
    let language = parts.first().cloned().unwrap_or_default();
    let script_index = parts
        .get(1)
        .is_some_and(|part| part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .then_some(1);
    match option {
        "language" => {
            if text.contains('-') || !valid_language(&text) {
                return Err(vm.range_error(p, "invalid language".into()));
            }
            parts[0] = quench_intl::language_alias(text.to_ascii_lowercase());
        }
        "script" => {
            if text.len() != 4 || !text.bytes().all(|byte| byte.is_ascii_alphabetic()) {
                return Err(vm.range_error(p, "invalid script".into()));
            }
            let script = quench_intl::titlecase_script(&text);
            if let Some(index) = script_index {
                parts[index] = script;
            } else {
                parts.insert(1, script);
            }
        }
        "region" => {
            let valid = (text.len() == 2 && text.bytes().all(|byte| byte.is_ascii_alphabetic()))
                || (text.len() == 3 && text.bytes().all(|byte| byte.is_ascii_digit()));
            if !valid {
                return Err(vm.range_error(p, "invalid region".into()));
            }
            let index = (1..parts.len())
                .find(|index| {
                    parts[*index].len() == 2
                        || (parts[*index].len() == 3
                            && parts[*index].bytes().all(|byte| byte.is_ascii_digit()))
                })
                .unwrap_or(parts.len());
            if index == parts.len() {
                parts.push(text.clone());
            } else {
                parts[index] = text.clone();
            }
            parts[index] = quench_intl::canonical_region(&text, &[language]);
        }
        "variants" => {
            let mut variants = text
                .split('-')
                .map(|part| part.to_ascii_lowercase())
                .collect::<Vec<_>>();
            if variants.is_empty()
                || variants.iter().any(|variant| {
                    !((5..=8).contains(&variant.len())
                        && variant.bytes().all(|byte| byte.is_ascii_alphanumeric())
                        || variant.len() == 4
                            && variant
                                .as_bytes()
                                .first()
                                .is_some_and(u8::is_ascii_digit))
                })
            {
                return Err(vm.range_error(p, "invalid variants".into()));
            }
            variants.sort();
            if variants.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(vm.range_error(p, "invalid variants".into()));
            }
            let region_index = (1..parts.len()).find(|index| {
                parts[*index].len() == 2
                    || (parts[*index].len() == 3
                        && parts[*index].bytes().all(|byte| byte.is_ascii_digit()))
            });
            let has_script = parts.get(1).is_some_and(|part| {
                part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic())
            });
            let variant_start = region_index.map_or_else(
                || if has_script { 2 } else { 1 },
                |index| index + 1,
            );
            parts.truncate(variant_start);
            parts.extend(variants);
        }
        _ => unreachable!(),
    }
    Ok(format!("{}{suffix}", parts.join("-")))
}

fn valid_language(value: &str) -> bool {
    matches!(value.len(), 2 | 3 | 5..=8)
        && value.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn option_value<H: Host>(
    p: &ResidualProgram,
    vm: &mut Vm<H>,
    name: &str,
    value: &str,
) -> Result<String, JsError> {
    if value.split('-').all(|part| {
        (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
    }) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(vm.range_error(p, format!("invalid {name}").into()))
    }
}

fn normalize_first_day_option<H: Host>(
    p: &ResidualProgram,
    vm: &mut Vm<H>,
    value: &str,
) -> Result<String, JsError> {
    let value = value.to_ascii_lowercase();
    let weekday = match value.as_str() {
        "0" | "7" => "sun",
        "1" => "mon",
        "2" => "tue",
        "3" => "wed",
        "4" => "thu",
        "5" => "fri",
        "6" => "sat",
        _ => value.as_str(),
    };
    if weekday == "true" {
        return Ok(weekday.into());
    }
    option_value(p, vm, "firstDayOfWeek", weekday)
}

fn set_unicode_keyword(locale: &str, key: &str, value: Option<&str>) -> String {
    let (base, unicode_extension) = locale
        .split_once("-u-")
        .map_or((locale, None), |(base, extension)| (base, Some(extension)));
    let (unicode, trailing_extensions) = unicode_extension.map_or(
        (Vec::new(), String::new()),
        |extension| {
        let parts = extension.split('-').collect::<Vec<_>>();
        let boundary = parts.iter().position(|part| part.len() == 1);
        let (unicode, trailing) = boundary.map_or((parts.as_slice(), &[][..]), |index| {
            (&parts[..index], &parts[index..])
        });
            (
                unicode.to_vec(),
                if trailing.is_empty() {
                    String::new()
                } else {
                    format!("-{}", trailing.join("-"))
                },
            )
        },
    );
    let mut attributes = Vec::new();
    let mut keywords = std::collections::BTreeMap::<String, Vec<String>>::new();
    let mut index = 0;
    while index < unicode.len() && unicode[index].len() != 2 {
        attributes.push(unicode[index].to_owned());
        index += 1;
    }
    while index < unicode.len() {
        let current = unicode[index].to_owned();
        index += 1;
        let start = index;
        while index < unicode.len() && unicode[index].len() != 2 {
            index += 1;
        }
        keywords.insert(
            current,
            unicode[start..index]
                .iter()
                .map(|part| (*part).to_owned())
                .collect(),
        );
    }
    if let Some(value) = value {
        keywords.insert(
            key.into(),
            if value == "true" {
                Vec::new()
            } else {
                value.split('-').map(str::to_owned).collect()
            },
        );
    } else {
        keywords.remove(key);
    }
    if keywords.is_empty() && attributes.is_empty() {
        return format!("{base}{trailing_extensions}");
    }
    let mut subtags = attributes;
    for (key, values) in keywords {
        subtags.push(key);
        subtags.extend(values);
    }
    format!("{base}-u-{}{trailing_extensions}", subtags.join("-"))
}

fn locale_base_name(locale: &str) -> String {
    locale
        .split('-')
        .take_while(|part| part.len() != 1)
        .collect::<Vec<_>>()
        .join("-")
}

fn unicode_locale_keyword(locale: &str, key: &str) -> Option<String> {
    let extension = locale.split_once("-u-")?.1;
    let parts = extension.split('-').collect::<Vec<_>>();
    let index = parts.iter().position(|part| *part == key)?;
    let end = parts
        .iter()
        .enumerate()
        .skip(index + 1)
        .find(|(_, part)| part.len() == 2 || part.len() == 1)
        .map_or(parts.len(), |(end, _)| end);
    let value = parts[index + 1..end].join("-");
    Some(if value.is_empty() {
        "true".into()
    } else {
        value
    })
}

const WEEKDAY_SUNDAY: f64 = 7.0;
const WEEKDAY_MONDAY: f64 = 1.0;
const WEEKDAY_TUESDAY: f64 = 2.0;
const WEEKDAY_WEDNESDAY: f64 = 3.0;
const WEEKDAY_THURSDAY: f64 = 4.0;
const WEEKDAY_FRIDAY: f64 = 5.0;
const WEEKDAY_SATURDAY: f64 = 6.0;

fn weekday_number(day: &str) -> f64 {
    match day {
        "sun" => WEEKDAY_SUNDAY,
        "mon" => WEEKDAY_MONDAY,
        "tue" => WEEKDAY_TUESDAY,
        "wed" => WEEKDAY_WEDNESDAY,
        "thu" => WEEKDAY_THURSDAY,
        "fri" => WEEKDAY_FRIDAY,
        "sat" => WEEKDAY_SATURDAY,
        _ => WEEKDAY_MONDAY,
    }
}

fn week_first_day(region: &str) -> Option<f64> {
    Some(if matches!(region, "US" | "CA" | "JP" | "MX" | "BR" | "PH") {
        WEEKDAY_SUNDAY
    } else {
        WEEKDAY_MONDAY
    })
}

fn maximize_locale(tag: &str) -> String {
    if tag == "xtg" || tag.starts_with("xtg-") || tag == "posix" {
        return tag.to_owned();
    }
    let base = locale_base_name(tag);
    let extension = &tag[base.len()..];
    let parts = base.split('-').collect::<Vec<_>>();
    let language = parts.first().copied().unwrap_or("und");
    let script = parts
        .iter()
        .skip(1)
        .find(|part| part.len() == 4 && part.bytes().all(|byte| byte.is_ascii_alphabetic()))
        .copied();
    let region = parts
        .iter()
        .skip(1)
        .find(|part| part.len() == 2 || part.len() == 3)
        .copied();
    let inferred_language = if language == "und" {
        match (script, region) {
            (Some("Thai"), _) => "th",
            (_, Some("AT")) => "de",
            (_, Some("CW")) => "pap",
            (_, Some("419")) => "es",
            (_, Some("150" | "AQ")) => "en",
            (Some("Cyrl"), Some("RO")) => "bg",
            _ => "en",
        }
    } else {
        language
    };
    let (default_script, default_region) = likely_subtags(inferred_language);
    let default_script = if inferred_language == "zh"
        && matches!(region, Some("TW" | "HK" | "MO"))
    {
        "Hant"
    } else {
        default_script
    };
    let default_region = if inferred_language == "en" && script == Some("Shaw") {
        "GB"
    } else if inferred_language == "zh" && script == Some("Hant") {
        "TW"
    } else {
        default_region
    };
    let maximized = format!(
        "{inferred_language}-{}-{}",
        script.unwrap_or(default_script),
        region.unwrap_or(default_region)
    );
    let variants = parts
        .iter()
        .skip(1)
        .filter(|part| {
            part.len() >= 4
                && Some(**part) != script
                && Some(**part) != region
        })
        .copied()
        .collect::<Vec<_>>();
    if variants.is_empty() {
        format!("{maximized}{extension}")
    } else {
        format!("{maximized}-{}{extension}", variants.join("-"))
    }
}

fn minimize_locale(tag: &str) -> String {
    if tag == "xtg" || tag.starts_with("xtg-") || tag == "posix" {
        return tag.to_owned();
    }
    let maximized = maximize_locale(tag);
    let base = locale_base_name(&maximized);
    let extension = &maximized[base.len()..];
    let parts = base.split('-').collect::<Vec<_>>();
    let language = parts.first().copied().unwrap_or("und");
    let script = parts.get(1).copied().unwrap_or("Latn");
    let region = parts.get(2).copied().unwrap_or("US");
    let variants = parts.get(3..).unwrap_or(&[]).join("-");
    let (default_script, default_region) = likely_subtags(language);
    let default_region = if language == "en" && script == "Shaw" {
        "GB"
    } else if language == "zh" && script == "Hant" {
        "TW"
    } else {
        default_region
    };
    let minimized = if language == "zh" && script == "Hant" && region == "TW" {
        "zh-TW".to_owned()
    } else if script == default_script && region == default_region {
        language.to_owned()
    } else if script == default_script {
        format!("{language}-{region}")
    } else if region == default_region {
        format!("{language}-{script}")
    } else {
        format!("{language}-{script}-{region}")
    };
    if variants.is_empty() {
        format!("{minimized}{extension}")
    } else {
        format!("{minimized}-{variants}{extension}")
    }
}

fn likely_subtags(language: &str) -> (&'static str, &'static str) {
    match language {
        "und" => ("Latn", "US"),
        "aa" => ("Latn", "ET"),
        "aae" => ("Latn", "IT"),
        "ar" => ("Arab", "EG"),
        "bg" => ("Cyrl", "BG"),
        "cs" => ("Latn", "CZ"),
        "de" => ("Latn", "DE"),
        "en" => ("Latn", "US"),
        "es" => ("Latn", "ES"),
        "fr" => ("Latn", "FR"),
        "he" => ("Hebr", "IL"),
        "hak" => ("Hans", "CN"),
        "hsn" => ("Hans", "CN"),
        "hy" => ("Armn", "AM"),
        "hyw" => ("Armn", "AM"),
        "hi" => ("Deva", "IN"),
        "jbo" => ("Latn", "001"),
        "ja" => ("Jpan", "JP"),
        "ko" => ("Kore", "KR"),
        "ru" => ("Cyrl", "RU"),
        "ro" => ("Latn", "RO"),
        "sr" => ("Cyrl", "RS"),
        "th" => ("Thai", "TH"),
        "pap" => ("Latn", "CW"),
        "uz" => ("Latn", "UZ"),
        "zh" => ("Hans", "CN"),
        _ => ("Latn", "US"),
    }
}
