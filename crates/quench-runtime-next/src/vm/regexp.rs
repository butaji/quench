use super::*;
use std::panic::{AssertUnwindSafe, catch_unwind};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
const HIGH_SURROGATE_START: u16 = 0xD800;
const HIGH_SURROGATE_END: u16 = 0xDBFF;
const LOW_SURROGATE_START: u16 = 0xDC00;
const LOW_SURROGATE_END: u16 = 0xDFFF;
const REGEXP_ESCAPE: u16 = b'\\' as u16;
const REGEXP_DELIMITER: u16 = b'/' as u16;
const REGEXP_NEWLINE: u16 = b'\n' as u16;
const REGEXP_CARRIAGE_RETURN: u16 = b'\r' as u16;
const REGEXP_LINE_SEPARATOR: u16 = 0x2028;
const REGEXP_PARAGRAPH_SEPARATOR: u16 = 0x2029;

pub(super) struct CompiledRegexp(quench_regexp::Regex);

struct RegExpReplaceMatch {
    capture_roots: Vec<RootId>,
    groups_root: Option<RootId>,
    matched: JsString,
    position: f64,
}

impl CompiledRegexp {
    pub(super) fn find_from_utf16(
        &self,
        input: &[u16],
        start: usize,
    ) -> Option<quench_regexp::Match> {
        self.0.find_from_utf16(input, start).next()
    }

    pub(super) fn find_range_from_utf16(
        &self,
        input: &[u16],
        start: usize,
    ) -> Option<std::ops::Range<usize>> {
        self.0.find_range_from_utf16(input, start)
    }

}

impl<H: Host> Vm<H> {
    pub(super) fn install_regexp(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for name in ["source", "flags", "lastIndex", "index", "input"] {
            self.intern_atom(name);
        }
        let constructor = self.native_value(Native::RegExp);
        self.regexp_proto = self.object();
        self.regexp_prototypes
            .insert(self.realm.globals, self.regexp_proto);
        self.set_named(program, constructor, "prototype", self.regexp_proto)?;
        self.set_builtin_named(program, constructor, "escape", Native::RegExpEscape)?;
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
        self.install_regexp_accessors(program, self.regexp_proto, self.realm.globals)?;
        self.global(program, "RegExp", constructor)
    }

    pub(super) fn install_regexp_accessors(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        for (name, native) in REGEXP_FLAG_ACCESSORS {
            let getter = self.native_with_realm(*native, realm, realm);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_named(program, prototype, name, getter)?;
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
        for (name, native) in [
            ("source", Native::RegExpSource),
            ("flags", Native::RegExpFlags),
        ] {
            let getter = self.native_with_realm(native, realm, realm);
            self.set_builtin_function_name(getter, &format!("get {name}"))?;
            let atom = self.intern_atom(name);
            self.set_named(program, prototype, name, getter)?;
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
        Ok(())
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
        if let Some(symbol) = self.well_known_symbols.get("search").copied() {
            let method = self.native_with_realm(Native::RegExpSymbolSearch, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.search]")?;
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
        if let Some(symbol) = self.well_known_symbols.get("split").copied() {
            let method = self.native_with_realm(Native::RegExpSymbolSplit, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.split]")?;
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
        if let Some(symbol) = self.well_known_symbols.get("matchAll").copied() {
            let method = self.native_with_realm(Native::RegExpSymbolMatchAll, realm, realm);
            self.set_builtin_function_name(method, "[Symbol.matchAll]")?;
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
        if !self.is_object_like(receiver) {
            return Err(self.type_error(
                p,
                "RegExp.prototype[@@replace] receiver is not an object".into(),
            ));
        }
        let input =
            self.regexp_input_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let input_value = self.heap.alloc(Cell::String(input.clone()));
        let replacement = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let flags_atom = self.intern_atom("flags");
        let flags_value = self.get_property(p, receiver, flags_atom)?;
        let flags = self.to_string(p, flags_value)?;
        let global_atom = self.intern_atom("global");
        let global_value = self.get_property(p, receiver, global_atom)?;
        let global = self.truthy(global_value);
        let unicode = if global {
            let unicode_atom = self.intern_atom("unicode");
            let unicode_value = self.get_property(p, receiver, unicode_atom)?;
            self.truthy(unicode_value) || flags.contains('v')
        } else {
            false
        };
        let callable = self.is_function(replacement);
        let replacement_string = if callable {
            None
        } else {
            Some(self.regexp_input_string(p, replacement)?)
        };
        let last_index_atom = self.intern_atom("lastIndex");
        if global {
            self.set_property_with_program_mode(
                p,
                receiver,
                last_index_atom,
                Value::number(0.0),
                true,
            )?;
        }

        let mut matches = Vec::new();
        let collection_result = (|| {
            loop {
                let result = self.regexp_exec_value(p, receiver, input_value)?;
                if result.is_null() {
                    break;
                }
                let result_root = self.heap.root(result);
                let mut record_roots = Vec::new();
                let record = (|| {
                    let result = self.heap.root_value(result_root).unwrap_or(result);
                    let matched_atom = self.intern_atom("0");
                    let matched_value = self.get_property(p, result, matched_atom)?;
                    let matched = self.regexp_input_string(p, matched_value)?;
                    let position_atom = self.intern_atom("index");
                    let position_value = self.get_property(p, result, position_atom)?;
                    let position =
                        regexp_to_integer_or_infinity(self.to_number(p, position_value)?);
                    let length_atom = self.intern_atom("length");
                    let length_value = self.get_property(p, result, length_atom)?;
                    let length = regexp_to_length(self.to_number(p, length_value)?);
                    let mut capture_roots = Vec::new();
                    for index in 1..length {
                        let atom = self.intern_atom(&index.to_string());
                        let capture = self.get_property(p, result, atom)?;
                        let capture_root = self.heap.root(capture);
                        let retained_capture_root = if !callable && !capture.is_undefined() {
                            let conversion = self.regexp_input_string(
                                p,
                                self.heap.root_value(capture_root).unwrap_or(capture),
                            );
                            let string = match conversion {
                                Ok(string) => string,
                                Err(error) => {
                                    self.heap.release_root(capture_root);
                                    return Err(error);
                                }
                            };
                            self.heap.release_root(capture_root);
                            let capture = self.heap.alloc(Cell::String(string));
                            self.heap.root(capture)
                        } else {
                            capture_root
                        };
                        capture_roots.push(retained_capture_root);
                        record_roots.push(retained_capture_root);
                    }
                    let groups_atom = self.intern_atom("groups");
                    let groups = self.get_property(p, result, groups_atom)?;
                    if !callable && groups.is_null() {
                        return Err(self.type_error(p, "RegExp replace groups is null".into()));
                    }
                    let groups_root = (!groups.is_undefined()).then(|| self.heap.root(groups));
                    Ok::<_, JsError>((matched, position, capture_roots, groups_root))
                })();
                let (matched, position, capture_roots, groups_root) = match record {
                    Ok(record) => record,
                    Err(error) => {
                        for root in record_roots {
                            self.heap.release_root(root);
                        }
                        self.heap.release_root(result_root);
                        return Err(error);
                    }
                };
                self.heap.release_root(result_root);
                let empty_match = matched.units().is_empty();
                matches.push(RegExpReplaceMatch {
                    capture_roots,
                    groups_root,
                    matched,
                    position,
                });
                if !global {
                    break;
                }
                if empty_match {
                    let last_index = self.get_property(p, receiver, last_index_atom)?;
                    let last_index = regexp_to_length(self.to_number(p, last_index)?);
                    let next = advance_string_index_units(input.units(), last_index, unicode);
                    self.set_property_with_program_mode(
                        p,
                        receiver,
                        last_index_atom,
                        Value::number(next as f64),
                        true,
                    )?;
                }
            }
            Ok::<_, JsError>(())
        })();
        if let Err(error) = collection_result {
            for matched in matches {
                for root in matched.capture_roots {
                    self.heap.release_root(root);
                }
                if let Some(root) = matched.groups_root {
                    self.heap.release_root(root);
                }
            }
            return Err(error);
        }

        if matches.is_empty() {
            return Ok(self.heap.alloc(Cell::String(input)));
        }
        let output_result = (|| {
            let mut output = Vec::new();
            let mut next_source = 0usize;
            for matched in &matches {
                let input_units = input.units().len();
                let position = if matched.position.is_nan() || matched.position <= 0.0 {
                    0
                } else if matched.position.is_infinite() {
                    input_units
                } else {
                    (matched.position.trunc() as usize).min(input_units)
                };
                if position < next_source {
                    continue;
                }
                let captures = matched
                    .capture_roots
                    .iter()
                    .filter_map(|root| self.heap.root_value(*root))
                    .collect::<Vec<_>>();
                let groups = matched
                    .groups_root
                    .and_then(|root| self.heap.root_value(root))
                    .unwrap_or(Value::UNDEFINED);
                output.extend_from_slice(&input.units()[next_source..position]);
                let replacement_text = if callable {
                    let mut callback_args = Vec::with_capacity(captures.len() + 4);
                    callback_args.push(self.heap.alloc(Cell::String(matched.matched.clone())));
                    callback_args.extend(captures);
                    callback_args.push(Value::number(position as f64));
                    callback_args.push(input_value);
                    if !groups.is_undefined() {
                        callback_args.push(groups);
                    }
                    let value =
                        self.call_value(p, replacement, Value::UNDEFINED, &callback_args)?;
                    self.regexp_input_string(p, value)?
                } else {
                    self.regexp_expand_replace_template(
                        p,
                        replacement_string
                            .as_ref()
                            .expect("non-callable replacement"),
                        &input,
                        position,
                        &matched.matched,
                        &captures,
                        groups,
                    )?
                };
                output.extend_from_slice(replacement_text.units());
                let end = position.saturating_add(matched.matched.units().len());
                next_source = end.min(input_units);
            }
            output.extend_from_slice(&input.units()[next_source..]);
            Ok::<_, JsError>(self.heap.alloc(Cell::String(JsString::from_units(&output))))
        })();
        for matched in matches {
            for root in matched.capture_roots {
                self.heap.release_root(root);
            }
            if let Some(root) = matched.groups_root {
                self.heap.release_root(root);
            }
        }
        output_result
    }

    fn regexp_expand_replace_template(
        &mut self,
        p: &ResidualProgram,
        template: &JsString,
        input: &JsString,
        match_position: usize,
        matched: &JsString,
        captures: &[Value],
        groups: Value,
    ) -> Result<JsString, JsError> {
        let units = template.units();
        let mut output = Vec::new();
        let mut cursor = 0usize;
        while cursor < units.len() {
            if units[cursor] != u16::from(b'$') || cursor + 1 == units.len() {
                output.push(units[cursor]);
                cursor += 1;
                continue;
            }
            match units[cursor + 1] {
                unit if unit == u16::from(b'$') => {
                    output.push(u16::from(b'$'));
                    cursor += 2;
                }
                unit if unit == u16::from(b'&') => {
                    output.extend_from_slice(matched.units());
                    cursor += 2;
                }
                unit if unit == u16::from(b'`') => {
                    output.extend_from_slice(&input.units()[..match_position]);
                    cursor += 2;
                }
                unit if unit == u16::from(b'\'') => {
                    let end = match_position.saturating_add(matched.units().len());
                    output.extend_from_slice(&input.units()[end.min(input.units().len())..]);
                    cursor += 2;
                }
                unit if (u16::from(b'0')..=u16::from(b'9')).contains(&unit) => {
                    let first = usize::from(units[cursor + 1] - u16::from(b'0'));
                    let second = units
                        .get(cursor + 2)
                        .filter(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(unit))
                        .map(|unit| first * 10 + usize::from(*unit - u16::from(b'0')));
                    let selected = match (first, second) {
                        (0, Some(index)) if index > 0 && index <= captures.len() => {
                            Some((index, 2))
                        }
                        (0, _) => None,
                        (_, Some(index)) if index <= captures.len() => Some((index, 2)),
                        (_, _) if first <= captures.len() => Some((first, 1)),
                        _ => None,
                    };
                    if let Some((capture, consumed)) = selected {
                        let value = captures[capture - 1];
                        if !value.is_undefined() {
                            let value = self.regexp_input_string(p, value)?;
                            output.extend_from_slice(value.units());
                        }
                        cursor += consumed + 1;
                    } else {
                        output.push(u16::from(b'$'));
                        cursor += 1;
                    }
                }
                unit if unit == u16::from(b'<') && !groups.is_undefined() => {
                    let Some(end) = units[cursor + 2..]
                        .iter()
                        .position(|unit| *unit == u16::from(b'>'))
                        .map(|offset| cursor + 2 + offset)
                    else {
                        output.push(u16::from(b'$'));
                        cursor += 1;
                        continue;
                    };
                    let name = String::from_utf16_lossy(&units[cursor + 2..end]);
                    let atom = self.intern_atom(&name);
                    let value = self.get_property(p, groups, atom)?;
                    if !value.is_undefined() {
                        let value = self.regexp_input_string(p, value)?;
                        output.extend_from_slice(value.units());
                    }
                    cursor = end + 1;
                }
                _ => {
                    output.push(u16::from(b'$'));
                    cursor += 1;
                }
            }
        }
        Ok(JsString::from_units(&output))
    }

    pub(super) fn regexp_symbol_search(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(
                self.type_error(p, "RegExp.prototype[@@search] called on non-object".into())
            );
        }
        let input = self.to_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let last_index_atom = self.intern_atom("lastIndex");
        let previous = self.get_property(p, receiver, last_index_atom)?;
        if !self.same_value(previous, Value::number(0.0)) {
            self.set_property_with_program_mode(
                p,
                receiver,
                last_index_atom,
                Value::number(0.0),
                true,
            )?;
        }
        let result = self.regexp_exec(p, receiver, &input)?;
        let current = self.get_property(p, receiver, last_index_atom)?;
        if !self.same_value(current, previous) {
            self.set_property_with_program_mode(p, receiver, last_index_atom, previous, true)?;
        }
        if result.is_null() {
            return Ok(Value::number(-1.0));
        }
        if !self.is_object_like(result) {
            return Err(self.type_error(p, "RegExp exec result is not an object".into()));
        }
        let index_atom = self.intern_atom("index");
        let index = self.get_property(p, result, index_atom)?;
        let index = regexp_to_length(self.to_number(p, index)?);
        Ok(Value::number(index as f64))
    }

    pub(super) fn regexp_symbol_split(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(
                p,
                "RegExp.prototype[@@split] receiver is not an object".into(),
            ));
        }
        let input =
            self.regexp_input_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let flags_atom = self.intern_atom("flags");
        let flags_value = self.get_property(p, receiver, flags_atom)?;
        let flags = self.to_string(p, flags_value)?;
        let unicode = flags.contains('u') || flags.contains('v');
        let splitter_flags = if flags.contains('y') {
            flags
        } else {
            format!("{flags}y")
        };
        let splitter = self.regexp_split_species(p, receiver, &splitter_flags)?;
        let limit = self.regexp_split_limit(p, args.get(1).copied())?;
        if limit == 0 {
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(Vec::new()),
            }));
        }

        let input_value = self.heap.alloc(Cell::String(input.clone()));
        let size = input.units().len();
        if size == 0 {
            let result = self.regexp_exec_value(p, splitter, input_value)?;
            let values = if result.is_null() {
                vec![input_value]
            } else {
                Vec::new()
            };
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: Rc::new(values),
            }));
        }

        let last_index_atom = self.intern_atom("lastIndex");
        let length_atom = self.intern_atom("length");
        let mut values = Vec::new();
        let mut p_index = 0usize;
        let mut q = 0usize;
        while q < size {
            self.set_property_with_program_mode(
                p,
                splitter,
                last_index_atom,
                Value::number(q as f64),
                true,
            )?;
            let result = self.regexp_exec_value(p, splitter, input_value)?;
            if result.is_null() {
                q = advance_string_index_units(input.units(), q, unicode);
                continue;
            }
            let end_value = self.get_property(p, splitter, last_index_atom)?;
            let end = regexp_to_length(self.to_number(p, end_value)?);
            if end == p_index {
                q = advance_string_index_units(input.units(), q, unicode);
                continue;
            }
            let piece_end = q.min(size);
            values.push(self.heap.alloc(Cell::String(JsString::from_units(
                &input.units()[p_index.min(size)..piece_end],
            ))));
            if values.len() >= limit {
                break;
            }
            let length = self.get_property(p, result, length_atom)?;
            let captures = regexp_to_length(self.to_number(p, length)?).saturating_sub(1);
            for index in 1..=captures {
                if values.len() >= limit {
                    break;
                }
                let capture_atom = self.intern_atom(&index.to_string());
                values.push(self.get_property(p, result, capture_atom)?);
            }
            p_index = end;
            q = p_index;
        }
        if values.len() < limit {
            values.push(self.heap.alloc(Cell::String(JsString::from_units(
                &input.units()[p_index.min(size)..],
            ))));
        }
        Ok(self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        }))
    }

    fn regexp_split_species(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        flags: &str,
    ) -> Result<Value, JsError> {
        let constructor_atom = self.intern_atom("constructor");
        let constructor = self.get_property(p, receiver, constructor_atom)?;
        let species = if constructor.is_undefined() {
            Value::UNDEFINED
        } else {
            if !self.is_object_like(constructor) {
                return Err(self.type_error(p, "RegExp constructor is not an object".into()));
            }
            let species_symbol = self
                .well_known_symbols
                .get("species")
                .copied()
                .ok_or_else(|| self.type_error(p, "RegExp species symbol is unavailable".into()))?;
            self.get_index(p, constructor, species_symbol)?
        };
        let intrinsic = if species.is_undefined() || species.is_null() {
            let regexp_atom = self.intern_atom("RegExp");
            self.get_property(p, self.realm.globals, regexp_atom)?
        } else {
            species
        };
        let flags = self.heap.alloc(Cell::String(flags.into()));
        self.construct_value(p, intrinsic, &[receiver, flags])
    }

    pub(super) fn regexp_split_limit(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<usize, JsError> {
        let Some(value) = value.filter(|value| !value.is_undefined()) else {
            return Ok(u32::MAX as usize);
        };
        let number = self.to_number(p, value)?;
        if !number.is_finite() || number == 0.0 {
            return Ok(0);
        }
        let modulus = f64::from(u32::MAX) + 1.0;
        Ok(number.trunc().rem_euclid(modulus) as u32 as usize)
    }

    pub(super) fn regexp_symbol_match_all(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(
                p,
                "RegExp.prototype[@@matchAll] receiver is not an object".into(),
            ));
        }
        let input =
            self.regexp_input_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let flags_atom = self.intern_atom("flags");
        let flags_value = self.get_property(p, receiver, flags_atom)?;
        let flags = self.to_string(p, flags_value)?;
        let matcher = self.regexp_match_all_species(p, receiver, &flags)?;
        let last_index_atom = self.intern_atom("lastIndex");
        let last_index = self.get_property(p, receiver, last_index_atom)?;
        let last_index =
            regexp_to_length(self.to_number(p, last_index)?).min(MAX_SAFE_INTEGER as usize);
        self.set_property(matcher, last_index_atom, Value::number(last_index as f64))?;
        Ok(self.heap.alloc(Cell::Iterator {
            object: Self::empty_object(self.regexp_string_iterator_proto),
            source: matcher,
            next_method: None,
            helper: Some(Box::new(IteratorHelper::RegExpStringMatchAll {
                input,
                global: flags.contains('g'),
                unicode: flags.contains('u') || flags.contains('v'),
            })),
            helper_running: false,
            helper_started: false,
            kind: IteratorKind::RegExpStringMatchAll,
            index: 0,
            done: false,
            generator: None,
        }))
    }

    fn regexp_match_all_species(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        flags: &str,
    ) -> Result<Value, JsError> {
        let constructor_atom = self.intern_atom("constructor");
        let constructor = self.get_property(p, receiver, constructor_atom)?;
        let species = if !constructor.is_undefined() {
            if !self.is_object_like(constructor) {
                return Err(self.type_error(p, "RegExp constructor is not an object".into()));
            }
            let species_symbol = self
                .well_known_symbols
                .get("species")
                .copied()
                .ok_or_else(|| self.type_error(p, "RegExp species symbol is unavailable".into()))?;
            self.get_index(p, constructor, species_symbol)?
        } else {
            Value::UNDEFINED
        };
        let regexp_atom = self.intern_atom("RegExp");
        let intrinsic = self.get_property(p, self.realm.globals, regexp_atom)?;
        let flags_value = self.heap.alloc(Cell::String(flags.into()));
        if species.is_undefined() || species.is_null() {
            return self.construct_value(p, intrinsic, &[receiver, flags_value]);
        }
        self.construct_value(p, species, &[receiver, flags_value])
    }

    pub(super) fn regexp_is_regexp(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<bool, JsError> {
        if !self.is_object_like(value) {
            return Ok(false);
        }
        let symbol = self
            .well_known_symbols
            .get("match")
            .copied()
            .ok_or_else(|| self.type_error(p, "RegExp match symbol is unavailable".into()))?;
        let matcher = self.get_index(p, value, symbol)?;
        if !matcher.is_undefined() {
            return Ok(self.truthy(matcher));
        }
        Ok(matches!(self.heap.get(value), Some(Cell::RegExp { .. })))
    }

    pub(super) fn regexp_input_string(
        &mut self,
        program: &ResidualProgram,
        value: Value,
    ) -> Result<JsString, JsError> {
        let primitive = self.to_primitive(program, value, "string")?;
        if let Some(Cell::String(string)) = self.heap.get(primitive) {
            return Ok(string.clone());
        }
        self.to_string(program, primitive).map(JsString::from)
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
        let input = self.regexp_input_string(
            p,
            args.first().copied().unwrap_or(Value::UNDEFINED),
        )?;
        let input_value = self.heap.alloc(Cell::String(input.clone()));
        let flags_atom = self.intern_atom("flags");
        let flags_value = self.get_property(p, receiver, flags_atom)?;
        let flags = self.to_string(p, flags_value)?;
        let global = self.intern_atom("global");
        let global_value = self.get_property(p, receiver, global)?;
        if !self.truthy(global_value) {
            return self.regexp_exec_value(p, receiver, input_value);
        }
        let unicode = self.intern_atom("unicode");
        let unicode_sets = self.intern_atom("unicodeSets");
        let unicode_value = self.get_property(p, receiver, unicode)?;
        let full_unicode = if self.truthy(unicode_value) {
            true
        } else {
            let unicode_sets_value = self.get_property(p, receiver, unicode_sets)?;
            self.truthy(unicode_sets_value) || flags.contains('v')
        };
        let last_index = self.intern_atom("lastIndex");
        self.set_property(receiver, last_index, Value::number(0.0))?;
        let mut matches = Vec::new();
        loop {
            let result = self.regexp_exec_value(p, receiver, input_value)?;
            if result.is_null() {
                break;
            }
            if !self.is_object_like(result) {
                return Err(self.type_error(p, "RegExp exec result is not an object".into()));
            }
            let zero = self.intern_atom("0");
            let matched_value = self.get_property(p, result, zero)?;
            let matched = self.coerce_js_string(p, matched_value)?;
            let empty = matched.units().is_empty();
            matches.push(self.heap.alloc(Cell::String(matched)));
            if empty {
                let current_value = self.get_property(p, receiver, last_index)?;
                let current = self.to_number(p, current_value)?;
                let current = if current.is_nan() || current <= 0.0 {
                    0
                } else {
                    current.trunc().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize
                };
                let next = advance_string_index_units(input.units(), current, full_unicode);
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

    pub(super) fn regexp_exec(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &str,
    ) -> Result<Value, JsError> {
        let input = self.heap.alloc(Cell::String(input.into()));
        self.regexp_exec_value(p, receiver, input)
    }

    fn regexp_exec_value(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: Value,
    ) -> Result<Value, JsError> {
        let exec = self.intern_atom("exec");
        let method = self.get_property(p, receiver, exec)?;
        if self.is_function(method) {
            let result = self.call_value(p, method, receiver, &[input])?;
            return if result.is_null() || self.is_object_like(result) {
                Ok(result)
            } else {
                Err(self.type_error(p, "RegExp exec result is not an object".into()))
            };
        }
        if matches!(self.heap.get(receiver), Some(Cell::RegExp { .. })) {
            return self.regexp_native(p, Native::RegExpExec, receiver, &[input]);
        }
        Err(self.type_error(p, "RegExp exec is not callable".into()))
    }

    pub(super) fn regexp_slot_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        match (native, self.heap.get(this)) {
            (Native::RegExpSource, Some(Cell::RegExp { source, .. })) => {
                Ok(self.heap.alloc(Cell::String(escape_regexp_source(source))))
            }
            (Native::RegExpSource, _)
                if self.regexp_prototypes.get(&self.realm.globals) == Some(&this) =>
            {
                Ok(self.heap.alloc(Cell::String("(?:)".into())))
            }
            _ => Err(self.type_error(p, "RegExp accessor called on incompatible receiver".into())),
        }
    }

    pub(super) fn regexp_flag_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        let flags = match self.heap.get(this) {
            Some(Cell::RegExp { flags, .. }) => flags.clone(),
            _ if self.regexp_prototypes.get(&self.realm.globals) == Some(&this) => {
                return Ok(Value::UNDEFINED);
            }
            _ => {
                return Err(
                    self.type_error(p, "RegExp accessor called on incompatible receiver".into())
                );
            }
        };
        let contains = match native {
            Native::RegExpGlobal => flags.contains('g'),
            Native::RegExpIgnoreCase => flags.contains('i'),
            Native::RegExpMultiline => flags.contains('m'),
            Native::RegExpDotAll => flags.contains('s'),
            Native::RegExpUnicode => flags.contains('u'),
            Native::RegExpUnicodeSets => flags.contains('v'),
            Native::RegExpSticky => flags.contains('y'),
            Native::RegExpHasIndices => flags.contains('d'),
            _ => return Err(JsError("invalid RegExp flag accessor".into())),
        };
        Ok(if contains { Value::TRUE } else { Value::FALSE })
    }

    pub(super) fn regexp_flags_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if !self.is_object_like(receiver) {
            return Err(self.type_error(p, "RegExp.prototype.flags called on non-object".into()));
        }
        let properties = [
            ("hasIndices", 'd'),
            ("global", 'g'),
            ("ignoreCase", 'i'),
            ("multiline", 'm'),
            ("dotAll", 's'),
            ("unicode", 'u'),
            ("unicodeSets", 'v'),
            ("sticky", 'y'),
        ];
        let mut flags = String::new();
        for (property, flag) in properties {
            let atom = self.intern_atom(property);
            let value = self.get_property(p, receiver, atom)?;
            if self.truthy(value) {
                flags.push(flag);
            }
        }
        Ok(self.heap.alloc(Cell::String(flags.into())))
    }

    pub(super) fn construct_regexp_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let pattern_value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let flags_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
        let pattern_is_regexp = self.regexp_is_regexp(p, pattern_value)?;
        let flags_omitted = flags_value.is_undefined();
        if flags_omitted && pattern_is_regexp {
            let constructor_atom = self.intern_atom("constructor");
            let constructor = self.get_property(p, pattern_value, constructor_atom)?;
            let regexp_atom = self.intern_atom("RegExp");
            let intrinsic = self.get_property(p, self.realm.globals, regexp_atom)?;
            if self.same_value(constructor, intrinsic) {
                return Ok(pattern_value);
            }
        }
        let source_value = if pattern_value.is_undefined() {
            self.heap.alloc(Cell::String(String::new().into()))
        } else if pattern_is_regexp {
            let source_atom = self.intern_atom("source");
            self.get_property(p, pattern_value, source_atom)?
        } else {
            pattern_value
        };
        let pattern = self.regexp_input_string(p, source_value)?;
        let flags = if flags_omitted && pattern_is_regexp {
            let flags_atom = self.intern_atom("flags");
            let value = self.get_property(p, pattern_value, flags_atom)?;
            self.to_string(p, value)?
        } else if flags_omitted {
            String::new()
        } else {
            self.to_string(p, flags_value)?
        };
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

    pub(super) fn regexp_escape_native(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(Cell::String(input)) = self.heap.get(value) else {
            return Err(self.type_error(program, "RegExp.escape requires a string value".into()));
        };
        Ok(self.heap.alloc(Cell::String(escape_regexp_string(&input))))
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
                return Err(
                    self.type_error(p, "RegExp method called on incompatible receiver".into())
                );
            }
        };
        let regex = Self::compile_regexp(&source, &flags)?;
        let input =
            self.regexp_input_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        let stateful = flags.contains('g') || flags.contains('y');
        let sticky = flags.contains('y');
        let last_index_atom = self.intern_atom("lastIndex");
        let last_index = self.get_property(p, this, last_index_atom)?;
        let last_index = regexp_to_length(self.to_number(p, last_index)?);
        let start = if stateful { last_index } else { 0 };
        if stateful && start > input.units().len() {
            self.set_property_with_program_mode(
                p,
                this,
                last_index_atom,
                Value::number(0.0),
                true,
            )?;
            return Ok(if native == Native::RegExpTest {
                Value::FALSE
            } else {
                Value::NULL
            });
        }
        if native == Native::RegExpTest {
            let matched = regex
                .find_range_from_utf16(input.units(), start)
                .filter(|matched| !sticky || matched.start == start);
            let Some(matched) = matched else {
                if stateful {
                    self.set_property_with_program_mode(
                        p,
                        this,
                        last_index_atom,
                        Value::number(0.0),
                        true,
                    )?;
                }
                return Ok(Value::FALSE);
            };
            if stateful {
                self.set_property_with_program_mode(
                    p,
                    this,
                    last_index_atom,
                    Value::number(matched.end as f64),
                    true,
                )?;
            }
            return Ok(Value::TRUE);
        }
        let matched = regex.find_from_utf16(input.units(), start);
        let matched = matched.filter(|matched| !sticky || matched.range.start == start);
        let Some(matched) = matched else {
            if stateful {
                self.set_property_with_program_mode(
                    p,
                    this,
                    last_index_atom,
                    Value::number(0.0),
                    true,
                )?;
            }
            return Ok(if native == Native::RegExpTest {
                Value::FALSE
            } else {
                Value::NULL
            });
        };
        if stateful {
            self.set_property_with_program_mode(
                p,
                this,
                last_index_atom,
                Value::number(matched.range.end as f64),
                true,
            )?;
        }
        let values = std::iter::once(Some(matched.range.clone()))
            .chain(matched.captures.iter().cloned())
            .map(|range| {
                range.map_or(Value::UNDEFINED, |range| {
                    self.heap
                        .alloc(Cell::String(JsString::from_units(&input.units()[range])))
                })
            })
            .collect::<Vec<_>>();
        let result = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(values),
        });
        let named = regexp_named_capture_ranges(&matched);
        let groups = self.regexp_groups_object(&named, input.units())?;
        let groups_atom = self.intern_atom("groups");
        self.set_property(result, groups_atom, groups)?;
        let index = matched.range.start;
        let index_atom = self.intern_atom("index");
        self.set_property(result, index_atom, Value::number(index as f64))?;
        let input_value = self.heap.alloc(Cell::String(input));
        let input_atom = self.intern_atom("input");
        self.set_property(result, input_atom, input_value)?;
        if flags.contains('d') {
            let indices = self.regexp_indices_array(&matched, &named)?;
            let indices_atom = self.intern_atom("indices");
            self.set_property(result, indices_atom, indices)?;
        }
        Ok(result)
    }

    pub(super) fn regexp_groups_object(
        &mut self,
        named: &[(String, Option<std::ops::Range<usize>>)],
        input: &[u16],
    ) -> Result<Value, JsError> {
        if named.is_empty() {
            return Ok(Value::UNDEFINED);
        }
        let groups = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        for (name, range) in named {
            let atom = self.intern_atom(name);
            let value = range.as_ref().map_or(Value::UNDEFINED, |range| {
                self.heap
                    .alloc(Cell::String(JsString::from_units(&input[range.clone()])))
            });
            self.set_property(groups, atom, value)?;
        }
        Ok(groups)
    }

    fn regexp_indices_array(
        &mut self,
        matched: &quench_regexp::Match,
        named: &[(String, Option<std::ops::Range<usize>>)],
    ) -> Result<Value, JsError> {
        let ranges = std::iter::once(Some(matched.range.clone()))
            .chain(matched.captures.iter().cloned())
            .collect::<Vec<_>>();
        let entries = ranges
            .into_iter()
            .map(|range| self.regexp_index_pair(range))
            .collect::<Vec<_>>();
        let indices = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(entries),
        });
        let groups = if !named.is_empty() {
            let groups = self
                .heap
                .alloc(Cell::Object(Self::empty_object(Value::NULL)));
            for (name, range) in named {
                let atom = self.intern_atom(name);
                let pair = self.regexp_index_pair(range.clone());
                self.set_property(groups, atom, pair)?;
            }
            groups
        } else {
            Value::UNDEFINED
        };
        let groups_atom = self.intern_atom("groups");
        self.set_property(indices, groups_atom, groups)?;
        Ok(indices)
    }

    fn regexp_index_pair(&mut self, range: Option<std::ops::Range<usize>>) -> Value {
        let Some(range) = range else {
            return Value::UNDEFINED;
        };
        let pair = [
            Value::number(range.start as f64),
            Value::number(range.end as f64),
        ];
        self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: Rc::new(pair.into()),
        })
    }

    pub(super) fn compile_regexp(source: &str, flags: &str) -> Result<CompiledRegexp, JsError> {
        quench_regexp::validate_flags(flags)
            .map_err(|error| JsError(format!("SyntaxError: {error}").into()))?;
        crate::compile::regexp::validate_pattern(source, flags)
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

}

fn escape_regexp_source(source: &JsString) -> JsString {
    if source.units().is_empty() {
        return "(?:)".into();
    }
    let mut escaped = Vec::new();
    let mut after_odd_backslashes = false;
    for &unit in source.units() {
        match unit {
            REGEXP_DELIMITER if !after_odd_backslashes => {
                escaped.extend([REGEXP_ESCAPE, REGEXP_DELIMITER]);
            }
            REGEXP_NEWLINE if !after_odd_backslashes => {
                escaped.extend([REGEXP_ESCAPE, b'n' as u16]);
            }
            REGEXP_CARRIAGE_RETURN if !after_odd_backslashes => {
                escaped.extend([REGEXP_ESCAPE, b'r' as u16]);
            }
            REGEXP_LINE_SEPARATOR | REGEXP_PARAGRAPH_SEPARATOR if !after_odd_backslashes => {
                let escape = if unit == REGEXP_LINE_SEPARATOR {
                    [b'2' as u16, b'0' as u16, b'2' as u16, b'8' as u16]
                } else {
                    [b'2' as u16, b'0' as u16, b'2' as u16, b'9' as u16]
                };
                escaped.extend([REGEXP_ESCAPE, b'u' as u16]);
                escaped.extend(escape);
            }
            _ => escaped.push(unit),
        }
        after_odd_backslashes = if unit == REGEXP_ESCAPE {
            !after_odd_backslashes
        } else {
            false
        };
    }
    JsString::from_units(&escaped)
}

pub(super) fn advance_string_index_units(input: &[u16], index: usize, unicode: bool) -> usize {
    if unicode
        && input.get(index).is_some_and(|unit| {
            (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(unit)
                && input
                    .get(index + 1)
                    .is_some_and(|next| (LOW_SURROGATE_START..=LOW_SURROGATE_END).contains(next))
        })
    {
        index + 2
    } else {
        index + 1
    }
}

pub(super) fn regexp_to_length(value: f64) -> usize {
    if value.is_nan() || value <= 0.0 {
        0
    } else if value.is_infinite() {
        MAX_SAFE_INTEGER as usize
    } else {
        value.trunc().min(MAX_SAFE_INTEGER).min(usize::MAX as f64) as usize
    }
}

fn regexp_to_integer_or_infinity(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 {
        0.0
    } else if value.is_infinite() {
        value
    } else {
        value.trunc()
    }
}

pub(super) fn regexp_named_capture_ranges(
    matched: &quench_regexp::Match,
) -> Vec<(String, Option<std::ops::Range<usize>>)> {
    let mut groups: Vec<(String, Option<std::ops::Range<usize>>)> = Vec::new();
    for (name, range) in matched.named_groups() {
        if let Some((_, existing)) = groups.iter_mut().find(|(known, _)| known == name) {
            if existing.is_none() {
                *existing = range;
            }
        } else {
            groups.push((name.to_owned(), range));
        }
    }
    groups
}

fn escape_regexp_string(input: &JsString) -> JsString {
    let units = input.units();
    let mut escaped = String::new();
    let mut index = 0;
    while let Some(unit) = units.get(index) {
        if (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(unit)
            && units
                .get(index + 1)
                .is_some_and(|next| (LOW_SURROGATE_START..=LOW_SURROGATE_END).contains(next))
        {
            let scalar = 0x1_0000
                + ((u32::from(*unit) - u32::from(HIGH_SURROGATE_START)) << 10)
                + u32::from(units[index + 1])
                - u32::from(LOW_SURROGATE_START);
            if let Some(character) = char::from_u32(scalar) {
                escape_regexp_character(&mut escaped, character, index == 0);
            }
            index += 2;
        } else if (HIGH_SURROGATE_START..=LOW_SURROGATE_END).contains(unit) {
            escaped.push_str(&format!("\\u{unit:04x}"));
            index += 1;
        } else {
            if let Some(character) = char::from_u32(u32::from(*unit)) {
                escape_regexp_character(&mut escaped, character, index == 0);
            }
            index += 1;
        }
    }
    JsString::from(escaped)
}

fn escape_regexp_character(output: &mut String, character: char, first: bool) {
    if first && character.is_ascii_alphanumeric() {
        output.push_str(&format!("\\x{:02x}", u32::from(character)));
    } else if let Some(escape) = regexp_escape_control(character) {
        output.push_str(escape);
    } else if "^$\\.*+?()[]{}|/".contains(character) {
        output.push('\\');
        output.push(character);
    } else if ",-=<>#&!%:;@~'`\"".contains(character) || character == ' ' {
        output.push_str(&format!("\\x{:02x}", u32::from(character)));
    } else if character.is_control() || character.is_whitespace() || character == '\u{FEFF}' {
        if u32::from(character) <= 0xFF {
            output.push_str(&format!("\\x{:02x}", u32::from(character)));
        } else {
            output.push_str(&format!("\\u{:04x}", u32::from(character)));
        }
    } else {
        output.push(character);
    }
}

fn regexp_escape_control(character: char) -> Option<&'static str> {
    match character {
        '\n' => Some("\\n"),
        '\r' => Some("\\r"),
        '\t' => Some("\\t"),
        '\u{000B}' => Some("\\v"),
        '\u{000C}' => Some("\\f"),
        _ => None,
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
