use super::*;
use crate::staged::string_replace::{self, Piece};
use crate::staged::strings::{ConcreteString, concrete_char_at, concrete_char_code_at};

enum StringReplacement {
    Callable(Value),
    Template(JsString),
}

const MATCH_ALL_FLAGS: &str = "g";

/// `String.prototype`'s methods with their lengths, in installation order: derived from the
/// rows of `native_rows`.
const STRING_METHODS: &[(&str, Native, f64)] = super::native_rows::STRING_METHODS;

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

pub(super) const fn string_native_length(native: Native) -> Option<f64> {
    let mut index = 0;
    while index < STRING_METHODS.len() {
        let (_, candidate, length) = STRING_METHODS[index];
        if candidate as u16 == native as u16 {
            return Some(length);
        }
        index += 1;
    }
    let mut index = 0;
    while index < STRING_HTML_METHODS.len() {
        let method = &STRING_HTML_METHODS[index];
        if method.native as u16 == native as u16 {
            return Some(if method.attribute.is_some() { 1.0 } else { 0.0 });
        }
        index += 1;
    }
    None
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

/// Whether `native` is one of the HTML string methods (`STRING_HTML_METHODS`).
pub(super) fn is_string_html_method(native: Native) -> bool {
    string_html_method(native).is_some()
}

fn string_html_method(native: Native) -> Option<&'static StringHtmlMethod> {
    STRING_HTML_METHODS
        .iter()
        .find(|method| method.native == native)
}

impl Vm {
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
            .alloc_object(Self::empty_object(object_proto));
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
                let atom = vm.length_atom;
                let length = vm.get_property(p, raw, atom)?;
                let length = vm.regexp_to_length_value(p, length)?;
                let mut output = JsString::from_str("");
                for index in 0..length {
                    let atom = vm.index_key_atom(index);
                    let segment = vm.get_property(p, raw, atom)?;
                    let segment = vm.coerce_js_string(p, segment)?;
                    output.push_js_string(&segment);
                    if index + 1 < length
                        && let Some(substitution) = args.get(index + 1) {
                            let substitution = vm.coerce_js_string(p, *substitution)?;
                            output.push_js_string(&substitution);
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
            let from_end = native == Native::StringAt;
            let Some(index) = crate::staged::strings::relative_unit(units, raw, from_end) else {
                return Ok(Value::UNDEFINED);
            };
            if from_end {
                let first = units.at(index);
                return self.string_from_units(&[first]);
            }
            let Some(code_point) = crate::staged::strings::code_point_at(units, index) else {
                return Ok(Value::UNDEFINED);
            };
            return Ok(Value::number(f64::from(code_point)));
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
                // Without locale tailoring, only a capital sigma's lowercase needs the
                // run-wise host mapping; an unchanged string is its own result.
                if matches!(native, Native::StringToUpperCase | Native::StringToLowerCase)
                    && let Some(mapped) = receiver.default_case(upper)
                {
                    return Ok(match mapped {
                        std::borrow::Cow::Borrowed(_) => this,
                        std::borrow::Cow::Owned(text) => self.heap.alloc_string(text),
                    });
                }
                let text = if matches!(
                    native,
                    Native::StringToLocaleUpperCase | Native::StringToLocaleLowerCase
                ) {
                    let locale = layer_hooks::canonical_locale_list(self, p, args.first().copied())?
                        .into_iter()
                        .next()
                        .unwrap_or_else(|| "en-US".into());
                    map_case(receiver.units(), |text| quench_intl::locale_case(text, &locale, upper))
                } else {
                    map_case(receiver.units(), str::to_lowercase)
                };
                Ok(self.heap.alloc_string(text))
            }
            Native::StringLocaleCompare => {
                let other = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let collation = layer_hooks::locale_compare_collation(
                    self,
                    p,
                    args.get(1).copied().unwrap_or(Value::UNDEFINED),
                    args.get(2).copied().unwrap_or(Value::UNDEFINED),
                )?;
                Ok(Value::number(collation.compare(receiver.host_string(), other.host_string())))
            }
            // The receiver then each argument, converted in order: the one StringConcat step of
            // `+` and template literals (`string_concat`).
            Native::StringConcat => self.string_concat(p, 1 + args.len(), |_, part| if part == 0 { this } else { args[part - 1] }),
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
            _ if self.heap.is_string(this)  => this,
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
            ext: crate::heap::IteratorExt::default(),
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

    /// String.prototype.split: the in-place body (`split_in_place`) first, which takes a flat
    /// string at a flat string separator with no limit; any other call runs the general steps
    /// (the protocol lookups, ToUint32 of the limit, `staged::string_split::runs`).
    pub(super) fn string_split_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if self.declined_fast.take() != Some(Native::StringSplit)
            && let Some(parts) = self.split_in_place(Native::StringSplit, this, args)
        {
            return Ok(parts);
        }
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            vm.require_object_coercible(p, this)?;
            let separator = args.first().copied().unwrap_or(Value::UNDEFINED);
            let limit = args.get(1).copied();
            if let Some(delegated) = vm.split_by_protocol(p, this, separator, limit)? {
                return Ok(delegated);
            }
            let input = vm.coerce_js_string(p, this)?;
            let limit = vm.regexp_split_limit(p, limit)?;
            let separator_string = vm.coerce_js_string(p, separator)?;
            // Gathered first (`SplitParts`), so the array's backing is made once at its length.
            let mut parts = super::wtf16::split::SplitParts::<Value>::new();
            if separator.is_undefined() {
                if limit != 0 { parts.push(vm.heap.alloc_string(input)); }
            } else {
                crate::staged::string_split::runs(input.units(), separator_string.units(), limit, |run| parts.push(vm.split_piece(run)));
            }
            let values = crate::heap::ArrayElements::from_slice(&parts);
            Ok(vm.heap.alloc_array(crate::heap::ArrayObject::new(
                    Self::empty_object(vm.array_proto),
                    values,
                )))
        })
    }

    /// The `@@split` protocol of `separator`: its method's result when the separator is an object
    /// with one, `None` for a primitive or a separator without the method (the general steps run).
    fn split_by_protocol(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        separator: Value,
        limit: Option<Value>,
    ) -> Result<Option<Value>, JsError> {
        if !self.is_object_like(separator) {
            return Ok(None);
        }
        let symbol = self
            .well_known_symbols
            .get(WellKnownSymbol::Split)
            .ok_or_else(|| self.type_error(p, "RegExp split symbol is unavailable".into()))?;
        let method = self.string_protocol_method(p, separator, symbol)?;
        if method.is_undefined() || method.is_null() {
            return Ok(None);
        }
        if !self.is_function(method) {
            return Err(self.type_error(p, "String split method is not callable".into()));
        }
        self.call_protocol_method(p, method, separator, &[this, limit.unwrap_or(Value::UNDEFINED)])
            .map(Some)
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
                vm.well_known_symbols.named(name).ok_or_else(|| {
                    vm.type_error(p, "RegExp method symbol is unavailable".into())
                })?;
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

    /// String.prototype.replace and replaceAll: the in-place string-pattern body
    /// (`replace_string_pattern`) first, which borrows the operands; a pattern that is not a flat
    /// string, a function replacement or a rope takes the general steps below.
    pub(super) fn string_replace_native(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
        replace_all: bool,
    ) -> Result<Value, JsError> {
        let native = if replace_all { Native::StringReplaceAll } else { Native::StringReplace };
        if self.declined_fast.take() != Some(native)
            && let Some(replaced) = self.replace_string_pattern(p, native, this, args)
        {
            return replaced;
        }
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            vm.require_object_coercible(p, this)?;
            let search_value = args.first().copied().unwrap_or(Value::UNDEFINED);
            let replacement_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            if let Some(delegated) = vm.replace_by_protocol(p, this, search_value, replacement_value, replace_all)? {
                return Ok(delegated);
            }
            let input = vm.coerce_js_string(p, this)?;
            let search = vm.coerce_js_string(p, search_value)?;
            let replacement = if vm.is_function(replacement_value) {
                StringReplacement::Callable(replacement_value)
            } else {
                StringReplacement::Template(vm.coerce_js_string(p, replacement_value)?)
            };
            // A template replacement is the string-pattern core's (`Vm::replace_units`), the one
            // body every caller shares; it answers `None` when nothing matches.
            let method = match replacement {
                StringReplacement::Template(template) => {
                    let replaced = vm.replace_units(p, &input, &search, &template, replace_all)?;
                    return Ok(replaced.unwrap_or_else(|| vm.heap.alloc_string(input)));
                }
                StringReplacement::Callable(method) => method,
            };
            vm.replace_with_callback(p, &input, &search, method, replace_all)
        })
    }

    /// The `@@replace` protocol of `search_value`: its method's result when it is an object with
    /// one, `None` for a primitive or a pattern without the method. `replaceAll` first requires
    /// a RegExp pattern to be global.
    fn replace_by_protocol(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        search_value: Value,
        replacement_value: Value,
        replace_all: bool,
    ) -> Result<Option<Value>, JsError> {
        if !self.is_object_like(search_value) {
            return Ok(None);
        }
        if replace_all && self.regexp_is_regexp(p, search_value)? {
            self.require_global_regexp(p, search_value)?;
        }
        let symbol = self
            .well_known_symbols
            .get(WellKnownSymbol::Replace)
            .ok_or_else(|| self.type_error(p, "RegExp replace symbol is unavailable".into()))?;
        let method = self.string_protocol_method(p, search_value, symbol)?;
        if method.is_undefined() || method.is_null() {
            return Ok(None);
        }
        if !self.is_function(method) {
            return Err(self.type_error(p, "String replace method is not callable".into()));
        }
        self.call_protocol_method(p, method, search_value, &[this, replacement_value]).map(Some)
    }

    /// `replaceAll`'s check of a RegExp pattern: its `flags` must hold `g`.
    fn require_global_regexp(&mut self, p: &ResidualProgram, regexp: Value) -> Result<(), JsError> {
        let atom = self.intern_atom("flags");
        let flags = self.get_property(p, regexp, atom)?;
        self.require_object_coercible(p, flags)?;
        let flags = self.coerce_js_string(p, flags)?;
        if flags.host_string().contains(MATCH_ALL_FLAGS) {
            return Ok(());
        }
        Err(self.type_error(p, "String.prototype.replaceAll requires a global RegExp".into()))
    }

    /// The replacement of each match of `search` in `input` by `method`'s result (coerced to a
    /// string): every match for `replaceAll`, else the first. The matches are those of
    /// `string_replace::match_starts`, found in the immutable `input` and `search`, which the
    /// callbacks cannot change. A pattern with no match answers `input` itself, unchanged.
    fn replace_with_callback(
        &mut self,
        p: &ResidualProgram,
        input: &JsString,
        search: &JsString,
        method: Value,
        replace_all: bool,
    ) -> Result<Value, JsError> {
        let search_length = search.units().len();
        let mut starts = string_replace::match_starts(input.units(), search.units(), replace_all);
        let Some(first) = starts.next() else {
            return Ok(self.heap.alloc_string(input.clone()));
        };
        let callback_inputs = [self.heap.alloc_string(search.clone()), self.heap.alloc_string(input.clone())];
        self.with_call_roots(callback_inputs, |vm| {
            let [matched, whole] = callback_inputs;
            let mut output = super::wtf16::JsStringBuilder::default();
            string_replace::walk_matches(
                input.units().len(),
                std::iter::once(first).chain(starts),
                |&index| index..index + search_length,
                |piece| -> Result<(), JsError> {
                    match piece {
                        Piece::Gap(gap) => {
                            output.append_slice(input, gap);
                            Ok(())
                        }
                        Piece::Match(index) => {
                            let value = vm.call_value(p, method, Value::UNDEFINED, &[matched, Value::number(index as f64), whole])?;
                            let text = vm.coerce_js_string(p, value)?;
                            output.append(&text);
                            Ok(())
                        }
                    }
                },
            )?;
            let output = vm.string_build_result(p, output.finish())?;
            Ok(vm.heap.alloc_string(output))
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
        // Latin-1 text is mapped as its UTF-16 units; the result is stored in canonical form.
        units => units.wide(),
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

/// The integral position of `charCodeAt` and `charAt` for a Number index: ToIntegerOrInfinity
/// of it (NaN is zero, the rest truncated). `None` for any other value, which the native
/// coerces, since ToNumber may run guest code.
#[inline(always)]
fn string_unit_index(index: Value) -> Option<i64> {
    if let Some(index) = index.as_int() {
        return Some(i64::from(index));
    }
    let number = index.as_number()?;
    Some(if number.is_nan() { 0 } else { number.trunc() as i64 })
}

impl Vm {
    /// The one body of `String.prototype.charCodeAt`: the unit of the string `receiver` at
    /// `index`, NaN outside the string, surrogates raw. `None` where the receiver is no string
    /// or the index is no Number. Interpreter fast paths and the native both call this.
    #[inline(always)]
    pub fn string_char_code_at_core(&mut self, receiver: Value, index: Value) -> Option<Value> {
        let records = self.heap.string_record_base().unwrap_or(std::ptr::null());
        if let Some(code) = concrete_char_code_at(ConcreteString::Record { records, value: receiver }, index) {
            return Some(code);
        }
        // Any other flat text: the same row over its units, the semantics the tiers share.
        if let Some(code) = self.heap.flat_text(receiver).and_then(|text| concrete_char_code_at(ConcreteString::Units(text.units()), index)) {
            return Some(code);
        }
        let position = string_unit_index(index)?;
        let text = self.heap.string(receiver)?;
        let unit = usize::try_from(position).ok().and_then(|position| text.units().get(position));
        Some(unit.map_or_else(|| Value::number(f64::NAN), |unit| Value::number(unit.into())))
    }

    /// The one body of `String.prototype.charAt`: the one-unit string of `receiver` at `index`,
    /// `""` outside the string, shared strings for ASCII units. `None` where the receiver is no
    /// string or the index is no Number. Interpreter fast paths and the native both call this.
    #[inline(always)]
    pub fn string_char_at_core(&mut self, receiver: Value, index: Value) -> Option<Value> {
        let records = self.heap.string_record_base().unwrap_or(std::ptr::null());
        let table = self.heap.ascii_unit_strings_base().unwrap_or(std::ptr::null());
        if let Some(character) = concrete_char_at(ConcreteString::Record { records, value: receiver }, table, index) {
            return Some(character);
        }
        // Any other flat text: the same row over its units; a unit with no shared string yet
        // is made by the general steps below.
        if let Some(character) = self.heap.flat_text(receiver).and_then(|text| concrete_char_at(ConcreteString::Units(text.units()), table, index)) {
            return Some(character);
        }
        let position = string_unit_index(index)?;
        let unit = {
            let text = self.heap.string(receiver)?;
            usize::try_from(position).ok().and_then(|position| text.units().get(position))
        };
        Some(match unit {
            None => self.heap.alloc_string(JsString::from_units(&[])),
            Some(unit) => match self.ascii_unit_string(unit) {
                Some(value) => value,
                None => self.heap.alloc_string(JsString::from_units(&[unit])),
            },
        })
    }
}
