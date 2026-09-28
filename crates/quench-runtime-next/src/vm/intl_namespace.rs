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
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlGetCanonicalLocales => self.intl_get_canonical_locales(p, args),
            Native::IntlSupportedValuesOf => self.intl_supported_values_of(p, args),
            Native::IntlLocale => Err(self.type_error(p, "Intl.Locale requires 'new'".into())),
            _ => Err(JsError("invalid Intl namespace operation".into())),
        }
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
        if tag.is_null()
            || tag.is_undefined()
            || matches!(self.heap.get(tag), Some(Cell::Symbol(_)))
        {
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
            for (option, key) in [
                ("calendar", "ca"),
                ("collation", "co"),
                ("hourCycle", "hc"),
                ("caseFirst", "kf"),
                ("firstDayOfWeek", "fw"),
                ("numberingSystem", "nu"),
            ] {
                let atom = self.intern_atom(option);
                let value = self.get_property(p, options, atom)?;
                if value.is_undefined() {
                    continue;
                }
                let value = self.to_string(p, value)?;
                let value = if option == "calendar" {
                    quench_intl::calendar_alias(&value)
                } else {
                    value.to_ascii_lowercase()
                };
                if !quench_intl::valid_unicode_type(&value) {
                    return Err(self.range_error(p, format!("invalid {option}").into()));
                }
                locale = add_unicode_locale_key(&locale, key, &value);
            }
            locale = canonical_locale(&locale)
                .ok_or_else(|| self.range_error(p, "invalid language tag".into()))?;
        }
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            self.object_proto
        };
        let instance = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(instance, INTL_LOCALE_SLOT, &locale)?;
        let base_name = locale_base_name(&locale);
        let base_name = self.heap.alloc(Cell::String(base_name.into()));
        self.set_builtin_value_named(instance, "baseName", base_name)?;
        let language = locale.split('-').next().unwrap_or_default().to_owned();
        let language = self.heap.alloc(Cell::String(language.into()));
        self.set_builtin_value_named(instance, "language", language)?;
        for (property, key) in [
            ("calendar", "ca"),
            ("collation", "co"),
            ("hourCycle", "hc"),
            ("caseFirst", "kf"),
            ("firstDayOfWeek", "fw"),
            ("numberingSystem", "nu"),
        ] {
            if let Some(value) = unicode_locale_keyword(&locale, key) {
                let value = self.heap.alloc(Cell::String(value.into()));
                self.set_builtin_value_named(instance, property, value)?;
            }
        }
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
        if matches!(self.heap.get(locales), Some(Cell::String(_))) {
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
    quench_intl::canonicalize_locale_identifier(locale).ok()
}

fn add_unicode_locale_key(locale: &str, key: &str, value: &str) -> String {
    let (base, extension) = locale
        .split_once("-u-")
        .map_or((locale, None), |(base, extension)| (base, Some(extension)));
    let mut keywords = extension
        .map(|extension| extension.split('-').collect::<Vec<_>>())
        .unwrap_or_default();
    if let Some(index) = keywords
        .iter()
        .position(|part| part.len() == 2 && *part == key)
    {
        keywords.truncate(index);
    }
    keywords.extend([key, value]);
    format!("{base}-u-{}", keywords.join("-"))
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
