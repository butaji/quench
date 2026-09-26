use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const HIGH_SURROGATE_START: u16 = 0xD800;
const HIGH_SURROGATE_END: u16 = 0xDBFF;
const LOW_SURROGATE_START: u16 = 0xDC00;
const LOW_SURROGATE_END: u16 = 0xDFFF;

pub(super) struct CompiledRegexp(quench_regexp::Regex);

impl CompiledRegexp {
    pub(super) fn find_from(&self, input: &str, start: usize) -> Option<quench_regexp::Match> {
        self.0.find_from(input, start).next()
    }

    pub(super) fn find_iter<'a>(
        &'a self,
        input: &'a str,
    ) -> impl Iterator<Item = quench_regexp::Match> + 'a {
        let mut next_start = 0;
        let mut exhausted = false;
        std::iter::from_fn(move || {
            if exhausted {
                return None;
            }
            let matched = self.find_from(input, next_start)?;
            if matched.range.is_empty() {
                if matched.range.end == input.len() {
                    exhausted = true;
                } else if let Some(next) = input[matched.range.end..].chars().next() {
                    next_start = matched.range.end + next.len_utf8();
                } else {
                    exhausted = true;
                }
            } else {
                next_start = matched.range.end;
            }
            Some(matched)
        })
    }
}

fn utf16_to_byte_index(text: &str, target: usize) -> usize {
    if target == 0 {
        return 0;
    }
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= target {
            return byte;
        }
        units += character.len_utf16();
        if units >= target {
            return byte + character.len_utf8();
        }
    }
    text.len()
}

fn utf16_index(text: &str, byte_index: usize) -> usize {
    text[..byte_index].encode_utf16().count()
}

impl<H: Host> Vm<H> {
    pub(super) fn is_regexp(&self, value: Value) -> bool {
        let mut current = value;
        for _ in 0..32 {
            if matches!(self.heap.get(current), Some(Cell::RegExp { .. })) {
                return true;
            }
            if current == self.regexp_proto {
                return true;
            }
            let Some(Cell::Object(object)) = self.heap.get(current) else {
                return false;
            };
            if object.proto.is_null() {
                return false;
            }
            current = object.proto;
        }
        false
    }

    pub(super) fn install_regexp(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for name in ["source", "flags", "lastIndex", "index", "input"] {
            self.intern_atom(name);
        }
        let constructor = self.native_value(Native::RegExp);
        self.regexp_proto = self.object();
        self.set_named(program, constructor, "prototype", self.regexp_proto)?;
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
        self.set_builtin_named(program, self.regexp_proto, "constructor", Native::RegExp)?;
        self.set_builtin_named(program, self.regexp_proto, "exec", Native::RegExpExec)?;
        self.set_builtin_named(program, self.regexp_proto, "test", Native::RegExpTest)?;
        self.set_builtin_named(
            program,
            self.regexp_proto,
            "toString",
            Native::RegExpToString,
        )?;
        self.install_regexp_symbol_properties(constructor, self.regexp_proto, self.realm.globals)?;
        for (name, native) in REGEXP_FLAG_ACCESSORS {
            let getter = self.native_value(*native);
            let atom = self.intern_atom(name);
            self.set_named(program, self.regexp_proto, name, getter)?;
            self.set_property_attributes(
                self.regexp_proto,
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
        for (name, native) in [
            ("source", Native::RegExpSource),
            ("flags", Native::RegExpFlags),
        ] {
            let getter = self.native_value(native);
            let atom = self.intern_atom(name);
            self.set_named(program, self.regexp_proto, name, getter)?;
            self.set_property_attributes(
                self.regexp_proto,
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
        self.global(program, "RegExp", constructor)
    }

    pub(super) fn install_regexp_symbol_properties(
        &mut self,
        constructor: Value,
        prototype: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        if let Some(symbol) = self.well_known_symbols.get("match").copied() {
            let method = self.native_with_realm(Native::RegExpSymbolMatch, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.match]")?;
            self.set_symbol_property(prototype, symbol, method)?;
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
        }
        if let Some(symbol) = self.well_known_symbols.get("replace").copied() {
            let method = self.native_with_realm(Native::RegExpSymbolReplace, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.replace]")?;
            self.set_symbol_property(prototype, symbol, method)?;
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
        }
        if let Some(symbol) = self.well_known_symbols.get("species").copied() {
            let getter = self.native_with_realm(Native::RegExpSpecies, realm, realm);
            self.set_builtin_function_name(getter, "get [Symbol.species]")?;
            self.set_symbol_property(constructor, symbol, Value::UNDEFINED)?;
            self.set_property_attributes(
                constructor,
                PropertyKey::symbol(symbol),
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
        Ok(())
    }

    pub(super) fn regexp_symbol_replace(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let input = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let input = self.heap.alloc(Cell::String(input.into()));
        let replacement = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        self.string_replace_native(p, input, &[receiver, replacement], false)
    }

    pub(super) fn regexp_symbol_match(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(p, "RegExp.prototype[@@match] called on non-object".into()));
        }
        let input = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let global = self.intern_atom("global");
        let global_value = self.get_property(p, receiver, global)?;
        if !self.truthy(global_value) {
            return self.regexp_exec(p, receiver, &input);
        }
        let unicode = self.intern_atom("unicode");
        let unicode_sets = self.intern_atom("unicodeSets");
        let unicode_value = self.get_property(p, receiver, unicode)?;
        let full_unicode = if self.truthy(unicode_value) {
            true
        } else {
            let unicode_sets_value = self.get_property(p, receiver, unicode_sets)?;
            self.truthy(unicode_sets_value)
        };
        let last_index = self.intern_atom("lastIndex");
        self.set_property(receiver, last_index, Value::number(0.0))?;
        let mut matches = Vec::new();
        loop {
            let result = self.regexp_exec(p, receiver, &input)?;
            if result.is_null() {
                break;
            }
            if !self.is_object_like(result) {
                return Err(self.type_error(p, "RegExp exec result is not an object".into()));
            }
            let zero = self.intern_atom("0");
            let matched_value = self.get_property(p, result, zero)?;
            let matched = self.to_string(p, matched_value)?;
            let empty = matched.is_empty();
            matches.push(self.heap.alloc(Cell::String(matched.into())));
            if empty {
                let current_value = self.get_property(p, receiver, last_index)?;
                let current = self.to_number(p, current_value)?;
                let current = if current.is_nan() || current <= 0.0 {
                    0
                } else {
                    current.trunc().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize
                };
                let next = advance_string_index(&input, current, full_unicode);
                self.set_property(receiver, last_index, Value::number(next as f64))?;
            }
        }
        if matches.is_empty() {
            return Ok(Value::NULL);
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(matches),
        }))
    }

    fn regexp_exec(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &str,
    ) -> Result<Value, JsError> {
        let exec = self.intern_atom("exec");
        let method = self.get_property(p, receiver, exec)?;
        if self.is_function(method) {
            let input = self.heap.alloc(Cell::String(input.into()));
            let result = self.call_value(p, method, receiver, &[input])?;
            return if result.is_null() || self.is_object_like(result) {
                Ok(result)
            } else {
                Err(self.type_error(p, "RegExp exec result is not an object".into()))
            };
        }
        if !method.is_undefined() {
            return Err(self.type_error(p, "RegExp exec is not callable".into()));
        }
        if !matches!(self.heap.get(receiver), Some(Cell::RegExp { .. })) {
            return Err(self.type_error(p, "RegExp exec is not callable".into()));
        }
        let input = self.heap.alloc(Cell::String(input.into()));
        self.regexp_native(p, Native::RegExpExec, receiver, &[input])
    }

    pub(super) fn regexp_slot_native(
        &mut self,
        _p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        match (native, self.heap.get(this)) {
            (Native::RegExpSource, Some(Cell::RegExp { source, .. })) => {
                Ok(self.heap.alloc(Cell::String(source.clone())))
            }
            (Native::RegExpFlags, Some(Cell::RegExp { flags, .. })) => {
                Ok(self.heap.alloc(Cell::String(flags.clone().into())))
            }
            (Native::RegExpSource, _) if this == self.regexp_proto => {
                Ok(self.heap.alloc(Cell::String("(?:)".into())))
            }
            (Native::RegExpFlags, _) if this == self.regexp_proto => {
                Ok(self.heap.alloc(Cell::String(String::new().into())))
            }
            _ => Err(JsError(
                "RegExp accessor called on incompatible receiver".into(),
            )),
        }
    }

    fn regexp_source_string(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<JsString, JsError> {
        let primitive = self.to_primitive(p, value, "string")?;
        if let Some(Cell::String(source)) = self.heap.get(primitive) {
            return Ok(source.clone());
        }
        self.to_string(p, primitive)
            .map(|source| JsString::from_str(&source))
    }

    pub(super) fn regexp_flag_native(
        &mut self,
        _p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let flags = match self.heap.get(this) {
            Some(Cell::RegExp { flags, .. }) => flags.clone(),
            _ => {
                return Err(JsError(
                    "RegExp accessor called on incompatible receiver".into(),
                ));
            }
        };
        let contains = match native {
            Native::RegExpGlobal => flags.contains('g'),
            Native::RegExpIgnoreCase => flags.contains('i'),
            Native::RegExpMultiline => flags.contains('m'),
            Native::RegExpDotAll => flags.contains('s'),
            Native::RegExpUnicode => flags.contains('u') || flags.contains('v'),
            Native::RegExpUnicodeSets => flags.contains('v'),
            Native::RegExpSticky => flags.contains('y'),
            Native::RegExpHasIndices => flags.contains('d'),
            _ => return Err(JsError("invalid RegExp flag accessor".into())),
        };
        Ok(if contains { Value::TRUE } else { Value::FALSE })
    }

    pub(super) fn construct_regexp_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let pattern = match args.first().copied() {
            None | Some(Value::UNDEFINED) => JsString::from_str(""),
            Some(value) => self.regexp_source_string(p, value)?,
        };
        let flags = args
            .get(1)
            .copied()
            .filter(|value| !value.is_undefined())
            .map(|value| self.to_string(p, value))
            .transpose()?
            .unwrap_or_default();
        let regex = Self::compile_regexp(pattern.host_string(), &flags)?;
        drop(regex);
        let object = self.heap.alloc(Cell::RegExp {
            object: Self::empty_object(self.regexp_proto),
            source: pattern,
            flags,
        });
        let last_index_atom = self.intern_atom("lastIndex");
        self.set_property(object, last_index_atom, Value::number(0.0))?;
        self.set_property_attributes(
            object,
            PropertyKey::string(last_index_atom),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(object)
    }

    pub(super) fn regexp_to_string_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if receiver.is_null() || receiver.is_undefined() {
            return Err(self.type_error(
                p,
                "RegExp.prototype.toString called on incompatible receiver".into(),
            ));
        }
        let source_atom = self.intern_atom("source");
        let flags_atom = self.intern_atom("flags");
        let source = self.get_property(p, receiver, source_atom)?;
        let source = self.to_string(p, source)?;
        let flags = self.get_property(p, receiver, flags_atom)?;
        let flags = self.to_string(p, flags)?;
        Ok(self
            .heap
            .alloc(Cell::String(format!("/{source}/{flags}").into())))
    }

    pub(super) fn regexp_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let (source, flags) = match self.heap.get(this) {
            Some(Cell::RegExp { source, flags, .. }) => {
                (source.host_string().to_owned(), flags.clone())
            }
            _ => {
                return Err(JsError(
                    "RegExp method called on incompatible receiver".into(),
                ));
            }
        };
        let regex = Self::compile_regexp(&source, &flags)?;
        let input = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let stateful = flags.contains('g') || flags.contains('y');
        let sticky = flags.contains('y');
        let last_index_atom = self.intern_atom("lastIndex");
        let start = if stateful {
            let value = self.get_property(p, this, last_index_atom)?;
            let number = self.to_number(p, value)?;
            if number.is_finite() && number > 0.0 {
                utf16_to_byte_index(&input, number.floor() as usize)
            } else {
                0
            }
        } else {
            0
        };
        let matched = regex.find_from(&input, start);
        let matched = matched.filter(|matched| !sticky || matched.range.start == start);
        let Some(matched) = matched else {
            if stateful {
                self.set_property(this, last_index_atom, Value::number(0.0))?;
            }
            return Ok(if native == Native::RegExpTest {
                Value::FALSE
            } else {
                Value::NULL
            });
        };
        if native == Native::RegExpTest {
            return Ok(Value::TRUE);
        }
        let values = std::iter::once(Some(matched.range.clone()))
            .chain(matched.captures.iter().cloned())
            .map(|range| {
                range.map_or(Value::UNDEFINED, |range| {
                    self.heap
                        .alloc(Cell::String(input[range].to_owned().into()))
                })
            })
            .collect::<Vec<_>>();
        let result = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        });
        let index = utf16_index(&input, matched.range.start);
        if stateful {
            let end = utf16_index(&input, matched.range.end);
            self.set_property(this, last_index_atom, Value::number(end as f64))?;
        }
        let index_atom = self.intern_atom("index");
        self.set_property(result, index_atom, Value::number(index as f64))?;
        let input_value = self.heap.alloc(Cell::String(input.into()));
        let input_atom = self.intern_atom("input");
        self.set_property(result, input_atom, input_value)?;
        Ok(result)
    }

    pub(super) fn compile_regexp(source: &str, flags: &str) -> Result<CompiledRegexp, JsError> {
        quench_regexp::validate_flags(flags)
            .map_err(|error| JsError(format!("SyntaxError: {error}").into()))?;
        let regex = catch_unwind(AssertUnwindSafe(|| {
            quench_regexp::Regex::with_flags(source, quench_regexp::Flags::from(flags))
        }))
        .map_err(|_| JsError("SyntaxError: invalid regular expression".into()))?
        .map_err(|error| {
            JsError(format!("SyntaxError: invalid regular expression: {error}").into())
        })?;
        Ok(CompiledRegexp(regex))
    }

    pub(super) fn regexp_source_and_flags(&self, value: Value) -> Option<(String, String)> {
        match self.heap.get(value) {
            Some(Cell::RegExp { source, flags, .. }) => {
                Some((source.host_string().to_owned(), flags.clone()))
            }
            _ => None,
        }
    }
}

fn advance_string_index(input: &str, index: usize, unicode: bool) -> usize {
    let units = input.encode_utf16().collect::<Vec<_>>();
    if unicode
        && units.get(index).is_some_and(|unit| {
            (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(unit)
                && units
                    .get(index + 1)
                    .is_some_and(|next| (LOW_SURROGATE_START..=LOW_SURROGATE_END).contains(next))
        })
    {
        index + 2
    } else {
        index + 1
    }
}

const REGEXP_FLAG_ACCESSORS: &[(&str, Native)] = &[
    ("global", Native::RegExpGlobal),
    ("ignoreCase", Native::RegExpIgnoreCase),
    ("multiline", Native::RegExpMultiline),
    ("dotAll", Native::RegExpDotAll),
    ("unicode", Native::RegExpUnicode),
    ("unicodeSets", Native::RegExpUnicodeSets),
    ("sticky", Native::RegExpSticky),
    ("hasIndices", Native::RegExpHasIndices),
];
