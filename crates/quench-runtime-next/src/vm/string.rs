use super::*;

const STRING_METHODS: &[(&str, Native, f64)] = &[
    ("at", Native::StringAt, 1.0),
    ("charAt", Native::StringCharAt, 1.0),
    ("charCodeAt", Native::StringCharCodeAt, 1.0),
    ("codePointAt", Native::StringCodePointAt, 1.0),
    ("concat", Native::StringConcat, 1.0),
    ("endsWith", Native::StringEndsWith, 1.0),
    ("includes", Native::StringIncludes, 1.0),
    ("isWellFormed", Native::StringIsWellFormed, 0.0),
    ("indexOf", Native::StringIndexOf, 1.0),
    ("lastIndexOf", Native::StringLastIndexOf, 1.0),
    ("localeCompare", Native::StringLocaleCompare, 1.0),
    ("match", Native::StringMatch, 1.0),
    ("matchAll", Native::StringMatchAll, 1.0),
    ("normalize", Native::StringNormalize, 0.0),
    ("padEnd", Native::StringPadEnd, 1.0),
    ("padStart", Native::StringPadStart, 1.0),
    ("repeat", Native::StringRepeat, 1.0),
    ("replace", Native::StringReplace, 2.0),
    ("replaceAll", Native::StringReplaceAll, 2.0),
    ("search", Native::StringSearch, 1.0),
    ("slice", Native::StringSlice, 2.0),
    ("split", Native::StringSplit, 2.0),
    ("startsWith", Native::StringStartsWith, 1.0),
    ("substr", Native::StringSubstr, 2.0),
    ("substring", Native::StringSubstring, 2.0),
    ("toLocaleLowerCase", Native::StringToLocaleLowerCase, 0.0),
    ("toLocaleUpperCase", Native::StringToLocaleUpperCase, 0.0),
    ("toLowerCase", Native::StringToLowerCase, 0.0),
    ("toString", Native::StringToString, 0.0),
    ("toWellFormed", Native::StringToWellFormed, 0.0),
    ("toUpperCase", Native::StringToUpperCase, 0.0),
    ("trim", Native::StringTrim, 0.0),
    ("trimEnd", Native::StringTrimEnd, 0.0),
    ("trimStart", Native::StringTrimStart, 0.0),
    ("valueOf", Native::StringValueOf, 0.0),
];

struct StringHtmlMethod {
    name: &'static str,
    native: Native,
    tag: &'static str,
    attribute: Option<&'static str>,
}

const STRING_HTML_METHODS: &[StringHtmlMethod] = &[
    StringHtmlMethod { name: "anchor", native: Native::StringAnchor, tag: "a", attribute: Some("name") },
    StringHtmlMethod { name: "big", native: Native::StringBig, tag: "big", attribute: None },
    StringHtmlMethod { name: "blink", native: Native::StringBlink, tag: "blink", attribute: None },
    StringHtmlMethod { name: "bold", native: Native::StringBold, tag: "b", attribute: None },
    StringHtmlMethod { name: "fixed", native: Native::StringFixed, tag: "tt", attribute: None },
    StringHtmlMethod { name: "fontcolor", native: Native::StringFontcolor, tag: "font", attribute: Some("color") },
    StringHtmlMethod { name: "fontsize", native: Native::StringFontsize, tag: "font", attribute: Some("size") },
    StringHtmlMethod { name: "italics", native: Native::StringItalics, tag: "i", attribute: None },
    StringHtmlMethod { name: "link", native: Native::StringLink, tag: "a", attribute: Some("href") },
    StringHtmlMethod { name: "small", native: Native::StringSmall, tag: "small", attribute: None },
    StringHtmlMethod { name: "strike", native: Native::StringStrike, tag: "strike", attribute: None },
    StringHtmlMethod { name: "sub", native: Native::StringSub, tag: "sub", attribute: None },
    StringHtmlMethod { name: "sup", native: Native::StringSup, tag: "sup", attribute: None },
];

const STRING_METHOD_ALIASES: &[(&str, &str)] = &[("trimLeft", "trimStart"), ("trimRight", "trimEnd")];
const HTML_ATTRIBUTE_QUOTE: u16 = b'"' as u16;
const HTML_QUOTE_ENTITY: &str = "&quot;";

pub(super) fn string_native_length(native: Native) -> Option<f64> {
    STRING_METHODS
        .iter()
        .find_map(|(_, candidate, length)| (*candidate == native).then_some(*length))
        .or_else(|| {
            STRING_HTML_METHODS.iter().find_map(|method| {
                (method.native == native)
                    .then_some(if method.attribute.is_some() { 1.0 } else { 0.0 })
            })
        })
}

fn string_html_method(native: Native) -> Option<&'static StringHtmlMethod> {
    STRING_HTML_METHODS
        .iter()
        .find(|method| method.native == native)
}

fn utf16_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index].encode_utf16().count()
}

pub(super) fn rfind_utf16(text: &[u16], search: &[u16], position: usize) -> Option<usize> {
    if search.is_empty() {
        return Some(position.min(text.len()));
    }
        (search.len() <= text.len())
            .then_some(position.min(text.len() - search.len()))
            .into_iter()
            .flat_map(|end| 0..=end)
            .rev()
            .find(|index| text[*index..*index + search.len()] == *search)
}

impl<H: Host> Vm<H> {
    pub(super) fn string_method_receiver(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if matches!(
            native,
            Native::StringMatch
                | Native::StringMatchAll
                | Native::StringReplace
                | Native::StringReplaceAll
                | Native::StringSearch
                | Native::StringSplit
        ) {
            self.require_object_coercible(p, receiver)?;
            return Ok(receiver);
        }
        let converts_receiver = STRING_METHODS.iter().any(|(_, method, _)| {
            *method == native && !matches!(native, Native::StringToString | Native::StringValueOf)
        }) || string_html_method(native).is_some();
        if !converts_receiver {
            return Ok(receiver);
        }
        self.require_object_coercible(p, receiver)?;
        if matches!(self.heap.get(receiver), Some(Cell::String(_))) {
            return Ok(receiver);
        }
        let text = self.to_string(p, receiver)?;
        Ok(self.heap.alloc(Cell::String(text.into())))
    }

    pub(super) fn install_string(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
    ) -> Result<(), JsError> {
        self.string_proto = self.install_string_prototype(
            program,
            constructor,
            self.object_proto,
            self.realm.globals,
        )?;
        let iterator = self.native_value(Native::StringValues);
        self.set_builtin_function_name(iterator, "[Symbol.iterator]")?;
        Ok(())
    }

    pub(super) fn install_string_for_realm(
        &mut self,
        program: &ResidualProgram,
        global: Value,
        object_proto: Value,
    ) -> Result<(), JsError> {
        let string = self.native_with_realm(Native::String, global, global);
        self.set_builtin_function_name(string, "String")?;
        self.install_string_prototype(program, string, object_proto, global)?;
        self.set_builtin_value_named(global, "String", string)?;
        self.set_builtin_named_for_realm(program, string, "fromCharCode", Native::StringFromCharCode, global)?;
        self.set_builtin_named_for_realm(program, string, "fromCodePoint", Native::StringFromCodePoint, global)?;
        self.set_builtin_named_for_realm(program, string, "raw", Native::StringRaw, global)
    }

    fn install_string_prototype(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        object_proto: Value,
        realm: Value,
    ) -> Result<Value, JsError> {
        let empty = self.heap.alloc(Cell::String(JsString::from_str("")));
        let prototype = self.heap.alloc(Cell::Object(Self::empty_object(object_proto)));
        let value_atom = self.intern_atom("\0rqj:string-value");
        self.set_property(prototype, value_atom, empty)?;
        self.set_named_constant(program, prototype, "length", Value::number(0.0))?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.intern_atom("prototype");
        self.set_property_attributes(
            constructor,
            super::property_key::PropertyKey::string(prototype_atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        for (name, native, _) in STRING_METHODS {
            self.set_builtin_named_for_realm(program, prototype, name, *native, realm)?;
        }
        for method in STRING_HTML_METHODS {
            self.set_builtin_named_for_realm(program, prototype, method.name, method.native, realm)?;
        }
        for (alias, original) in STRING_METHOD_ALIASES {
            let original = self.intern_atom(original);
            let function = self
                .own_property(prototype, original)
                .expect("String method aliases have installed targets");
            self.set_builtin_value_named(prototype, alias, function)?;
        }
        let iterator = self.native_with_realm(Native::StringValues, realm, realm);
        self.set_builtin_function_name(iterator, "[Symbol.iterator]")?;
        let symbol = self.well_known_symbols["iterator"];
        self.set_symbol_property(prototype, symbol, iterator)?;
        Ok(prototype)
    }

    pub(super) fn string_html_method(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        args: &[Value],
    ) -> Option<Result<Value, JsError>> {
        let method = string_html_method(native)?;
        Some((|| {
            let receiver = self.coerce_js_string(p, receiver)?;
            let mut units = Vec::with_capacity(receiver.units().len());
            let opening = match method.attribute {
                Some(attribute) => format!("<{} {}=\"", method.tag, attribute),
                None => format!("<{}>", method.tag),
            };
            units.extend(opening.encode_utf16());
            if method.attribute.is_some() {
                let argument = self.coerce_js_string(
                    p,
                    args.first().copied().unwrap_or(Value::UNDEFINED),
                )?;
                for &unit in argument.units() {
                    if unit == HTML_ATTRIBUTE_QUOTE {
                        units.extend(HTML_QUOTE_ENTITY.encode_utf16());
                    } else {
                        units.push(unit);
                    }
                }
                units.extend("\">".encode_utf16());
            }
            units.extend(receiver.units());
            units.extend(format!("</{}>", method.tag).encode_utf16());
            Ok(self.heap.alloc(Cell::String(JsString::from_units(&units))))
        })())
    }

    fn set_builtin_named_for_realm(
        &mut self,
        _program: &ResidualProgram,
        object: Value,
        name: &str,
        native: Native,
        realm: Value,
    ) -> Result<(), JsError> {
        let function = self.native_with_realm(native, realm, realm);
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    pub(super) fn string_raw(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let template = args.first().copied().unwrap_or(Value::UNDEFINED);
        self.require_object_coercible(p, template)?;
        let template = if self.is_object_like(template) {
            template
        } else {
            self.box_primitive_object(template)?
        };
        let raw_key = self.intern_atom("raw");
        let raw = self.get_property(p, template, raw_key)?;
        self.require_object_coercible(p, raw)?;
        let raw = if self.is_object_like(raw) {
            raw
        } else {
            self.box_primitive_object(raw)?
        };
        let length_key = self.intern_atom("length");
        let raw_length = self.get_property(p, raw, length_key)?;
        let length = super::regexp::regexp_to_length(self.to_number(p, raw_length)?);
        if length == 0 {
            return self.string_from_units(&[]);
        }
        let mut result = JsString::from_str("");
        for index in 0..length {
            let key = self.intern_atom(&index.to_string());
            let segment = self.get_property(p, raw, key)?;
            let segment = self.coerce_js_string(p, segment)?;
            result.push_js_string(&segment);
            if index + 1 < length {
                let substitution = args
                    .get(index + 1)
                    .copied()
                    .unwrap_or_else(|| self.heap.alloc(Cell::String(JsString::from_str(""))));
                let substitution = self.coerce_js_string(p, substitution)?;
                result.push_js_string(&substitution);
            }
        }
        Ok(self.heap.alloc(Cell::String(result)))
    }

    pub(super) fn string_basic_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let Some(Cell::String(receiver)) = self.heap.get(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        match native {
            Native::StringAt | Native::StringCodePointAt => {
                let units = receiver.units().to_vec();
                let raw = self
                    .to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?
                    .trunc() as i64;
                let index = if native == Native::StringAt && raw < 0 {
                    units.len() as i64 + raw
                } else {
                    raw
                };
                let Some(index) = usize::try_from(index)
                    .ok()
                    .filter(|index| *index < units.len())
                else {
                    return Ok(Value::UNDEFINED);
                };
                if native == Native::StringAt {
                    return self.string_from_units(&units[index..index + 1]);
                }
                let first = units[index];
                let code_point = units
                    .get(index + 1)
                    .and_then(|low| crate::unicode::decode_surrogate_pair(first, *low))
                    .unwrap_or_else(|| u32::from(first));
                Ok(Value::number(code_point as f64))
            }
            Native::StringToUpperCase
            | Native::StringToLowerCase
            | Native::StringToLocaleUpperCase
            | Native::StringToLocaleLowerCase => {
                let upper = matches!(
                    native,
                    Native::StringToUpperCase | Native::StringToLocaleUpperCase
                );
                let text = if matches!(
                    native,
                    Native::StringToLocaleUpperCase | Native::StringToLocaleLowerCase
                ) {
                    let locale = self
                        .collator_locale_list(p, args.first().copied())?
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| "en-US".into());
                    quench_intl::locale_case(receiver.host_string(), &locale, upper)
                } else if upper {
                    receiver.host_string().to_uppercase()
                } else {
                    receiver.host_string().to_lowercase()
                };
                Ok(self.heap.alloc(Cell::String(text.into())))
            }
            Native::StringLocaleCompare => {
                let other = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                self.collator_locale_list(p, args.get(1).copied())?;
                let collator_args = [
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                    args.get(2).copied().unwrap_or(Value::UNDEFINED),
                ];
                let constructor = self.realm.intrinsics.intl_collator_constructors
                    .get(&self.realm.globals)
                    .copied()
                    .ok_or_else(|| JsError("Intl.Collator intrinsic is not installed".into()))?;
                let collator = self.intl_collator_construct(p, &collator_args, constructor)?;
                let left = self.heap.alloc(Cell::String(receiver));
                let right = self.heap.alloc(Cell::String(other.into()));
                self.intl_collator_native(
                    p,
                    Native::IntlCollatorCompare,
                    collator,
                    &[left, right],
                )
            }
            Native::StringConcat => {
                let mut text = receiver;
                for value in args {
                    let value = self.coerce_js_string(p, *value)?;
                    text.push_js_string(&value);
                }
                Ok(self.heap.alloc(Cell::String(text)))
            }
            Native::StringNormalize => {
                use unicode_normalization::UnicodeNormalization;
                let form = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let text = match form.as_str() {
                    "NFC" | "undefined" => receiver.host_string().nfc().collect(),
                    "NFD" => receiver.host_string().nfd().collect(),
                    "NFKC" => receiver.host_string().nfkc().collect(),
                    "NFKD" => receiver.host_string().nfkd().collect(),
                    _ => return Err(self.range_error(p, "invalid normalization form".into())),
                };
                Ok(self.heap.alloc(Cell::String(text)))
            }
            _ => unreachable!(),
        }
    }

    pub(super) fn string_iterator_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
    ) -> Result<Value, JsError> {
        self.require_object_coercible(p, this)?;
        let source = match self.heap.get(this) {
            Some(Cell::String(_)) => this,
            _ => {
                let text = self.to_string(p, this)?;
                self.heap.alloc(Cell::String(text.into()))
            }
        };
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.string_iterator_proto),
            source,
            next_method: None,
            helper: None,
            helper_running: false,
            helper_started: false,
            kind: IteratorKind::String,
            index: 0,
            done: false,
            generator: None,
        }))
    }

    pub(super) fn string_split_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if this.is_null() || this.is_undefined() {
            return Err(self.type_error(p, "String.prototype.split called on nullish value".into()));
        }
        let separator = args.first().copied().unwrap_or(Value::UNDEFINED);
        let limit = args.get(1).copied();
        if self.is_object_like(separator) {
            let symbol = self
                .well_known_symbols
                .get("split")
                .copied()
                .ok_or_else(|| self.type_error(p, "RegExp split symbol is unavailable".into()))?;
            let method = self.get_index(p, separator, symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "String split method is not callable".into()));
                }
                return self.call_value(
                    p,
                    method,
                    separator,
                    &[this, limit.unwrap_or(Value::UNDEFINED)],
                );
            }
        }

        let input = self.regexp_input_string(p, this)?;
        let limit = self.regexp_split_limit(p, limit)?;
        let parts = if separator.is_undefined() {
            vec![input]
        } else {
            let separator = self.regexp_input_string(p, separator)?;
            input.split_units(separator.units())
        };
        if limit == 0 {
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(Vec::new()),
            }));
        }
        let values = parts
            .into_iter()
            .take(limit)
            .map(|part| self.heap.alloc(Cell::String(part)))
            .collect();
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    pub(super) fn string_match_or_search_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let name = match native {
            Native::StringMatch => "match",
            Native::StringSearch => "search",
            _ => unreachable!("match/search dispatch owns only its two methods"),
        };
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            vm.require_object_coercible(p, this)?;
            let pattern = args.first().copied().unwrap_or(Value::UNDEFINED);
            let symbol = vm
                .well_known_symbols
                .get(name)
                .copied()
                .ok_or_else(|| vm.type_error(p, "RegExp method symbol is unavailable".into()))?;
            if vm.is_object_like(pattern) {
                let method = vm.get_index(p, pattern, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "RegExp method is not callable".into()));
                    }
                    return vm.call_value(p, method, pattern, &[this]);
                }
            }
            let input = vm.regexp_input_string(p, this)?;
            let input = vm.heap.alloc(Cell::String(input));
            vm.with_call_roots([input], |vm| {
                let matcher = vm.regexp_create(p, pattern, Value::UNDEFINED)?;
                vm.with_call_roots([matcher], |vm| {
                    let method = vm.get_index(p, matcher, symbol)?;
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "RegExp method is not callable".into()));
                    }
                    vm.call_value(p, method, matcher, &[input])
                })
            })
        })
    }

    pub(super) fn string_match_all_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if this.is_null() || this.is_undefined() {
            return Err(self.type_error(
                p,
                "String.prototype.matchAll called on nullish value".into(),
            ));
        }
        let pattern = args.first().copied().unwrap_or(Value::UNDEFINED);
        if self.regexp_is_regexp(p, pattern)? {
            let flags_atom = self.intern_atom("flags");
            let flags_value = self.get_property(p, pattern, flags_atom)?;
            let flags = self.to_string(p, flags_value)?;
            if !flags.contains('g') {
                return Err(self.type_error(
                    p,
                    "String.prototype.matchAll requires a global RegExp".into(),
                ));
            }
        }
        let symbol = self
            .well_known_symbols
            .get("matchAll")
            .copied()
            .ok_or_else(|| self.type_error(p, "RegExp matchAll symbol is unavailable".into()))?;
        if !self.is_object_like(pattern) {
            let regexp_atom = self.intern_atom("RegExp");
            let constructor = self.get_property(p, self.realm.globals, regexp_atom)?;
            let global_flag = self.heap.alloc(Cell::String("g".into()));
            let matcher = self.construct_value(p, constructor, &[pattern, global_flag])?;
            let method = self.get_index(p, matcher, symbol)?;
            if !self.is_function(method) {
                return Err(self.type_error(p, "RegExp @@matchAll is not callable".into()));
            }
            let input = self.regexp_input_string(p, this)?;
            let pattern_string = self.heap.alloc(Cell::String(input));
            return self.call_value(p, method, matcher, &[pattern_string]);
        }
        let method = self.get_index(p, pattern, symbol)?;
        let method = if method.is_undefined() || method.is_null() {
            let regexp_atom = self.intern_atom("RegExp");
            let constructor = self.get_property(p, self.realm.globals, regexp_atom)?;
            let global_flag = self.heap.alloc(Cell::String("g".into()));
            let matcher = self.construct_value(p, constructor, &[pattern, global_flag])?;
            let method = self.get_index(p, matcher, symbol)?;
            if !self.is_function(method) {
                return Err(self.type_error(p, "RegExp @@matchAll is not callable".into()));
            }
            let input = self.regexp_input_string(p, this)?;
            let pattern_string = self.heap.alloc(Cell::String(input));
            return self.call_value(p, method, matcher, &[pattern_string]);
        } else {
            method
        };
        if !self.is_function(method) {
            return Err(self.type_error(p, "@@matchAll is not callable".into()));
        }
        let input = self.regexp_input_string(p, this)?;
        let input = self.heap.alloc(Cell::String(input));
        self.call_value(p, method, pattern, &[input])
    }

    pub(super) fn string_replace_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        replace_all: bool,
    ) -> Result<Value, JsError> {
        let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let replacement_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        if replace_all && !search_value.is_null() && !search_value.is_undefined() {
            let is_regexp = self.regexp_is_regexp(p, search_value)?;
            if is_regexp {
                let flags_atom = self.intern_atom("flags");
                let flags = self.get_property(p, search_value, flags_atom)?;
                self.require_object_coercible(p, flags)?;
                let flags = self.to_string(p, flags)?;
                if !flags.contains('g') {
                    return Err(self.type_error(
                        p,
                        "String.prototype.replaceAll requires a global RegExp".into(),
                    ));
                }
            }
        }
        if self.is_object_like(search_value) {
            let symbol = self
                .well_known_symbols
                .get("replace")
                .copied()
                .ok_or_else(|| self.type_error(p, "RegExp replace symbol is unavailable".into()))?;
            let method = self.get_index(p, search_value, symbol)?;
            if !method.is_undefined() && !method.is_null() {
                if !self.is_function(method) {
                    return Err(self.type_error(p, "String replace method is not callable".into()));
                }
                return self.call_value(p, method, search_value, &[this, replacement_value]);
            }
        }
        let receiver = self.regexp_input_string(p, this)?;
        let receiver_host = receiver.host_string();
        let search = self.to_string(p, search_value)?;
        let replacement_function = matches!(
            self.heap.get(replacement_value),
            Some(Cell::Function { .. })
        );
        let replacement = if replacement_function {
            String::new()
        } else {
            self.to_string(p, replacement_value)?
        };
        if replace_all && search.is_empty() {
            let input = self.heap.alloc(Cell::String(receiver.clone()));
            let units = receiver.units().to_vec();
            let mut output = Vec::new();
            for (offset, unit) in units.iter().copied().enumerate() {
                let text = if replacement_function {
                    let callback_args = [
                        self.heap.alloc(Cell::String(String::new().into())),
                        Value::number(offset as f64),
                        input,
                    ];
                    let value =
                        self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                    self.to_string(p, value)?
                } else {
                    replacement.clone()
                };
                output.extend(text.encode_utf16());
                output.push(unit);
            }
            let text = if replacement_function {
                let callback_args = [
                    self.heap.alloc(Cell::String(String::new().into())),
                    Value::number(units.len() as f64),
                    input,
                ];
                let value =
                    self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                self.to_string(p, value)?
            } else {
                replacement
            };
            output.extend(text.encode_utf16());
            return self.string_from_units(&output);
        }
        let Some(index) = receiver_host.find(&search) else {
            return Ok(self.heap.alloc(Cell::String(receiver)));
        };
        if replace_all && !search.is_empty() {
            let mut result = String::with_capacity(receiver_host.len());
            let mut cursor = 0;
            for (index, _) in receiver_host.match_indices(&search) {
                result.push_str(&receiver_host[cursor..index]);
                let replacement_text = if replacement_function {
                    let callback_args = [
                        self.heap.alloc(Cell::String(search.clone().into())),
                        Value::number(utf16_index(receiver_host, index) as f64),
                        self.heap.alloc(Cell::String(receiver.clone())),
                    ];
                    let value =
                        self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
                    self.to_string(p, value)?
                } else {
                    expand_replacement(
                        &replacement,
                        &search,
                        &[],
                        receiver_host,
                        index,
                        index + search.len(),
                        &[],
                    )
                };
                result.push_str(&replacement_text);
                cursor = index + search.len();
            }
            result.push_str(&receiver_host[cursor..]);
            return Ok(self.heap.alloc(Cell::String(result.into())));
        }
        let replacement = if replacement_function {
            let callback_args = [
                self.heap.alloc(Cell::String(search.clone().into())),
                Value::number(utf16_index(receiver_host, index) as f64),
                self.heap.alloc(Cell::String(receiver.clone())),
            ];
            let value = self.call_value(p, replacement_value, Value::UNDEFINED, &callback_args)?;
            self.to_string(p, value)?
        } else {
            expand_replacement(
                &replacement,
                &search,
                &[],
                receiver_host,
                index,
                index + search.len(),
                &[],
            )
        };
        let mut result = String::with_capacity(
            receiver_host.len() + replacement.len().saturating_sub(search.len()),
        );
        result.push_str(&receiver_host[..index]);
        result.push_str(&replacement);
        result.push_str(&receiver_host[index + search.len()..]);
        Ok(self.heap.alloc(Cell::String(result.into())))
    }
}

fn expand_replacement(
    template: &str,
    whole: &str,
    captures: &[Option<&str>],
    input: &str,
    start: usize,
    end: usize,
    named_captures: &[(String, Option<std::ops::Range<usize>>)],
) -> String {
    let chars = template.chars().collect::<Vec<_>>();
    let mut output = String::with_capacity(template.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' || index + 1 >= chars.len() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let next = chars[index + 1];
        match next {
            '$' => {
                output.push('$');
                index += 2;
            }
            '&' => {
                output.push_str(whole);
                index += 2;
            }
            '`' => {
                output.push_str(&input[..start]);
                index += 2;
            }
            '\'' => {
                output.push_str(&input[end..]);
                index += 2;
            }
            '<' if !named_captures.is_empty() => {
                let name_start = index + 2;
                if let Some(close) = chars[name_start..]
                    .iter()
                    .position(|character| *character == '>')
                {
                    let name = chars[name_start..name_start + close]
                        .iter()
                        .collect::<String>();
                    if let Some((_, Some(range))) =
                        named_captures.iter().find(|(group, _)| group == &name)
                    {
                        output.push_str(&input[range.clone()]);
                    }
                    index = name_start + close + 1;
                } else {
                    output.push('$');
                    index += 1;
                }
            }
            '0'..='9' if next != '0' => {
                let first = next.to_digit(10).unwrap() as usize;
                let mut consumed = 1;
                let mut capture_index = first;
                if index + 2 < chars.len()
                    && chars[index + 2].is_ascii_digit()
                    && first * 10 + chars[index + 2].to_digit(10).unwrap() as usize
                        <= captures.len()
                {
                    capture_index = first * 10 + chars[index + 2].to_digit(10).unwrap() as usize;
                    consumed = 2;
                }
                if capture_index <= captures.len() {
                    output.push_str(captures[capture_index - 1].unwrap_or(""));
                    index += consumed + 1;
                } else {
                    output.push('$');
                    index += 1;
                }
            }
            _ => {
                output.push('$');
                index += 1;
            }
        }
    }
    output
}
