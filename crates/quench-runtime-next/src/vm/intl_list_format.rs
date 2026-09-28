use super::*;

const LIST_FORMAT_LOCALE_SLOT: &str = "\0rqj:intl-list-format-locale";
const LIST_FORMAT_TYPE_SLOT: &str = "\0rqj:intl-list-format-type";
const LIST_FORMAT_STYLE_SLOT: &str = "\0rqj:intl-list-format-style";

#[derive(Clone)]
struct ListPart {
    kind: &'static str,
    value: String,
}

impl<H: Host> Vm<H> {
    pub(super) fn install_intl_list_format_for_realm(
        &mut self,
        intl: Value,
        global: Value,
        object_prototype: Value,
    ) -> Result<(), JsError> {
        let constructor = self.native_with_realm(Native::IntlListFormat, global, global);
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_prototype)));
        self.intl_list_format_constructors
            .insert(global, constructor);
        self.intl_list_format_prototypes.insert(global, prototype);
        self.set_builtin_function_name(constructor, "ListFormat")?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            PropertyKey::string(prototype_atom),
            list_format_immutable_attributes(),
        );
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.install_builtin_to_string_tag(prototype, "Intl.ListFormat")?;
        let format = self.native_with_realm(Native::IntlListFormatFormat, global, global);
        self.set_builtin_function_name(format, "format")?;
        self.set_builtin_value_named(prototype, "format", format)?;
        let format_to_parts =
            self.native_with_realm(Native::IntlListFormatFormatToParts, global, global);
        self.set_builtin_function_name(format_to_parts, "formatToParts")?;
        self.set_builtin_value_named(prototype, "formatToParts", format_to_parts)?;
        let resolved_options =
            self.native_with_realm(Native::IntlListFormatResolvedOptions, global, global);
        self.set_builtin_function_name(resolved_options, "resolvedOptions")?;
        self.set_builtin_value_named(prototype, "resolvedOptions", resolved_options)?;
        let supported =
            self.native_with_realm(Native::IntlListFormatSupportedLocalesOf, global, global);
        self.set_builtin_function_name(supported, "supportedLocalesOf")?;
        self.set_builtin_value_named(constructor, "supportedLocalesOf", supported)?;
        self.set_builtin_value_named(intl, "ListFormat", constructor)
    }

    pub(super) fn intl_list_format_construct(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Value,
    ) -> Result<Value, JsError> {
        let locales = self.collator_locale_list(p, args.first().copied())?;
        let locale = locales.first().cloned().unwrap_or_else(|| "en-US".into());
        let (style, kind) = self.list_format_options(p, args.get(1).copied())?;
        let prototype_atom = self.intern_atom("prototype");
        let candidate = self.get_property(p, new_target, prototype_atom)?;
        let prototype = if self.is_object_like(candidate) {
            candidate
        } else {
            let realm = self.function_realm(p, new_target)?;
            self.intl_list_format_prototypes
                .get(&realm)
                .copied()
                .unwrap_or(self.object_proto)
        };
        let instance = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        self.set_hidden_string(instance, LIST_FORMAT_LOCALE_SLOT, &locale)?;
        self.set_hidden_string(instance, LIST_FORMAT_STYLE_SLOT, &style)?;
        self.set_hidden_string(instance, LIST_FORMAT_TYPE_SLOT, &kind)?;
        Ok(instance)
    }

    fn list_format_options(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<(String, String), JsError> {
        let Some(options) = options.filter(|value| !value.is_undefined()) else {
            return Ok(("long".into(), "conjunction".into()));
        };
        if options.is_null() {
            return Err(self.type_error(p, "options must not be null".into()));
        }
        if !self.is_object_like(options) {
            return Err(self.type_error(p, "options must be an object".into()));
        }
        let mut style = "long".to_owned();
        let mut kind = "conjunction".to_owned();
        for (key, allowed) in [
            ("localeMatcher", &["lookup", "best fit"][..]),
            ("type", &["conjunction", "disjunction", "unit"][..]),
            ("style", &["long", "short", "narrow"][..]),
        ] {
            let atom = self.intern_atom(key);
            let value = self.get_property(p, options, atom)?;
            if value.is_undefined() {
                continue;
            }
            let value = self.to_string(p, value)?;
            if !allowed.contains(&value.as_str()) {
                return Err(self.range_error(p, format!("invalid {key}").into()));
            }
            match key {
                "style" => style = value,
                "type" => kind = value,
                _ => {}
            }
        }
        Ok((style, kind))
    }

    pub(super) fn intl_list_format_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if native == Native::IntlListFormatSupportedLocalesOf {
            let locales = self.collator_locale_list(p, args.first().copied())?;
            self.list_format_locale_matcher(p, args.get(1).copied())?;
            let values = locales
                .into_iter()
                .filter(|locale| super::intl_number::is_supported_locale(locale))
                .map(|locale| self.heap.alloc(Cell::String(locale.into())))
                .collect::<Vec<_>>();
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }
        if native == Native::IntlListFormatResolvedOptions {
            let locale = self.list_format_slot(p, this, LIST_FORMAT_LOCALE_SLOT)?;
            let kind = self.list_format_slot(p, this, LIST_FORMAT_TYPE_SLOT)?;
            let style = self.list_format_slot(p, this, LIST_FORMAT_STYLE_SLOT)?;
            let result = self.object();
            self.set_intl_string_property(result, "locale", &locale)?;
            self.set_intl_string_property(result, "type", &kind)?;
            self.set_intl_string_property(result, "style", &style)?;
            return Ok(result);
        }
        self.list_format_slot(p, this, LIST_FORMAT_LOCALE_SLOT)?;
        let parts = self.list_format_parts(p, this, args.first().copied())?;
        if native == Native::IntlListFormatFormat {
            let text = parts
                .iter()
                .map(|part| part.value.as_str())
                .collect::<String>();
            return Ok(self.heap.alloc(Cell::String(text.into())));
        }
        let mut values = Vec::with_capacity(parts.len());
        for part in parts {
            let object = self.object();
            self.set_intl_string_property(object, "type", part.kind)?;
            self.set_intl_string_property(object, "value", &part.value)?;
            values.push(object);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    fn list_format_parts(
        &mut self,
        p: &ResidualProgram,
        formatter: Value,
        value: Option<Value>,
    ) -> Result<Vec<ListPart>, JsError> {
        let value = value.unwrap_or(Value::UNDEFINED);
        let mut items = Vec::new();
        if !value.is_undefined() {
            let iterator = self.get_iterator(p, value)?;
            let done_atom = self.intern_atom("done");
            let value_atom = self.intern_atom("value");
            loop {
                let step = match self.iterator_next(p, iterator) {
                    Ok(step) => step,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                let done = match self.get_property(p, step, done_atom) {
                    Ok(done) => self.truthy(done),
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                if done {
                    break;
                }
                let item = match self.get_property(p, step, value_atom) {
                    Ok(item) => item,
                    Err(error) => return Err(self.iterator_abrupt(p, iterator, error)),
                };
                let Some(item) = (match self.heap.get(item) {
                    Some(Cell::String(value)) => Some(value.to_string()),
                    _ => None,
                }) else {
                    let error = self.type_error(
                        p,
                        "ListFormat iterable values must be strings".into(),
                    );
                    return Err(self.iterator_abrupt(p, iterator, error));
                };
                items.push(item);
            }
        }
        let kind = self.list_format_slot(p, formatter, LIST_FORMAT_TYPE_SLOT)?;
        let style = self.list_format_slot(p, formatter, LIST_FORMAT_STYLE_SLOT)?;
        let locale = self.list_format_slot(p, formatter, LIST_FORMAT_LOCALE_SLOT)?;
        Ok(join_list_parts(&items, &locale, &kind, &style))
    }

    fn list_format_slot(
        &mut self,
        p: &ResidualProgram,
        object: Value,
        slot: &str,
    ) -> Result<String, JsError> {
        self.hidden_string(object, slot)
            .ok_or_else(|| self.type_error(p, "not a ListFormat object".into()))
    }

    fn list_format_locale_matcher(
        &mut self,
        p: &ResidualProgram,
        options: Option<Value>,
    ) -> Result<(), JsError> {
        let Some(options) = options.filter(|value| !value.is_undefined()) else {
            return Ok(());
        };
        if options.is_null() {
            return Err(self.type_error(p, "options must be an object".into()));
        }
        let options = self.box_object(options)?;
        let atom = self.intern_atom("localeMatcher");
        let value = self.get_property(p, options, atom)?;
        if value.is_undefined() {
            return Ok(());
        }
        let value = self.to_string(p, value)?;
        if matches!(value.as_str(), "lookup" | "best fit") {
            Ok(())
        } else {
            Err(self.range_error(p, "invalid localeMatcher".into()))
        }
    }
}

fn list_format_immutable_attributes() -> PropertyAttributes {
    PropertyAttributes {
        writable: false,
        enumerable: false,
        configurable: false,
        accessor: false,
        getter: None,
        setter: None,
    }
}

fn join_list_parts(items: &[String], locale: &str, kind: &str, style: &str) -> Vec<ListPart> {
    let mut parts = Vec::new();
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            let separator = list_separator(index, items.len(), locale, kind, style);
            parts.push(ListPart {
                kind: "literal",
                value: separator.into(),
            });
        }
        parts.push(ListPart {
            kind: "element",
            value: item.clone(),
        });
    }
    parts
}

pub(super) fn list_separator(
    index: usize,
    length: usize,
    locale: &str,
    kind: &str,
    style: &str,
) -> &'static str {
    if style == "narrow" {
        return if kind == "unit" { " " } else { ", " };
    }
    let spanish = locale.starts_with("es");
    if kind == "unit" && !spanish {
        return ", ";
    }
    let word = if style == "short" && !spanish && kind == "conjunction" {
        " & "
    } else if kind == "disjunction" {
        if spanish { " o " } else { " or " }
    } else if spanish {
        " y "
    } else {
        " and "
    };
    if length == 2 {
        return word;
    }
    if index + 1 != length {
        return ", ";
    }
    if kind == "unit" && spanish && style == "short" {
        return ", ";
    }
    if spanish {
        word
    } else if style == "short" {
        if kind == "disjunction" { ", or " } else { ", & " }
    } else {
        match (kind, style) {
            ("disjunction", "short") => ", or ",
            (_, "short") => ", & ",
            ("disjunction", _) => ", or ",
            _ => ", and ",
        }
    }
}
