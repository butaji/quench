use super::*;

const DISPLAY_NAMES_LOCALE_SLOT: &str = "\0rqj:intl-display-names-locale";
const DISPLAY_NAMES_TYPE_SLOT: &str = "\0rqj:intl-display-names-type";
const DISPLAY_NAMES_STYLE_SLOT: &str = "\0rqj:intl-display-names-style";
const DISPLAY_NAMES_FALLBACK_SLOT: &str = "\0rqj:intl-display-names-fallback";
const DISPLAY_NAMES_LANGUAGE_DISPLAY_SLOT: &str = "\0rqj:intl-display-names-language-display";
const DISPLAY_NAMES_OPTIONS: &[(&str, &str, &str)] = &[
    ("localeMatcher", "best fit", "lookup|best fit"),
    ("style", "long", "long|short|narrow"),
    (
        "type",
        "",
        "language|region|script|currency|calendar|dateTimeField",
    ),
    ("fallback", "code", "code|none"),
    ("languageDisplay", "dialect", "dialect|standard"),
];
const DATE_TIME_FIELDS: &[&str] = &[
    "era",
    "year",
    "quarter",
    "month",
    "weekOfYear",
    "weekday",
    "day",
    "dayPeriod",
    "hour",
    "minute",
    "second",
    "timeZoneName",
];

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_display_names_for_realm(
        &mut self,
        program: &ResidualProgram,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlDisplayNames, global, global);
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.realm.intrinsics.intl_display_names_constructors
            .insert(global, constructor);
        self.realm.intrinsics.intl_display_names_prototypes.insert(global, prototype);
        self.set_builtin_function_name(constructor, "DisplayNames")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            immutable_data_attributes(),
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.DisplayNames")?;
        for (name, native, length) in [
            ("of", Native::IntlDisplayNamesOf, "of"),
            (
                "resolvedOptions",
                Native::IntlDisplayNamesResolvedOptions,
                "resolvedOptions",
            ),
        ] {
            let method = self.native_with_realm(native, global, global);
            self.set_builtin_function_name(method, length)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        self.set_builtin_value_named(intl, "DisplayNames", constructor)?;
        let _ = program;
        Ok(())
    }

    pub(super) fn intl_display_names_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain([new_target]), |vm| {
            let prototype = vm.intl_instance_prototype(p, new_target, Native::IntlDisplayNames)?;
            vm.with_call_roots([prototype], |vm| {
                let locales = vm.canonical_locale_list(p, args.first().copied())?;
                let locale = locales.first().cloned().unwrap_or_else(|| "en-US".into());
                let options = vm.display_names_options(p, args.get(1).copied())?;
                let instance = vm.heap.alloc(Cell::Object(Self::empty_object(prototype)));
                for (slot, value) in [
                    (DISPLAY_NAMES_LOCALE_SLOT, locale),
                    (DISPLAY_NAMES_TYPE_SLOT, options.display_type),
                    (DISPLAY_NAMES_STYLE_SLOT, options.style),
                    (DISPLAY_NAMES_FALLBACK_SLOT, options.fallback),
                    (
                        DISPLAY_NAMES_LANGUAGE_DISPLAY_SLOT,
                        options.language_display,
                    ),
                ] {
                    vm.set_hidden_string(instance, slot, &value)?;
                }
                Ok(instance)
            })
        })
    }

    fn display_names_options(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<DisplayNamesOptions, JsError> {
        let options = self.get_options_object(p, value)?;
        let mut parsed = DisplayNamesOptions::default();
        for (name, default, allowed) in DISPLAY_NAMES_OPTIONS {
            let atom = self.intern_atom(name);
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                if default.is_empty() {
                    return Err(self.type_error(p, "options.type is required".into()));
                }
                parsed.set(name, (*default).into());
                continue;
            }
            let value = self.to_string(p, value)?;
            if !allowed.split('|').any(|candidate| candidate == value) {
                return Err(self.range_error(p, format!("invalid {name}").into()));
            }
            parsed.set(name, value);
        }
        Ok(parsed)
    }

    pub(super) fn intl_display_names_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let locale = self.display_names_slot(p, this, DISPLAY_NAMES_LOCALE_SLOT)?;
        match native {
            Native::IntlDisplayNamesOf => self.display_name_of(p, this, args, &locale),
            Native::IntlDisplayNamesResolvedOptions => self.display_names_resolved_options(p, this),
            _ => Err(JsError("invalid Intl.DisplayNames method".into())),
        }
    }

    fn display_name_of(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        locale: &str,
    ) -> Result<Value, JsError> {
        let code = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let kind = self.display_names_slot(p, this, DISPLAY_NAMES_TYPE_SLOT)?;
        if !valid_display_name_code(&code, &kind) {
            return Err(self.range_error(p, "invalid code".into()));
        }
        let fallback = self.display_names_slot(p, this, DISPLAY_NAMES_FALLBACK_SLOT)?;
        Ok(
            display_name_value(&code, &kind, locale, &fallback).map_or(Value::UNDEFINED, |name| {
                self.heap.alloc(Cell::String(name.into()))
            }),
        )
    }

    fn display_names_resolved_options(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        let result = self.object();
        for (key, slot) in [
            ("locale", DISPLAY_NAMES_LOCALE_SLOT),
            ("style", DISPLAY_NAMES_STYLE_SLOT),
            ("type", DISPLAY_NAMES_TYPE_SLOT),
            ("fallback", DISPLAY_NAMES_FALLBACK_SLOT),
            ("languageDisplay", DISPLAY_NAMES_LANGUAGE_DISPLAY_SLOT),
        ] {
            let value = self.display_names_slot(p, this, slot)?;
            let atom = self.intern_atom(key);
            let value = self.heap.alloc(Cell::String(value.into()));
            self.set_property(result, atom, value)?;
        }
        let _ = p;
        Ok(result)
    }

    fn display_names_slot(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        slot: &str,
    ) -> Result<String, JsError> {
        self.hidden_string(object, slot)
            .ok_or_else(|| self.type_error(p, "not a DisplayNames object".into()))
    }
}

fn immutable_data_attributes() -> PropertyAttributes {
    PropertyAttributes {
        writable: false,
        enumerable: false,
        configurable: false,
        accessor: false,
        getter: None,
        setter: None,
    }
}

fn valid_display_name_code(code: &str, kind: &str) -> bool {
    match kind {
        "language" => valid_language_code(code),
        "region" => {
            (code.len() == 2 && code.bytes().all(|byte| byte.is_ascii_alphabetic()))
                || (code.len() == 3 && code.bytes().all(|byte| byte.is_ascii_digit()))
        }
        "script" => valid_alpha_code(code, 4),
        "currency" => valid_alpha_code(code, 3),
        "calendar" => code.split('-').all(|part| {
            (3..=8).contains(&part.len()) && part.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }),
        "dateTimeField" => DATE_TIME_FIELDS.contains(&code),
        _ => false,
    }
}

fn valid_alpha_code(code: &str, length: usize) -> bool {
    code.len() == length && code.bytes().all(|byte| byte.is_ascii_alphabetic())
}

fn valid_language_code(code: &str) -> bool {
    let mut parts = code.split('-');
    let language = parts.next().unwrap_or_default();
    if !((2..=3).contains(&language.len()) || (5..=8).contains(&language.len()))
        || !language.bytes().all(|byte| byte.is_ascii_alphabetic())
    {
        return false;
    }
    let mut script_seen = false;
    let mut region_seen = false;
    let mut variants = FxHashSet::default();
    for part in parts {
        let alpha = part.bytes().all(|byte| byte.is_ascii_alphabetic());
        let digit = part.bytes().all(|byte| byte.is_ascii_digit());
        if part.len() == 1 || !(2..=8).contains(&part.len()) {
            return false;
        }
        if part.len() == 4 && alpha {
            if script_seen {
                return false;
            }
            script_seen = true;
        } else if (part.len() == 2 && alpha) || (part.len() == 3 && digit) {
            if region_seen {
                return false;
            }
            region_seen = true;
        } else if ((5..=8).contains(&part.len())
            && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
            || (part.len() == 4
                && part.as_bytes().first().is_some_and(u8::is_ascii_digit)
                && part[1..].bytes().all(|byte| byte.is_ascii_alphanumeric()))
        {
            if !variants.insert(part.to_ascii_lowercase()) {
                return false;
            }
        } else {
            return false;
        }
    }
    true
}

#[derive(Default)]
struct DisplayNamesOptions {
    display_type: String,
    style: String,
    fallback: String,
    language_display: String,
}

impl DisplayNamesOptions {
    fn set(&mut self, key: &str, value: String) {
        match key {
            "type" => self.display_type = value,
            "style" => self.style = value,
            "fallback" => self.fallback = value,
            "languageDisplay" => self.language_display = value,
            _ => {}
        }
    }
}

fn display_name_value(code: &str, kind: &str, locale: &str, fallback: &str) -> Option<String> {
    let language = code.split('-').next().unwrap_or(code);
    let name = if kind == "language" && locale.starts_with("en") {
        match language {
            "ar" => Some("Arabic"),
            "de" => Some("German"),
            "en" => Some("English"),
            "es" => Some("Spanish"),
            "fr" => Some("French"),
            "it" => Some("Italian"),
            "ja" => Some("Japanese"),
            "ko" => Some("Korean"),
            "ru" => Some("Russian"),
            "zh" => Some("Chinese"),
            _ => None,
        }
    } else {
        None
    };
    name.map(str::to_owned)
        .or_else(|| match kind {
            "currency" if quench_intl::CURRENCIES.contains(&code) => Some(code.to_owned()),
            "calendar" if quench_intl::CALENDARS.contains(&code) => Some(code.to_owned()),
            _ => None,
        })
        .or_else(|| (fallback == "code").then(|| code.to_owned()))
}
