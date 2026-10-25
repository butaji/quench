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

/// What a native does with its receiver before it runs (`Vm::string_method_receiver`).
#[derive(Clone, Copy)]
enum StringReceiver {
    /// Takes it as it is.
    Kept,
    /// Requires it coercible to an object: the methods that dispatch on a RegExp argument.
    Coercible,
    /// Converts it to a string: the other `String.prototype` methods.
    Converted,
}

/// Each native's receiver rule, by discriminant, derived from the method tables: every native
/// call reads it.
const STRING_RECEIVERS: [StringReceiver; Native::COUNT as usize] = {
    let mut table = [StringReceiver::Kept; Native::COUNT as usize];
    let mut index = 0;
    while index < STRING_METHODS.len() {
        let native = STRING_METHODS[index].1;
        if !matches!(native, Native::StringToString | Native::StringValueOf) {
            table[native as usize] = StringReceiver::Converted;
        }
        index += 1;
    }
    let mut index = 0;
    while index < STRING_HTML_METHODS.len() {
        table[STRING_HTML_METHODS[index].native as usize] = StringReceiver::Converted;
        index += 1;
    }
    let coercible = [
        Native::StringMatch,
        Native::StringMatchAll,
        Native::StringReplace,
        Native::StringReplaceAll,
        Native::StringSearch,
        Native::StringSplit,
    ];
    let mut index = 0;
    while index < coercible.len() {
        table[coercible[index] as usize] = StringReceiver::Coercible;
        index += 1;
    }
    table
};

fn string_html_method(native: Native) -> Option<&'static StringHtmlMethod> {
    STRING_HTML_METHODS
        .iter()
        .find(|method| method.native == native)
}

pub(super) fn rfind_utf16(
    text: super::wtf16::Units<'_>,
    search: super::wtf16::Units<'_>,
    position: usize,
) -> Option<usize> {
    if search.is_empty() {
        return Some(position.min(text.len()));
    }
    (search.len() <= text.len())
        .then_some(position.min(text.len() - search.len()))
        .into_iter()
        .flat_map(|end| 0..=end)
        .rev()
        .find(|index| text.slice(*index..*index + search.len()) == search)
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
        match STRING_RECEIVERS[native as usize] {
            StringReceiver::Kept => return Ok(receiver),
            StringReceiver::Coercible => {
                self.require_object_coercible(p, receiver)?;
                return Ok(receiver);
            }
            StringReceiver::Converted => {}
        }
        self.require_object_coercible(p, receiver)?;
        if self.heap.is_string(receiver) {
            return Ok(receiver);
        }
        let text = self.coerce_js_string(p, receiver)?;
        Ok(self.heap.alloc_string(text))
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
        self.set_builtin_function_name(constructor, "String")?;
        let empty = self.heap.alloc_string(JsString::from_str(""));
        let prototype = self
            .heap
            .alloc(Cell::Object(Self::empty_object(object_proto)));
        self.object_data_mut(prototype)
            .expect("primitive prototype")
            .set_boxed_primitive(empty);
        self.set_named_constant(program, prototype, "length", Value::number(0.0))?;
        self.set_builtin_value_named(prototype, "constructor", constructor)?;
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        self.realm
            .intrinsics
            .builtin_prototypes
            .insert((realm, Native::String), prototype);
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
        let symbol = self.well_known_symbols[WellKnownSymbol::Iterator];
        self.set_symbol_property(prototype, symbol, iterator)?;
        self.set_property_attributes(
            prototype,
            PropertyKey::symbol(symbol),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
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
                for unit in argument.units().iter() {
                    if unit == HTML_ATTRIBUTE_QUOTE {
                        units.extend(HTML_QUOTE_ENTITY.encode_utf16());
                    } else {
                        units.push(unit);
                    }
                }
                units.extend("\">".encode_utf16());
            }
            receiver.units().extend_into(&mut units);
            units.extend(format!("</{}>", method.tag).encode_utf16());
            Ok(self.heap.alloc_string(JsString::from_units(&units)))
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
                Ok(vm.heap.alloc_string(output))
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
        if matches!(native, Native::StringAt | Native::StringCodePointAt) {
            let raw = self
                .to_number(p, args.first().copied().unwrap_or(Value::UNDEFINED))?
                .trunc() as i64;
            // Strings are immutable: the receiver's units are read in place.
            let Some(receiver) = self.heap.string(this) else {
                return Err(JsError("string method receiver is not a string".into()));
            };
            let units = receiver.units();
            let index = if native == Native::StringAt && raw < 0 {
                units.len() as i64 + raw
            } else {
                raw
            };
            let Some((index, first)) = usize::try_from(index)
                .ok()
                .and_then(|index| units.get(index).map(|unit| (index, unit)))
            else {
                return Ok(Value::UNDEFINED);
            };
            if native == Native::StringAt {
                return self.string_from_units(&[first]);
            }
            let code_point = units
                .get(index + 1)
                .and_then(|low| crate::unicode::decode_surrogate_pair(first, low))
                .unwrap_or_else(|| u32::from(first));
            return Ok(Value::number(code_point as f64));
        }
        let Some(receiver) = self.heap.string(this).cloned() else {
            return Err(JsError("string method receiver is not a string".into()));
        };
        match native {
            Native::StringToUpperCase
            | Native::StringToLowerCase
            | Native::StringToLocaleUpperCase
            | Native::StringToLocaleLowerCase => {
                let upper = matches!(
                    native,
                    Native::StringToUpperCase | Native::StringToLocaleUpperCase
                );
                let units = receiver.units();
                // ASCII maps ASCII without locale tailoring; an already-mapped string is its
                // own result, as primitive strings have no identity.
                if matches!(
                    native,
                    Native::StringToUpperCase | Native::StringToLowerCase
                ) && let super::wtf16::Units::Ascii(bytes) = units
                {
                    let mapped = |byte: &u8| {
                        if upper {
                            byte.to_ascii_uppercase()
                        } else {
                            byte.to_ascii_lowercase()
                        }
                    };
                    if bytes.iter().all(|byte| mapped(byte) == *byte) {
                        return Ok(this);
                    }
                    let bytes: Vec<u8> = bytes.iter().map(mapped).collect();
                    let text = std::str::from_utf8(&bytes).expect("ASCII units are UTF-8");
                    return Ok(self.heap.alloc_string(JsString::from_str(text)));
                }
                let text = if matches!(
                    native,
                    Native::StringToLocaleUpperCase | Native::StringToLocaleLowerCase
                ) {
                    let locale = self
                        .canonical_locale_list(p, args.first().copied())?
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| "en-US".into());
                    map_case(units, |text| quench_intl::locale_case(text, &locale, upper))
                } else if upper {
                    map_case(units, str::to_uppercase)
                } else {
                    map_case(units, str::to_lowercase)
                };
                Ok(self.heap.alloc_string(text))
            }
            Native::StringLocaleCompare => {
                let other = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let collation = self.locale_compare_collation(
                    p,
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                    args.get(2).copied().unwrap_or(Value::UNDEFINED),
                )?;
                Ok(Value::number(
                    collation.compare(&receiver.to_string(), &other),
                ))
            }
            Native::StringConcat => {
                let mut text = receiver;
                for value in args {
                    let value = self.coerce_js_string(p, *value)?;
                    text.push_js_string(&value);
                }
                Ok(self.heap.alloc_string(text))
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
                Ok(self.heap.alloc_string(text))
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
            _ if self.heap.is_string(this) => this,
            _ => {
                let text = self.to_string(p, this)?;
                self.heap.alloc_string(text.into())
            }
        };
        Ok(self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(self.string_iterator_proto),
            source,
            kind: IteratorKind::String,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt {
                position_high: 0,
                next_method: None,
                helper: None,
                helper_running: false,
                helper_started: false,
                generator: None,
            },
        }))
    }

    /// `GetMethod(value, symbol)` for the RegExp protocol methods a string method consults,
    /// read from its proven slot when `value` is an ordinary RegExp
    /// (`Vm::regexp_proven_symbol_method`).
    fn string_protocol_method(
        &mut self,
        p: &ResidualProgram,
        value: Value,
        symbol: Value,
    ) -> Result<Value, JsError> {
        if let Some(method) = self.regexp_proven_symbol_method(value, symbol) {
            return Ok(method);
        }
        self.get_index(p, value, symbol)
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
                let symbol = vm
                    .well_known_symbols
                    .get(WellKnownSymbol::Split)
                    .ok_or_else(|| vm.type_error(p, "RegExp split symbol is unavailable".into()))?;
                let method = vm.string_protocol_method(p, separator, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "String split method is not callable".into()));
                    }
                    return vm.call_protocol_method(
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
                .map(|part| vm.heap.alloc_string(part))
                .collect::<Vec<_>>();
            Ok(vm.heap.alloc(Cell::Array {
                object: crate::heap::ArrayObject::new(Self::empty_object(vm.array_proto), values),
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
            let symbol = vm
                .well_known_symbols
                .named(name)
                .ok_or_else(|| vm.type_error(p, "RegExp method symbol is unavailable".into()))?;
            if vm.is_object_like(pattern) {
                let method = vm.string_protocol_method(p, pattern, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "RegExp method is not callable".into()));
                    }
                    return vm.call_protocol_method(p, method, pattern, &[this]);
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
                .get(WellKnownSymbol::MatchAll)
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
                let method = vm.string_protocol_method(p, pattern, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(vm.type_error(p, "@@matchAll is not callable".into()));
                    }
                    return vm.call_protocol_method(p, method, pattern, &[this]);
                }
            }
            let flags = vm.heap.alloc_string(MATCH_ALL_FLAGS.into());
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
            let input = vm.heap.alloc_string(input);
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
                    .get(WellKnownSymbol::Replace)
                    .ok_or_else(|| {
                        vm.type_error(p, "RegExp replace symbol is unavailable".into())
                    })?;
                let method = vm.string_protocol_method(p, search_value, symbol)?;
                if !method.is_undefined() && !method.is_null() {
                    if !vm.is_function(method) {
                        return Err(
                            vm.type_error(p, "String replace method is not callable".into())
                        );
                    }
                    return vm.call_protocol_method(
                        p,
                        method,
                        search_value,
                        &[this, replacement_value],
                    );
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
                return Ok(vm.heap.alloc_string(input));
            };
            let callback_inputs = match replacement {
                StringReplacement::Callable(_) => Some([
                    vm.heap.alloc_string(search.clone()),
                    vm.heap.alloc_string(input.clone()),
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
                Ok(vm.heap.alloc_string(output))
            })
        })
    }
}

/// Applies a full case mapping to each well-formed run of `units`. A lone
/// surrogate has no case mapping and is not cased, so it maps to itself and
/// leaves the surrounding runs' casing contexts unchanged.
fn map_case(units: super::wtf16::Units<'_>, map: impl Fn(&str) -> String) -> JsString {
    let units = match units {
        super::wtf16::Units::Ascii(bytes) => {
            return JsString::from_str(&map(
                std::str::from_utf8(bytes).expect("ASCII units are UTF-8")
            ));
        }
        super::wtf16::Units::Wide(units) => units,
    };
    let mut output = Vec::with_capacity(units.len());
    let flush = |run: &[u16], output: &mut Vec<u16>| {
        let text = String::from_utf16(run).expect("runs between lone surrogates are well formed");
        output.extend(map(&text).encode_utf16());
    };
    let mut start = 0;
    let mut index = 0;
    while let Some(&unit) = units.get(index) {
        let paired = crate::unicode::is_high_surrogate(unit)
            && units
                .get(index + 1)
                .is_some_and(|low| crate::unicode::is_low_surrogate(*low));
        if paired {
            index += 2;
        } else if crate::unicode::is_surrogate(u32::from(unit)) {
            flush(&units[start..index], &mut output);
            output.push(unit);
            index += 1;
            start = index;
        } else {
            index += 1;
        }
    }
    flush(&units[start..], &mut output);
    JsString::from_units(&output)
}
