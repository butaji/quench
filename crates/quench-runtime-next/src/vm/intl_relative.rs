use super::*;

const RELATIVE_LOCALE_SLOT: &str = "\0rqj:intl-relative-locale";
const RELATIVE_STYLE_SLOT: &str = "\0rqj:intl-relative-style";
const RELATIVE_NUMERIC_SLOT: &str = "\0rqj:intl-relative-numeric";
const RELATIVE_NUMBERING_SYSTEM_SLOT: &str = "\0rqj:intl-relative-numbering-system";

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
        let resolved =
            self.native_with_realm(Native::IntlRelativeTimeFormatResolvedOptions, global, global);
        self.set_builtin_function_name(resolved, "resolvedOptions")?;
        self.set_builtin_value_named(prototype, "resolvedOptions", resolved)?;
        self.set_builtin_value_named(intl, "RelativeTimeFormat", constructor)
    }

    pub(super) fn intl_relative_time_format_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        _this: Value,
        _args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::IntlRelativeTimeFormat => {
                Err(self.type_error(p, "RelativeTimeFormat requires 'new'".into()))
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
        let locale = locales.first().cloned().unwrap_or_else(|| "en".into());
        let options = match args.get(1).copied().filter(|value| !value.is_undefined()) {
            Some(value) if value.is_null() => {
                return Err(self.type_error(p, "options must not be null".into()));
            }
            Some(value) => self.box_object(value)?,
            None => self.heap.alloc(Cell::Object(Self::empty_object(Value::NULL))),
        };
        let style = self.string_option(p, options, "style", "long", &["long", "short", "narrow"])?;
        let numeric = self.string_option(p, options, "numeric", "always", &["always", "auto"])?;
        let numbering_system_atom = self.intern_atom("numberingSystem");
        let numbering_system = self.get_property(p, options, numbering_system_atom)?;
        let numbering_system = if numbering_system.is_undefined() {
            quench_intl::default_numbering_system(&locale).to_owned()
        } else {
            let requested = self.to_string(p, numbering_system)?.to_ascii_lowercase();
            if !quench_intl::valid_unicode_type(&requested) {
                return Err(self.range_error(p, "invalid numberingSystem".into()));
            }
            if quench_intl::NUMBERING_SYSTEMS.contains(&requested.as_str()) {
                requested
            } else {
                quench_intl::default_numbering_system(&locale).to_owned()
            }
        };
        let prototype_atom = self.intern_atom("prototype");
        let prototype = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.object_data(prototype).is_some() {
            prototype
        } else {
            self.object_proto
        };
        let instance = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(instance, RELATIVE_LOCALE_SLOT, &locale)?;
        self.set_hidden_string(instance, RELATIVE_STYLE_SLOT, &style)?;
        self.set_hidden_string(instance, RELATIVE_NUMERIC_SLOT, &numeric)?;
        self.set_hidden_string(instance, RELATIVE_NUMBERING_SYSTEM_SLOT, &numbering_system)?;
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

    fn string_option(
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
