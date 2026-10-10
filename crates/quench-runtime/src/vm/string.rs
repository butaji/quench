use super::*;

enum StringReplacement {
    Callable(Value),
    Template(JsString),
}

const MATCH_ALL_FLAGS: &str = "g";

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
    StringHtmlMethod {
        name: "anchor",
        native: Native::StringAnchor,
        tag: "a",
        attribute: Some("name"),
    },
    StringHtmlMethod {
        name: "big",
        native: Native::StringBig,
        tag: "big",
        attribute: None,
    },
    StringHtmlMethod {
        name: "blink",
        native: Native::StringBlink,
        tag: "blink",
        attribute: None,
    },
    StringHtmlMethod {
        name: "bold",
        native: Native::StringBold,
        tag: "b",
        attribute: None,
    },
    StringHtmlMethod {
        name: "fixed",
        native: Native::StringFixed,
        tag: "tt",
        attribute: None,
    },
    StringHtmlMethod {
        name: "fontcolor",
        native: Native::StringFontcolor,
        tag: "font",
        attribute: Some("color"),
    },
    StringHtmlMethod {
        name: "fontsize",
        native: Native::StringFontsize,
        tag: "font",
        attribute: Some("size"),
    },
    StringHtmlMethod {
        name: "italics",
        native: Native::StringItalics,
        tag: "i",
        attribute: None,
    },
    StringHtmlMethod {
        name: "link",
        native: Native::StringLink,
        tag: "a",
        attribute: Some("href"),
    },
    StringHtmlMethod {
        name: "small",
        native: Native::StringSmall,
        tag: "small",
        attribute: None,
    },
    StringHtmlMethod {
        name: "strike",
        native: Native::StringStrike,
        tag: "strike",
        attribute: None,
    },
    StringHtmlMethod {
        name: "sub",
        native: Native::StringSub,
        tag: "sub",
        attribute: None,
    },
    StringHtmlMethod {
        name: "sup",
        native: Native::StringSup,
        tag: "sup",
        attribute: None,
    },
];

const STRING_METHOD_ALIASES: &[(&str, &str)] =
    &[("trimLeft", "trimStart"), ("trimRight", "trimEnd")];
const HTML_ATTRIBUTE_QUOTE: u16 = b'"' as u16;
const HTML_QUOTE_ENTITY: &str = "&quot;";

pub(super) fn string_native_length(native: Native) -> Option<f64> {
    STRING_METHODS
        .iter()
        .find_map(|(_, candidate, length)| (*candidate == native).then_some(*length))
        .or_else(|| {
            STRING_HTML_METHODS.iter().find_map(|method| {
                (method.native == native).then_some(if method.attribute.is_some() {
                    1.0
                } else {
                    0.0
                })
            })
        })
}

fn string_html_method(native: Native) -> Option<&'static StringHtmlMethod> {
    STRING_HTML_METHODS
        .iter()
        .find(|method| method.native == native)
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
    pub(super) fn string_build_result(
        &mut self,
        p: &ResidualProgram,
        result: Result<JsString, super::wtf16::StringBuildError>,
    ) -> Result<JsString, JsError> {
        result.map_err(|error| {
            self.range_error(
                p,
                match error {
                    super::wtf16::StringBuildError::InvalidLength => "Invalid string length",
                    super::wtf16::StringBuildError::Allocation => "String allocation failed",
                }
                .into(),
            )
        })
    }

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
        let text = self.coerce_js_string(p, receiver)?;
        Ok(self.heap.alloc(Cell::String(text)))
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
        self.set_builtin_named_for_realm(
            program,
            string,
            "fromCharCode",
            Native::StringFromCharCode,
            global,
        )?;
        self.set_builtin_named_for_realm(
            program,
            string,
            "fromCodePoint",
            Native::StringFromCodePoint,
            global,
        )?;
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
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_proto)));
        let value_atom = self.intern_atom("\0quench:string-value");
        self.set_property(prototype, value_atom, empty)?;
        self.set_named_constant(program, prototype, "length", Value::number(0.0))?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
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
            self.set_builtin_named_for_realm(
                program,
                prototype,
                method.name,
                method.native,
                realm,
            )?;
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
                let argument =
                    self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
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
        self.with_call_roots(args.iter().copied(), |vm| {
            let template = args.first().copied().unwrap_or(Value::UNDEFINED);
            vm.require_object_coercible(p, template)?;
            let template = if vm.is_object_like(template) {
                template
            } else {
                vm.box_primitive_object(template)?
            };
            let atom = vm.intern_atom("raw");
            let raw = vm.get_property(p, template, atom)?;
            vm.require_object_coercible(p, raw)?;
            let raw = if vm.is_object_like(raw) {
                raw
            } else {
                vm.box_primitive_object(raw)?
            };
            vm.with_call_roots([raw], |vm| {
                let atom = vm.intern_atom("length");
                let length = vm.get_property(p, raw, atom)?;
                let length = vm.regexp_to_length_value(p, length)?;
                let mut output = JsString::from_str("");
                for index in 0..length {
                    let atom = vm.intern_atom(&index.to_string());
                    let segment = vm.get_property(p, raw, atom)?;
                    let segment = vm.coerce_js_string(p, segment)?;
                    output.push_js_string(&segment);
                    if index + 1 < length {
                        if let Some(substitution) = args.get(index + 1) {
                            let substitution = vm.coerce_js_string(p, *substitution)?;
                            output.push_js_string(&substitution);
                        }
                    }
                }
                Ok(vm.heap.alloc(Cell::String(output)))
            })
        })
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
                        .canonical_locale_list(p, args.first().copied())?
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
                let collator_args = [
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                    args.get(2).copied().unwrap_or(Value::UNDEFINED),
                ];
                let constructor = self
                    .realm
                    .intrinsics
                    .intl_collator_constructors
                    .get(&self.realm.globals)
                    .copied()
                    .ok_or_else(|| JsError("Intl.Collator intrinsic is not installed".into()))?;
                let collator = self.intl_collator_construct(p, &collator_args, constructor)?;
                let left = self.heap.alloc(Cell::String(receiver));
                let right = self.heap.alloc(Cell::String(other.into()));
                self.intl_collator_native(p, Native::IntlCollatorCompare, collator, &[left, right])
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
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            vm.require_object_coercible(p, this)?;
            let separator = args.first().copied().unwrap_or(Value::UNDEFINED);
            let limit = args.get(1).copied();
            if vm.is_object_like(separator) {
                let symbol =
                    vm.well_known_symbols.get("split").copied().ok_or_else(|| {
                        vm.type_error(p, "RegExp split symbol is unavailable".into())
                    })?;
                let method = vm.get_index(p, separator, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "String split method is not callable".into()));
                    }
                    return vm.call_value(
                        p,
                        method,
                        separator,
                        &[this, limit.unwrap_or(Value::UNDEFINED)],
                    );
                }
            }

            let input = vm.coerce_js_string(p, this)?;
            let limit = vm.regexp_split_limit(p, limit)?;
            let separator_string = vm.coerce_js_string(p, separator)?;
            let parts = if limit == 0 {
                Vec::new()
            } else if separator.is_undefined() {
                vec![input]
            } else {
                input.split_units(separator_string.units())
            };
            let values = parts
                .into_iter()
                .take(limit)
                .map(|part| vm.heap.alloc(Cell::String(part)))
                .collect();
            Ok(vm.heap.alloc(Cell::Array {
                object: Self::empty_object(vm.array_proto),
                elements: Rc::new(values),
            }))
        })
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
            let symbol =
                vm.well_known_symbols.get(name).copied().ok_or_else(|| {
                    vm.type_error(p, "RegExp method symbol is unavailable".into())
                })?;
            if vm.is_object_like(pattern) {
                let method = vm.get_index(p, pattern, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "RegExp method is not callable".into()));
                    }
                    return vm.call_value(p, method, pattern, &[this]);
                }
            }
            vm.string_regexp_fallback(p, this, pattern, symbol, Value::UNDEFINED)
        })
    }

    pub(super) fn string_match_all_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            vm.require_object_coercible(p, this)?;
            let pattern = args.first().copied().unwrap_or(Value::UNDEFINED);
            let symbol = vm
                .well_known_symbols
                .get("matchAll")
                .copied()
                .ok_or_else(|| vm.type_error(p, "RegExp matchAll symbol is unavailable".into()))?;
            if vm.is_object_like(pattern) {
                if vm.regexp_is_regexp(p, pattern)? {
                    let atom = vm.intern_atom("flags");
                    let flags = vm.get_property(p, pattern, atom)?;
                    vm.require_object_coercible(p, flags)?;
                    let flags = vm.coerce_js_string(p, flags)?;
                    if !flags.host_string().contains(MATCH_ALL_FLAGS) {
                        return Err(vm.type_error(
                            p,
                            "String.prototype.matchAll requires a global RegExp".into(),
                        ));
                    }
                }
                let method = vm.get_index(p, pattern, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "@@matchAll is not callable".into()));
                    }
                    return vm.call_value(p, method, pattern, &[this]);
                }
            }
            let flags = vm.heap.alloc(Cell::String(MATCH_ALL_FLAGS.into()));
            vm.string_regexp_fallback(p, this, pattern, symbol, flags)
        })
    }

    fn string_regexp_fallback(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        pattern: Value,
        symbol: Value,
        flags: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([receiver, pattern, flags], |vm| {
            let input = vm.coerce_js_string(p, receiver)?;
            let input = vm.heap.alloc(Cell::String(input));
            vm.with_call_roots([input], |vm| {
                let matcher = vm.regexp_create(p, pattern, flags)?;
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

    pub(super) fn string_replace_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        replace_all: bool,
    ) -> Result<Value, JsError> {
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            vm.require_object_coercible(p, this)?;
            let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
            let replacement_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            if vm.is_object_like(search_value) {
                if replace_all && vm.regexp_is_regexp(p, search_value)? {
                    let atom = vm.intern_atom("flags");
                    let flags = vm.get_property(p, search_value, atom)?;
                    vm.require_object_coercible(p, flags)?;
                    let flags = vm.coerce_js_string(p, flags)?;
                    if !flags.host_string().contains(MATCH_ALL_FLAGS) {
                        return Err(vm.type_error(
                            p,
                            "String.prototype.replaceAll requires a global RegExp".into(),
                        ));
                    }
                }
                let symbol = vm
                    .well_known_symbols
                    .get("replace")
                    .copied()
                    .ok_or_else(|| {
                        vm.type_error(p, "RegExp replace symbol is unavailable".into())
                    })?;
                let method = vm.get_index(p, search_value, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(
                            vm.type_error(p, "String replace method is not callable".into())
                        );
                    }
                    return vm.call_value(p, method, search_value, &[this, replacement_value]);
                }
            }
            let input = vm.coerce_js_string(p, this)?;
            let search = vm.coerce_js_string(p, search_value)?;
            let replacement = if vm.is_function(replacement_value) {
                StringReplacement::Callable(replacement_value)
            } else {
                StringReplacement::Template(vm.coerce_js_string(p, replacement_value)?)
            };
            let Some(first) = input.find_units(search.units(), 0) else {
                return Ok(vm.heap.alloc(Cell::String(input)));
            };
            let callback_inputs = match replacement {
                StringReplacement::Callable(_) => Some([
                    vm.heap.alloc(Cell::String(search.clone())),
                    vm.heap.alloc(Cell::String(input.clone())),
                ]),
                StringReplacement::Template(_) => None,
            };
            vm.with_call_roots(callback_inputs.into_iter().flatten(), |vm| {
                let search_length = search.units().len();
                // Empty matches occur at each code-unit boundary. The immutable
                // input/search strings determine positions independently of callbacks.
                let advance = search_length.max(1);
                let mut position = Some(first);
                let mut cursor = 0;
                let mut output = super::wtf16::JsStringBuilder::default();
                while let Some(index) = position {
                    output.append_slice(&input, cursor..index);
                    let text = match &replacement {
                        StringReplacement::Callable(method) => {
                            let [matched, input] =
                                callback_inputs.expect("callable replacement owns guest strings");
                            let value = vm.call_value(
                                p,
                                *method,
                                Value::UNDEFINED,
                                &[matched, Value::number(index as f64), input],
                            )?;
                            vm.coerce_js_string(p, value)?
                        }
                        StringReplacement::Template(template) => vm.replacement_substitution(
                            p,
                            template,
                            &input,
                            index,
                            &search,
                            &[],
                            Value::UNDEFINED,
                        )?,
                    };
                    output.append(&text);
                    cursor = index + search_length;
                    let next = index + advance;
                    position = if replace_all && next <= input.units().len() {
                        input.find_units(search.units(), next)
                    } else {
                        None
                    };
                }
                output.append_slice(&input, cursor..input.units().len());
                let output = vm.string_build_result(p, output.finish())?;
                Ok(vm.heap.alloc(Cell::String(output)))
            })
        })
    }
}
