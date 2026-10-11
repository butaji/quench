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
const REGEXP_LEGACY_CAPTURE_COUNT: usize = 9;
const REPLACEMENT_CAPTURE_RADIX: usize = 10;
const REGEXP_SEARCH_NOT_FOUND_INDEX: f64 = -1.0;
const REGEXP_LEGACY_ACCESSOR_GROUPS: &[(&[&str], bool)] = &[
    (&["input", "$_"], true),
    (&["lastMatch", "$&"], false),
    (&["lastParen", "$+"], false),
    (&["leftContext", "$`"], false),
    (&["rightContext", "$'"], false),
];

#[derive(Clone)]
pub(super) struct RegExpIntrinsics {
    pub(super) constructor: Value,
    pub(super) prototype: Value,
    legacy_input: Option<JsString>,
    legacy_match: LegacyRegExpMatch,
}

#[derive(Clone)]
enum LegacyRegExpMatch {
    Empty,
    Matched {
        input: JsString,
        matched: quench_regexp::Match,
    },
    Invalid,
}

impl RegExpIntrinsics {
    pub(super) fn new(constructor: Value, prototype: Value) -> Self {
        Self {
            constructor,
            prototype,
            legacy_input: Some(JsString::from_units(&[])),
            legacy_match: LegacyRegExpMatch::Empty,
        }
    }
}

enum RegExpConstructorInput {
    Internal {
        source: JsString,
        original_flags: Option<String>,
    },
    Observable {
        source: Value,
        flags: Value,
    },
}

impl<H: Host> Vm<H> {
    pub(super) fn regexp_intrinsic_constructor(&self) -> Value {
        self.realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .expect("RegExp intrinsics are installed for the active realm")
            .constructor
    }

    pub(super) fn install_regexp_intrinsics(
        &mut self,
        realm: Value,
        constructor: Value,
        prototype: Value,
    ) -> Result<(), JsError> {
        self.realm
            .intrinsics
            .regexp_intrinsics
            .insert(realm, RegExpIntrinsics::new(constructor, prototype));
        self.set_builtin_value_named(constructor, "prototype", prototype)?;
        let prototype_atom = self.prototype_atom();
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
        let initial_constructor = self.native_value(Native::RegExp);
        for (name, native) in [
            ("compile", Native::RegExpCompile),
            ("exec", Native::RegExpExec),
            ("test", Native::RegExpTest),
            ("toString", Native::RegExpToString),
        ] {
            let method = if constructor == initial_constructor {
                self.native_value(native)
            } else {
                self.native_with_realm(native, realm, realm)
            };
            self.set_builtin_function_name(method, name)?;
            self.set_builtin_value_named(prototype, name, method)?;
        }
        Ok(())
    }

    pub(super) fn install_regexp(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        for name in ["source", "flags", "lastIndex", "index", "input"] {
            self.intern_atom(name);
        }
        let constructor = self.native_value(Native::RegExp);
        self.regexp_proto = self.object();
        self.install_regexp_intrinsics(self.realm.globals, constructor, self.regexp_proto)?;
        self.set_builtin_named(program, constructor, "escape", Native::RegExpEscape)?;
        self.install_regexp_symbol_properties(constructor, self.regexp_proto, self.realm.globals)?;
        self.install_regexp_accessors(program, self.regexp_proto, self.realm.globals)?;
        self.install_regexp_legacy_accessors(program, constructor, self.realm.globals)?;
        self.global(program, "RegExp", constructor)
    }

    pub(super) fn install_regexp_legacy_accessors(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        for capture in 1..=REGEXP_LEGACY_CAPTURE_COUNT {
            let name = format!("${capture}");
            self.install_regexp_legacy_accessor(program, constructor, realm, &name, false)?;
        }
        for (names, has_setter) in REGEXP_LEGACY_ACCESSOR_GROUPS {
            for name in *names {
                self.install_regexp_legacy_accessor(
                    program,
                    constructor,
                    realm,
                    name,
                    *has_setter,
                )?;
            }
        }
        Ok(())
    }

    fn install_regexp_legacy_accessor(
        &mut self,
        program: &ResidualProgram,
        constructor: Value,
        realm: Value,
        name: &str,
        has_setter: bool,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        let selector = Value::number(atom as f64);
        let getter = self.native_with_realm(Native::RegExpLegacyGetter, selector, realm);
        self.set_builtin_function_name(getter, &format!("get RegExp.{name}"))?;
        let setter =
            has_setter.then(|| self.native_with_realm(Native::RegExpLegacySetter, selector, realm));
        if let Some(setter) = setter {
            self.set_builtin_function_name(setter, &format!("set RegExp.{name}"))?;
        }
        self.set_named(program, constructor, name, getter)?;
        self.set_property_attributes(
            constructor,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: true,
                getter: Some(getter),
                setter,
            },
        );
        Ok(())
    }

    pub(super) fn install_regexp_accessors(
        &mut self,
        program: &ResidualProgram,
        prototype: Value,
        realm: Value,
    ) -> Result<(), JsError> {
        // Preserve the existing prototype property order: hasIndices was
        // installed after the older accessors. Flags lookup uses canonical order.
        let (has_indices, older_flags) = REGEXP_FLAG_ACCESSORS
            .split_first()
            .expect("RegExp flag metadata includes hasIndices");
        for (name, native, _) in older_flags.iter().chain(std::iter::once(has_indices)) {
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
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                if !vm.is_object_like(receiver) {
                    return Err(vm.type_error(
                        p,
                        "RegExp.prototype[@@replace] receiver is not an object".into(),
                    ));
                }
                let input =
                    vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let replacement = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let callable = vm.is_function(replacement);
                let template = if callable {
                    None
                } else {
                    Some(vm.coerce_js_string(p, replacement)?)
                };
                let input_value = vm.heap.alloc(Cell::String(input.clone()));
                vm.with_call_roots([input_value], |vm| {
                    if let Some(template) = template.as_ref()
                        && let Some((matcher, flags)) = vm.regexp_replace_fast_matcher(receiver)
                    {
                        return vm.regexp_symbol_replace_fast(
                            p, receiver, &input, template, &matcher, &flags,
                        );
                    }
                    let atom = vm.intern_atom("flags");
                    let flags = vm.get_property(p, receiver, atom)?;
                    let flags = vm.coerce_js_string(p, flags)?;
                    let global = flags.host_string().contains('g');
                    let unicode =
                        flags.host_string().contains('u') || flags.host_string().contains('v');
                    let last_index_atom = vm.intern_atom("lastIndex");
                    if global {
                        vm.set_property_with_program_mode(
                            p,
                            receiver,
                            last_index_atom,
                            Value::number(0.0),
                            true,
                        )?;
                    }
                    // Results are the authoritative list. Scoped call roots trace its
                    // values until replacement finishes, including every abrupt exit.
                    let mut results = Vec::new();
                    loop {
                        let result = vm.regexp_exec_value(p, receiver, input_value)?;
                        if result.is_null() {
                            break;
                        }
                        results.push(result);
                        vm.active_call_roots.push(result);
                        if !global {
                            break;
                        }
                        let atom = vm.intern_atom("0");
                        let matched = vm.get_property(p, result, atom)?;
                        let matched = vm.coerce_js_string(p, matched)?;
                        if matched.units().is_empty() {
                            let index = vm.get_property(p, receiver, last_index_atom)?;
                            let index = vm.regexp_to_length_value(p, index)?;
                            let next = advance_string_index_units(input.units(), index, unicode);
                            vm.set_property_with_program_mode(
                                p,
                                receiver,
                                last_index_atom,
                                Value::number(next as f64),
                                true,
                            )?;
                        }
                    }
                    let mut output = super::wtf16::JsStringBuilder::default();
                    let mut next_source = 0;
                    for result in results {
                        vm.with_call_roots([result], |vm| {
                            let atom = vm.intern_atom("length");
                            let length = vm.get_property(p, result, atom)?;
                            let length = vm.regexp_to_length_value(p, length)?;
                            let atom = vm.intern_atom("0");
                            let matched = vm.get_property(p, result, atom)?;
                            let matched = vm.coerce_js_string(p, matched)?;
                            let atom = vm.intern_atom("index");
                            let position = vm.get_property(p, result, atom)?;
                            let position = vm.to_primitive(p, position, "number")?;
                            let position =
                                regexp_to_integer_or_infinity(vm.to_number(p, position)?);
                            let position =
                                position.max(0.0).min(input.units().len() as f64) as usize;
                            let mut captures = Vec::new();
                            for index in 1..length {
                                let atom = vm.intern_atom(&index.to_string());
                                let capture = vm.get_property(p, result, atom)?;
                                let capture = if capture.is_undefined() {
                                    capture
                                } else {
                                    let string = vm.coerce_js_string(p, capture)?;
                                    vm.heap.alloc(Cell::String(string))
                                };
                                captures.push(capture);
                                vm.active_call_roots.push(capture);
                            }
                            let atom = vm.intern_atom("groups");
                            let groups = vm.get_property(p, result, atom)?;
                            let text = vm.with_call_roots([groups], |vm| {
                                if callable {
                                    let matched = vm.heap.alloc(Cell::String(matched.clone()));
                                    let mut args = Vec::new();
                                    args.push(matched);
                                    args.extend(captures.iter().copied());
                                    args.push(Value::number(position as f64));
                                    args.push(input_value);
                                    if !groups.is_undefined() {
                                        args.push(groups);
                                    }
                                    let value =
                                        vm.call_value(p, replacement, Value::UNDEFINED, &args)?;
                                    vm.coerce_js_string(p, value)
                                } else {
                                    let groups =
                                        if groups.is_undefined() || vm.is_object_like(groups) {
                                            groups
                                        } else {
                                            vm.require_object_coercible(p, groups)?;
                                            vm.box_primitive_object(groups)?
                                        };
                                    vm.with_call_roots([groups], |vm| {
                                        vm.replacement_substitution(
                                            p,
                                            template
                                                .as_ref()
                                                .expect("template replacement owns its string"),
                                            &input,
                                            position,
                                            &matched,
                                            &captures,
                                            groups,
                                        )
                                    })
                                }
                            })?;
                            // Backward positions still run replacement effects; only
                            // the projection into the output is conditional.
                            if position >= next_source {
                                output.append_slice(&input, next_source..position);
                                output.append(&text);
                                next_source = position.saturating_add(matched.units().len());
                            }
                            Ok::<_, JsError>(())
                        })?;
                    }
                    if next_source < input.units().len() {
                        output.append_slice(&input, next_source..input.units().len());
                    }
                    let output = vm.string_build_result(p, output.finish())?;
                    Ok(vm.heap.alloc(Cell::String(output)))
                })
            },
        )
    }

    fn regexp_replace_fast_matcher(
        &self,
        receiver: Value,
    ) -> Option<(Rc<quench_regexp::Regex>, String)> {
        let Some(Cell::RegExp { matcher, meta, .. }) = self.heap.get(receiver) else {
            return None;
        };
        let flags = &meta.flags;
        if !flags.contains('g') || matcher.capture_count() != 0 {
            return None;
        }
        let intrinsics = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)?;
        if self.object_data(receiver)?.proto != intrinsics.prototype {
            return None;
        }
        let exec = self.lookup_atom("exec")?;
        let flags_atom = self.lookup_atom("flags")?;
        let last_index = self.lookup_atom("lastIndex")?;
        if self
            .property_attributes(receiver, PropertyKey::string(exec))
            .is_some()
            || self
                .property_attributes(receiver, PropertyKey::string(flags_atom))
                .is_some()
            || !self
                .property_attributes(receiver, PropertyKey::string(last_index))
                .is_some_and(|attributes| attributes.writable && !attributes.accessor)
            || !self.regexp_prototype_native_property(
                intrinsics.prototype,
                exec,
                Native::RegExpExec,
                false,
            )
            || !self.regexp_prototype_native_property(
                intrinsics.prototype,
                flags_atom,
                Native::RegExpFlags,
                true,
            )
        {
            return None;
        }
        for (name, native, _) in REGEXP_FLAG_ACCESSORS {
            let atom = self.lookup_atom(name)?;
            if self
                .property_attributes(receiver, PropertyKey::string(atom))
                .is_some()
                || !self.regexp_prototype_native_property(intrinsics.prototype, atom, *native, true)
            {
                return None;
            }
        }
        Some((Rc::clone(matcher), flags.clone()))
    }

    fn regexp_prototype_native_property(
        &self,
        prototype: Value,
        atom: Atom,
        expected: Native,
        accessor: bool,
    ) -> bool {
        let Some(attributes) = self.property_attributes(prototype, PropertyKey::string(atom))
        else {
            return false;
        };
        if attributes.accessor != accessor {
            return false;
        }
        let value = if accessor {
            let Some(getter) = attributes.getter else {
                return false;
            };
            getter
        } else {
            let Some(value) = self.own_property(prototype, atom) else {
                return false;
            };
            value
        };
        matches!(
            self.heap.get(value),
            Some(Cell::Function {
                kind: FunctionKind::Native(native),
                ..
            }) if *native == expected
        )
    }

    /// The compiled matcher for `source` and `flags`, shared by every RegExp created from them.
    /// Matching never re-enters JavaScript, so sharing the matcher's capture workspace is safe.
    pub(super) fn cached_regexp_matcher(
        &mut self,
        source: &JsString,
        flags: &str,
    ) -> Result<Rc<quench_regexp::Regex>, JsError> {
        if let Some(matcher) = self.regexp_matchers.find(source, flags) {
            return Ok(matcher);
        }
        let matcher = Rc::new(Self::compile_regexp(source, flags)?);
        self.regexp_matchers
            .insert(source, flags, Rc::clone(&matcher));
        Ok(matcher)
    }

    /// The prepared form of `input`, rebuilt only when a different string is matched.
    fn regexp_subject(&mut self, input: &JsString) -> Rc<quench_regexp::Subject> {
        match &self.regexp_subject {
            Some(subject) if subject.is_text_of(input.shared_units()) => Rc::clone(subject),
            _ => {
                let subject = Rc::new(quench_regexp::Subject::new(Rc::clone(input.shared_units())));
                self.regexp_subject = Some(Rc::clone(&subject));
                subject
            }
        }
    }

    fn regexp_symbol_replace_fast(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        template: &JsString,
        matcher: &quench_regexp::Regex,
        flags: &str,
    ) -> Result<Value, JsError> {
        let last_index = self
            .lookup_atom("lastIndex")
            .expect("RegExp installation interns lastIndex");
        self.set_property_with_program_mode(p, receiver, last_index, Value::number(0.0), true)?;

        let mut output = super::wtf16::JsStringBuilder::default();
        let mut next_source = 0;
        let mut search_start = 0;
        let sticky = flags.contains('y');
        let unicode = flags.contains('u') || flags.contains('v');
        let subject = self.regexp_subject(input);
        while let Some(found) = matcher.find_in_subject(&subject, search_start).next() {
            let start = found.range.start;
            let end = found.range.end;
            if sticky && start != search_start {
                break;
            }
            output.append_slice(input, next_source..start);
            let matched = JsString::from_units(&input.units()[start..end]);
            let replacement = self.replacement_substitution(
                p,
                template,
                input,
                start,
                &matched,
                &[],
                Value::UNDEFINED,
            )?;
            output.append(&replacement);
            next_source = end;
            search_start = if start == end {
                advance_string_index_units(input.units(), end, unicode)
            } else {
                end
            };
        }
        self.set_property_with_program_mode(p, receiver, last_index, Value::number(0.0), true)?;
        output.append_slice(input, next_source..input.units().len());
        let output = self.string_build_result(p, output.finish())?;
        Ok(self.heap.alloc(Cell::String(output)))
    }

    pub(super) fn replacement_substitution(
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
        let mut output = super::wtf16::JsStringBuilder::default();
        let mut cursor = 0usize;
        while cursor < units.len() {
            if units[cursor] != u16::from(b'$') || cursor + 1 == units.len() {
                output.append_slice(template, cursor..cursor + 1);
                cursor += 1;
                continue;
            }
            match units[cursor + 1] {
                unit if unit == u16::from(b'$') => {
                    output.append_slice(template, cursor..cursor + 1);
                    cursor += 2;
                }
                unit if unit == u16::from(b'&') => {
                    output.append(matched);
                    cursor += 2;
                }
                unit if unit == u16::from(b'`') => {
                    output.append_slice(input, 0..match_position);
                    cursor += 2;
                }
                unit if unit == u16::from(b'\'') => {
                    let end = match_position.saturating_add(matched.units().len());
                    output.append_slice(input, end.min(input.units().len())..input.units().len());
                    cursor += 2;
                }
                unit if (u16::from(b'0')..=u16::from(b'9')).contains(&unit) => {
                    let first = usize::from(units[cursor + 1] - u16::from(b'0'));
                    let second = units
                        .get(cursor + 2)
                        .filter(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(unit))
                        .map(|unit| {
                            first * REPLACEMENT_CAPTURE_RADIX + usize::from(*unit - u16::from(b'0'))
                        });
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
                            // RegExp replacement has already converted every capture.
                            let Some(Cell::String(value)) = self.heap.get(value) else {
                                unreachable!("replacement captures are strings or undefined");
                            };
                            output.append(value);
                        }
                        cursor += consumed + 1;
                    } else {
                        output.append_slice(template, cursor..cursor + 1);
                        cursor += 1;
                    }
                }
                unit if unit == u16::from(b'<') && !groups.is_undefined() => {
                    let Some(end) = units[cursor + 2..]
                        .iter()
                        .position(|unit| *unit == u16::from(b'>'))
                        .map(|offset| cursor + 2 + offset)
                    else {
                        output.append_slice(template, cursor..cursor + 1);
                        cursor += 1;
                        continue;
                    };
                    let name = self
                        .heap
                        .alloc(Cell::String(JsString::from_units(&units[cursor + 2..end])));
                    let value = self.get_index(p, groups, name)?;
                    if !value.is_undefined() {
                        let value = self.coerce_js_string(p, value)?;
                        output.append(&value);
                    }
                    cursor = end + 1;
                }
                _ => {
                    output.append_slice(template, cursor..cursor + 1);
                    cursor += 1;
                }
            }
        }
        self.string_build_result(p, output.finish())
    }
    pub(super) fn regexp_symbol_search(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                if !vm.is_object_like(receiver) {
                    return Err(
                        vm.type_error(p, "RegExp.prototype[@@search] called on non-object".into())
                    );
                }
                let input =
                    vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let atom = vm.intern_atom("lastIndex");
                let previous = vm.get_property(p, receiver, atom)?;
                vm.with_call_roots([previous], |vm| {
                    if !vm.same_value(previous, Value::number(0.0)) {
                        vm.set_property_with_program_mode(
                            p,
                            receiver,
                            atom,
                            Value::number(0.0),
                            true,
                        )?;
                    }
                    let input = vm.heap.alloc(Cell::String(input));
                    let result = vm.regexp_exec_value(p, receiver, input)?;
                    vm.with_call_roots([result], |vm| {
                        let current = vm.get_property(p, receiver, atom)?;
                        if !vm.same_value(current, previous) {
                            vm.set_property_with_program_mode(p, receiver, atom, previous, true)?;
                        }
                        if result.is_null() {
                            return Ok(Value::number(REGEXP_SEARCH_NOT_FOUND_INDEX));
                        }
                        let atom = vm.intern_atom("index");
                        vm.get_property(p, result, atom)
                    })
                })
            },
        )
    }

    pub(super) fn regexp_symbol_split(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                if !vm.is_object_like(receiver) {
                    return Err(vm.type_error(
                        p,
                        "RegExp.prototype[@@split] receiver is not an object".into(),
                    ));
                }
                let input =
                    vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let constructor = vm.regexp_species_constructor(p, receiver)?;
                let (unicode, splitter) = vm.with_call_roots([constructor], |vm| {
                    let atom = vm.intern_atom("flags");
                    let value = vm.get_property(p, receiver, atom)?;
                    let mut flags = vm.coerce_js_string(p, value)?;
                    let unicode =
                        flags.host_string().contains('u') || flags.host_string().contains('v');
                    if !flags.host_string().contains('y') {
                        flags.push_js_string(&JsString::from_str("y"));
                    }
                    let flags = vm.heap.alloc(Cell::String(flags));
                    let splitter = vm.construct_value(p, constructor, &[receiver, flags])?;
                    Ok::<_, JsError>((unicode, splitter))
                })?;
                let array = vm.new_array(Vec::new());
                let input_value = vm.heap.alloc(Cell::String(input.clone()));
                vm.with_call_roots([splitter, array, input_value], |vm| {
                    let limit = vm.regexp_split_limit(p, args.get(1).copied())?;
                    if limit == 0 {
                        return Ok(array);
                    }
                    let size = input.units().len();
                    if size == 0 {
                        if vm.regexp_exec_value(p, splitter, input_value)?.is_null() {
                            vm.append_fresh_array_element(array, input_value);
                        }
                        return Ok(array);
                    }
                    let last_index_atom = vm.intern_atom("lastIndex");
                    let length_atom = vm.intern_atom("length");
                    let mut p_index = 0usize;
                    let mut q = 0usize;
                    while q < size {
                        vm.set_property_with_program_mode(
                            p,
                            splitter,
                            last_index_atom,
                            Value::number(q as f64),
                            true,
                        )?;
                        let result = vm.regexp_exec_value(p, splitter, input_value)?;
                        if result.is_null() {
                            q = advance_string_index_units(input.units(), q, unicode);
                            continue;
                        }
                        let (next, full) = vm.with_call_roots([result], |vm| {
                            let value = vm.get_property(p, splitter, last_index_atom)?;
                            let end = vm.regexp_to_length_value(p, value)?.min(size);
                            if end == p_index {
                                return Ok::<_, JsError>((
                                    advance_string_index_units(input.units(), q, unicode),
                                    false,
                                ));
                            }
                            let piece = vm.heap.alloc(Cell::String(JsString::from_units(
                                &input.units()[p_index..q],
                            )));
                            if vm.append_fresh_array_element(array, piece) >= limit {
                                return Ok((end, true));
                            }
                            p_index = end;
                            let length = vm.get_property(p, result, length_atom)?;
                            let captures = vm.regexp_to_length_value(p, length)?.saturating_sub(1);
                            for index in 1..=captures {
                                let atom = vm.intern_atom(&index.to_string());
                                let capture = vm.get_property(p, result, atom)?;
                                if vm.append_fresh_array_element(array, capture) >= limit {
                                    return Ok((end, true));
                                }
                            }
                            Ok((end, false))
                        })?;
                        if full {
                            return Ok(array);
                        }
                        q = next;
                    }
                    let tail = vm.heap.alloc(Cell::String(JsString::from_units(
                        &input.units()[p_index..],
                    )));
                    vm.append_fresh_array_element(array, tail);
                    Ok(array)
                })
            },
        )
    }

    fn regexp_species_constructor(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([receiver], |vm| {
            let constructor_atom = vm.intern_atom("constructor");
            let constructor = vm.get_property(p, receiver, constructor_atom)?;
            if constructor.is_undefined() {
                return Ok(vm.regexp_intrinsic_constructor());
            }
            if !vm.is_object_like(constructor) {
                return Err(vm.type_error(p, "RegExp constructor is not an object".into()));
            }
            vm.with_call_roots([constructor], |vm| {
                let symbol = vm
                    .well_known_symbols
                    .get("species")
                    .copied()
                    .ok_or_else(|| {
                        vm.type_error(p, "RegExp species symbol is unavailable".into())
                    })?;
                let species = vm.get_index(p, constructor, symbol)?;
                if species.is_undefined() || species.is_null() {
                    return Ok(vm.regexp_intrinsic_constructor());
                }
                if !vm.is_constructable(p, species) {
                    return Err(vm.type_error(p, "RegExp species is not a constructor".into()));
                }
                Ok(species)
            })
        })
    }

    pub(super) fn regexp_split_limit(
        &mut self,
        p: &ResidualProgram,
        value: Option<Value>,
    ) -> Result<usize, JsError> {
        let Some(value) = value.filter(|value| !value.is_undefined()) else {
            return Ok(u32::MAX as usize);
        };
        self.with_call_roots([value], |vm| {
            let value = vm.to_primitive(p, value, "number")?;
            Ok(number_to_u32(vm.to_number(p, value)?) as usize)
        })
    }

    pub(super) fn regexp_symbol_match_all(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                if !vm.is_object_like(receiver) {
                    return Err(vm.type_error(
                        p,
                        "RegExp.prototype[@@matchAll] receiver is not an object".into(),
                    ));
                }
                let input =
                    vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let constructor = vm.regexp_species_constructor(p, receiver)?;
                let (flags, matcher) = vm.with_call_roots([constructor], |vm| {
                    let atom = vm.intern_atom("flags");
                    let value = vm.get_property(p, receiver, atom)?;
                    let flags = vm.coerce_js_string(p, value)?;
                    let value = vm.heap.alloc(Cell::String(flags.clone()));
                    let matcher = vm.construct_value(p, constructor, &[receiver, value])?;
                    Ok::<_, JsError>((flags, matcher))
                })?;
                vm.with_call_roots([matcher], |vm| {
                    let atom = vm.intern_atom("lastIndex");
                    let value = vm.get_property(p, receiver, atom)?;
                    let index = vm.regexp_to_length_value(p, value)?;
                    vm.set_property_with_program_mode(
                        p,
                        matcher,
                        atom,
                        Value::number(index as f64),
                        true,
                    )?;
                    let flags = flags.host_string();
                    Ok(vm.heap.alloc(Cell::Iterator {
                        object: Box::new(Self::empty_object(vm.regexp_string_iterator_proto)),
                        source: matcher,
                        kind: IteratorKind::RegExpStringMatchAll,
                        index: 0,
                        done: false,
                        ext: Box::new(crate::heap::IteratorExt {
                            next_method: None,
                            helper: Some(Box::new(IteratorHelper::RegExpStringMatchAll {
                                input,
                                global: flags.contains('g'),
                                unicode: flags.contains('u') || flags.contains('v'),
                            })),
                            helper_running: false,
                            helper_started: false,
                            generator: None,
                        }),
                    }))
                })
            },
        )
    }

    pub(super) fn regexp_to_length_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<usize, JsError> {
        self.with_call_roots([value], |vm| {
            let value = vm.to_primitive(p, value, "number")?;
            Ok(regexp_to_length(vm.to_number(p, value)?))
        })
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

    pub(super) fn regexp_symbol_match(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                if !vm.is_object_like(receiver) {
                    return Err(
                        vm.type_error(p, "RegExp.prototype[@@match] called on non-object".into())
                    );
                }
                let input =
                    vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let atom = vm.intern_atom("flags");
                let value = vm.get_property(p, receiver, atom)?;
                let flags = vm.coerce_js_string(p, value)?;
                if !flags.host_string().contains('g') {
                    let input = vm.heap.alloc(Cell::String(input));
                    return vm.regexp_exec_value(p, receiver, input);
                }
                let unicode =
                    flags.host_string().contains('u') || flags.host_string().contains('v');
                let last_index = vm.intern_atom("lastIndex");
                vm.set_property_with_program_mode(
                    p,
                    receiver,
                    last_index,
                    Value::number(0.0),
                    true,
                )?;
                let array = vm.new_array(Vec::new());
                let input_value = vm.heap.alloc(Cell::String(input.clone()));
                vm.with_call_roots([array, input_value], |vm| {
                    loop {
                        let result = vm.regexp_exec_value(p, receiver, input_value)?;
                        if result.is_null() {
                            let Some(cell @ Cell::Array { .. }) = vm.heap.get(array) else {
                                unreachable!("match owns its fresh result array");
                            };
                            let elements = cell.array_elements();
                            return Ok(if elements.is_empty() {
                                Value::NULL
                            } else {
                                array
                            });
                        }
                        let empty = vm.with_call_roots([result], |vm| {
                            let zero = vm.intern_atom("0");
                            let value = vm.get_property(p, result, zero)?;
                            let matched = vm.coerce_js_string(p, value)?;
                            let empty = matched.units().is_empty();
                            let matched = vm.heap.alloc(Cell::String(matched));
                            vm.append_fresh_array_element(array, matched);
                            Ok::<_, JsError>(empty)
                        })?;
                        if empty {
                            let value = vm.get_property(p, receiver, last_index)?;
                            let index = vm.regexp_to_length_value(p, value)?;
                            let next = advance_string_index_units(input.units(), index, unicode);
                            vm.set_property_with_program_mode(
                                p,
                                receiver,
                                last_index,
                                Value::number(next as f64),
                                true,
                            )?;
                        }
                    }
                })
            },
        )
    }

    pub(super) fn regexp_exec_value(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([receiver, input], |vm| {
            let exec = vm.intern_atom("exec");
            let method = vm.get_property(p, receiver, exec)?;
            if vm.is_function(method) {
                let result = vm.call_value(p, method, receiver, &[input])?;
                return if result.is_null() || vm.is_object_like(result) {
                    Ok(result)
                } else {
                    Err(vm.type_error(p, "RegExp exec result is not an object".into()))
                };
            }
            if matches!(vm.heap.get(receiver), Some(Cell::RegExp { .. })) {
                return vm.regexp_builtin_exec(p, receiver, &[input]);
            }
            Err(vm.type_error(p, "RegExp exec is not callable".into()))
        })
    }

    pub(super) fn regexp_slot_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
    ) -> Result<Value, JsError> {
        match (native, self.heap.get(this)) {
            (Native::RegExpSource, Some(Cell::RegExp { meta, .. })) => Ok(self
                .heap
                .alloc(Cell::String(escape_regexp_source(&meta.source)))),
            (Native::RegExpSource, _)
                if self
                    .realm
                    .intrinsics
                    .regexp_intrinsics
                    .get(&self.realm.globals)
                    .is_some_and(|intrinsics| intrinsics.prototype == this) =>
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
            Some(Cell::RegExp { meta, .. }) => meta.flags.clone(),
            _ if self
                .realm
                .intrinsics
                .regexp_intrinsics
                .get(&self.realm.globals)
                .is_some_and(|intrinsics| intrinsics.prototype == this) =>
            {
                return Ok(Value::UNDEFINED);
            }
            _ => {
                return Err(
                    self.type_error(p, "RegExp accessor called on incompatible receiver".into())
                );
            }
        };
        let flag = REGEXP_FLAG_ACCESSORS
            .iter()
            .find(|(_, accessor, _)| *accessor == native)
            .map(|(_, _, flag)| *flag)
            .ok_or_else(|| JsError("invalid RegExp flag accessor".into()))?;
        let contains = flags.contains(flag);
        Ok(if contains { Value::TRUE } else { Value::FALSE })
    }

    pub(super) fn regexp_flags_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([receiver], |vm| {
            if !vm.is_object_like(receiver) {
                return Err(vm.type_error(p, "RegExp.prototype.flags called on non-object".into()));
            }
            let mut flags = String::new();
            for (property, _, flag) in REGEXP_FLAG_ACCESSORS {
                let atom = vm.intern_atom(property);
                let value = vm.get_property(p, receiver, atom)?;
                if vm.truthy(value) {
                    flags.push(*flag);
                }
            }
            Ok(vm.heap.alloc(Cell::String(flags.into())))
        })
    }

    pub(super) fn regexp_legacy_getter_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        self.require_regexp_constructor_receiver(p, receiver)?;
        let selector = self
            .active_native_env()
            .and_then(Value::as_number)
            .ok_or_else(|| JsError("missing RegExp legacy property selector".into()))?
            as Atom;
        let name = self.atom_name(selector);
        let state = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .expect("active realm has RegExp intrinsics");
        let value = if matches!(name, "input" | "$_") {
            state.legacy_input.clone()
        } else {
            match &state.legacy_match {
                LegacyRegExpMatch::Invalid => None,
                LegacyRegExpMatch::Empty => Some(JsString::from_units(&[])),
                LegacyRegExpMatch::Matched { input, matched } => {
                    let range = match name {
                        "lastMatch" | "$&" => Some(matched.range.clone()),
                        "leftContext" | "$`" => Some(0..matched.range.start),
                        "rightContext" | "$'" => Some(matched.range.end..input.units().len()),
                        "lastParen" | "$+" => matched.captures.last().cloned().flatten(),
                        _ => name
                            .strip_prefix('$')
                            .and_then(|index| index.parse::<usize>().ok())
                            .and_then(|index| index.checked_sub(1))
                            .and_then(|index| matched.captures.get(index))
                            .cloned()
                            .flatten(),
                    };
                    Some(JsString::from_units(
                        range.map_or(&[][..], |range| &input.units()[range]),
                    ))
                }
            }
        };
        match value {
            Some(value) => Ok(self.heap.alloc(Cell::String(value))),
            None => Err(self.type_error(p, "RegExp legacy match state is unavailable".into())),
        }
    }

    pub(super) fn regexp_legacy_setter_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.require_regexp_constructor_receiver(p, receiver)?;
        let value = self.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
        self.realm
            .intrinsics
            .regexp_intrinsics
            .get_mut(&self.realm.globals)
            .expect("active realm has RegExp intrinsics")
            .legacy_input = Some(value);
        Ok(Value::UNDEFINED)
    }

    fn require_regexp_constructor_receiver(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<(), JsError> {
        if self.same_value(receiver, self.regexp_intrinsic_constructor()) {
            Ok(())
        } else {
            Err(self.type_error(
                p,
                "RegExp legacy accessor called on incompatible receiver".into(),
            ))
        }
    }

    pub(super) fn construct_regexp_native(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        new_target: Option<Value>,
    ) -> Result<Value, JsError> {
        self.with_call_roots(args.iter().copied().chain(new_target), |vm| {
            let pattern_value = args.first().copied().unwrap_or(Value::UNDEFINED);
            let flags_value = args.get(1).copied().unwrap_or(Value::UNDEFINED);
            let pattern_is_regexp = vm.regexp_is_regexp(p, pattern_value)?;
            let flags_omitted = flags_value.is_undefined();
            if new_target.is_none() && flags_omitted && pattern_is_regexp {
                let constructor_atom = vm.intern_atom("constructor");
                let constructor = vm.get_property(p, pattern_value, constructor_atom)?;
                let intrinsic = vm.regexp_intrinsic_constructor();
                if vm.same_value(constructor, intrinsic) {
                    return Ok(pattern_value);
                }
            }
            let input = if let Some(Cell::RegExp { meta, .. }) = vm.heap.get(pattern_value) {
                RegExpConstructorInput::Internal {
                    source: meta.source.clone(),
                    original_flags: flags_omitted.then(|| meta.flags.clone()),
                }
            } else {
                let source = if pattern_is_regexp {
                    let source_atom = vm.intern_atom("source");
                    vm.get_property(p, pattern_value, source_atom)?
                } else {
                    pattern_value
                };
                vm.active_call_roots.push(source);
                let flags = if flags_omitted && pattern_is_regexp {
                    let flags_atom = vm.intern_atom("flags");
                    vm.get_property(p, pattern_value, flags_atom)?
                } else {
                    flags_value
                };
                vm.active_call_roots.push(flags);
                RegExpConstructorInput::Observable { source, flags }
            };
            let prototype = if let Some(new_target) = new_target {
                vm.regexp_prototype_from_new_target(p, new_target)?
            } else {
                vm.realm
                    .intrinsics
                    .regexp_intrinsics
                    .get(&vm.realm.globals)
                    .expect("RegExp intrinsics are installed for the active realm")
                    .prototype
            };
            vm.active_call_roots.push(prototype);
            let (pattern, flags) = match input {
                RegExpConstructorInput::Internal {
                    source,
                    original_flags,
                } => {
                    let flags = if let Some(flags) = original_flags {
                        flags
                    } else {
                        vm.to_string(p, flags_value)?
                    };
                    (source, flags)
                }
                RegExpConstructorInput::Observable { source, flags } => {
                    vm.regexp_initialization_strings(p, source, flags)?
                }
            };
            let intrinsic = vm.regexp_intrinsic_constructor();
            let legacy_constructor =
                if new_target.is_none_or(|target| vm.same_value(target, intrinsic)) {
                    crate::heap::RegExpLegacyOwner::Enabled(intrinsic)
                } else {
                    crate::heap::RegExpLegacyOwner::Disabled(intrinsic)
                };
            vm.regexp_from_source(prototype, pattern, flags, legacy_constructor)
        })
    }

    pub(super) fn regexp_create(
        &mut self,
        p: &ResidualProgram,
        pattern: Value,
        flags: Value,
    ) -> Result<Value, JsError> {
        let prototype = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .expect("RegExp intrinsics are installed for the active realm")
            .prototype;
        let (source, flags) = self.regexp_initialization_strings(p, pattern, flags)?;
        self.regexp_from_source(
            prototype,
            source,
            flags,
            crate::heap::RegExpLegacyOwner::Enabled(self.regexp_intrinsic_constructor()),
        )
    }

    fn regexp_initialization_strings(
        &mut self,
        p: &ResidualProgram,
        pattern: Value,
        flags: Value,
    ) -> Result<(JsString, String), JsError> {
        self.with_call_roots([pattern, flags], |vm| {
            let source = if pattern.is_undefined() {
                JsString::from_str("")
            } else {
                vm.coerce_js_string(p, pattern)?
            };
            let flags = if flags.is_undefined() {
                String::new()
            } else {
                vm.to_string(p, flags)?
            };
            Ok((source, flags))
        })
    }

    fn regexp_from_source(
        &mut self,
        prototype: Value,
        source: JsString,
        flags: String,
        legacy_constructor: crate::heap::RegExpLegacyOwner,
    ) -> Result<Value, JsError> {
        let matcher = self.cached_regexp_matcher(&source, &flags)?;
        self.regexp_from_matcher(prototype, source, flags, matcher, legacy_constructor)
    }

    pub(super) fn regexp_literal(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site_index: usize,
    ) -> Result<Value, JsError> {
        let program = self.frames[frame].program;
        let site = p
            .regexp_literal_sites
            .get(site_index)
            .ok_or_else(|| JsError::validation("RegExp literal site is outside program".into()))?;
        let source = self
            .programs
            .constant(program, site.pattern_constant as usize)
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(source)) => Some(source.clone()),
                _ => None,
            })
            .ok_or_else(|| JsError::validation("RegExp literal pattern is not a string".into()))?;
        let flags = self
            .programs
            .constant(program, site.flags_constant as usize)
            .and_then(|value| match self.heap.get(value) {
                Some(Cell::String(flags)) => Some(flags.host_string().to_owned()),
                _ => None,
            })
            .ok_or_else(|| JsError::validation("RegExp literal flags are not a string".into()))?;
        let matcher = match self.programs.regexp_literal_matcher(program, site_index) {
            Some(matcher) => matcher,
            None => {
                let matcher = self
                    .cached_regexp_matcher(&source, &flags)
                    .map_err(|error| Rc::<str>::from(error.to_string()));
                if !self
                    .programs
                    .cache_regexp_literal_matcher(program, site_index, matcher.clone())
                {
                    return Err(JsError::validation(
                        "RegExp literal cache site is invalid".into(),
                    ));
                }
                matcher
            }
        }
        .map_err(|message| JsError(message.to_string().into()))?;
        let (prototype, constructor) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .map(|intrinsics| (intrinsics.prototype, intrinsics.constructor))
            .expect("RegExp intrinsics are installed for the active realm");
        self.regexp_from_matcher(
            prototype,
            source,
            flags,
            matcher,
            crate::heap::RegExpLegacyOwner::Enabled(constructor),
        )
    }

    fn regexp_from_matcher(
        &mut self,
        prototype: Value,
        source: JsString,
        flags: String,
        matcher: Rc<quench_regexp::Regex>,
        legacy_constructor: crate::heap::RegExpLegacyOwner,
    ) -> Result<Value, JsError> {
        let object = self.heap.alloc(Cell::RegExp {
            object: Box::new(Self::empty_object(prototype)),
            meta: Box::new(crate::heap::RegExpMeta {
                source,
                flags,
                legacy_constructor,
            }),
            matcher,
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

    pub(super) fn regexp_compile_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                let Some(Cell::RegExp { .. }) = vm.heap.get(receiver) else {
                    return Err(vm.type_error(
                        p,
                        "RegExp.prototype.compile called on incompatible receiver".into(),
                    ));
                };
                let enabled = matches!(vm.heap.get(receiver), Some(Cell::RegExp { meta, .. }) if matches!(&meta.legacy_constructor, crate::heap::RegExpLegacyOwner::Enabled(owner) if vm.same_value(*owner, vm.regexp_intrinsic_constructor())));
            if !enabled {
                return Err(vm.type_error(p, "RegExp.prototype.compile called on incompatible receiver".into()));
            }
            let pattern = args.first().copied().unwrap_or(Value::UNDEFINED);
                let flags = args.get(1).copied().unwrap_or(Value::UNDEFINED);
                let (source, flags) = if let Some(Cell::RegExp { meta, .. }) = vm.heap.get(pattern) {
                    if !flags.is_undefined() {
                        return Err(vm.type_error(
                            p,
                            "flags cannot be supplied when pattern is a RegExp".into(),
                        ));
                    }
                    (meta.source.clone(), meta.flags.clone())
                } else {
                    vm.regexp_initialization_strings(p, pattern, flags)?
                };

                let matcher = match vm.cached_regexp_matcher(&source, &flags) {
                    Ok(matcher) => matcher,
                    Err(error) => {
                        let message = error.to_string();
                        let message = message.strip_prefix("SyntaxError: ").unwrap_or(&message);
                        return vm.syntax_error_result(p, message).map(|_| Value::UNDEFINED);
                    }
                };
                let Some(Cell::RegExp {
                    meta,
                    matcher: current_matcher,
                    ..
                }) = vm.heap.get_mut(receiver)
                else {
                    unreachable!("RegExp receiver slot was validated before compilation")
                };
                meta.source = source;
                meta.flags = flags;
                *current_matcher = matcher;
                let last_index = vm.intern_atom("lastIndex");
                vm.set_property_with_program_mode(p, receiver, last_index, Value::number(0.0), true)?;
                Ok(receiver)
            },
        )
    }

    pub(super) fn regexp_to_string_native(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        self.with_call_roots([receiver], |vm| {
            if !vm.is_object_like(receiver) {
                return Err(vm.type_error(
                    p,
                    "RegExp.prototype.toString called on incompatible receiver".into(),
                ));
            }
            let source_atom = vm.intern_atom("source");
            let flags_atom = vm.intern_atom("flags");
            let source = vm.get_property(p, receiver, source_atom)?;
            let source = vm.coerce_js_string(p, source)?;
            let flags = vm.get_property(p, receiver, flags_atom)?;
            let flags = vm.coerce_js_string(p, flags)?;
            let mut output = Vec::new();
            output.push(REGEXP_DELIMITER);
            output.extend_from_slice(source.units());
            output.push(REGEXP_DELIMITER);
            output.extend_from_slice(flags.units());
            vm.string_from_units(&output)
        })
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

    pub(super) fn regexp_test(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(
            std::iter::once(receiver).chain(args.iter().copied()),
            |vm| {
                if !vm.is_object_like(receiver) {
                    return Err(
                        vm.type_error(p, "RegExp.prototype.test called on non-object".into())
                    );
                }
                let input =
                    vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
                let input = vm.heap.alloc(Cell::String(input));
                let result = vm.regexp_exec_value(p, receiver, input)?;
                Ok(if result.is_null() {
                    Value::FALSE
                } else {
                    Value::TRUE
                })
            },
        )
    }

    pub(super) fn regexp_builtin_exec(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        self.with_call_roots(std::iter::once(this).chain(args.iter().copied()), |vm| {
            if !matches!(vm.heap.get(this), Some(Cell::RegExp { .. })) {
                return Err(
                    vm.type_error(p, "RegExp method called on incompatible receiver".into())
                );
            }
            let input =
                vm.coerce_js_string(p, args.first().copied().unwrap_or(Value::UNDEFINED))?;
            let last_index_atom = vm.intern_atom("lastIndex");
            let last_index = vm.get_property(p, this, last_index_atom)?;
            let last_index = vm.regexp_to_length_value(p, last_index)?;
            let (regex, flags) = match vm.heap.get(this) {
                Some(Cell::RegExp { matcher, meta, .. }) => {
                    (Rc::clone(matcher), meta.flags.clone())
                }
                _ => {
                    return Err(
                        vm.type_error(p, "RegExp method called on incompatible receiver".into())
                    );
                }
            };
            let stateful = flags.contains('g') || flags.contains('y');
            let sticky = flags.contains('y');
            let start = if stateful { last_index } else { 0 };
            if stateful && start > input.units().len() {
                vm.set_property_with_program_mode(
                    p,
                    this,
                    last_index_atom,
                    Value::number(0.0),
                    true,
                )?;
                return Ok(Value::NULL);
            }
            let subject = vm.regexp_subject(&input);
            let matched = regex.find_in_subject(&subject, start).next();
            let matched = matched.filter(|matched| !sticky || matched.range.start == start);
            let Some(matched) = matched else {
                if stateful {
                    vm.set_property_with_program_mode(
                        p,
                        this,
                        last_index_atom,
                        Value::number(0.0),
                        true,
                    )?;
                }
                return Ok(Value::NULL);
            };
            if stateful {
                vm.set_property_with_program_mode(
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
                        vm.heap
                            .alloc(Cell::String(JsString::from_units(&input.units()[range])))
                    })
                })
                .collect::<Vec<_>>();
            let result = vm.heap.alloc(Cell::array(
                vm.array_proto,
                Rc::new(values),
            ));
            let named = regexp_named_capture_ranges(&matched);
            let groups = vm.regexp_groups_object(&named, input.units())?;
            let index = matched.range.start;
            let index_atom = vm.intern_atom("index");
            vm.set_property(result, index_atom, Value::number(index as f64))?;
            let input_value = vm.heap.alloc(Cell::String(input.clone()));
            let input_atom = vm.intern_atom("input");
            vm.set_property(result, input_atom, input_value)?;
            let groups_atom = vm.intern_atom("groups");
            vm.set_property(result, groups_atom, groups)?;
            if flags.contains('d') {
                let indices = vm.regexp_indices_array(&matched, &named)?;
                let indices_atom = vm.intern_atom("indices");
                vm.set_property(result, indices_atom, indices)?;
            }
            let owner = match vm.heap.get(this) {
                Some(Cell::RegExp { meta, .. }) => meta.legacy_constructor,
                _ => return Err(JsError("RegExp instance unavailable after match".into())),
            };
            let intrinsic = vm.regexp_intrinsic_constructor();
            if vm.same_value(owner.constructor(), intrinsic) {
                let state = vm
                    .realm
                    .intrinsics
                    .regexp_intrinsics
                    .get_mut(&vm.realm.globals)
                    .expect("active realm has RegExp intrinsics");
                match owner {
                    crate::heap::RegExpLegacyOwner::Enabled(_) => {
                        state.legacy_input = Some(input.clone());
                        state.legacy_match = LegacyRegExpMatch::Matched { input, matched };
                    }
                    crate::heap::RegExpLegacyOwner::Disabled(_) => {
                        state.legacy_input = None;
                        state.legacy_match = LegacyRegExpMatch::Invalid;
                    }
                }
            }
            Ok(result)
        })
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
        let indices = self.heap.alloc(Cell::array(
            self.array_proto,
            Rc::new(entries),
        ));
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
        self.heap.alloc(Cell::array(
            self.array_proto,
            Rc::new(pair.into()),
        ))
    }

    pub(super) fn compile_regexp(
        source: &JsString,
        flags: &str,
    ) -> Result<quench_regexp::Regex, JsError> {
        quench_regexp::validate_flags(flags)
            .map_err(|error| JsError(format!("SyntaxError: {error}").into()))?;
        let parser_source =
            regexp_parser_source(source, flags.contains('u') || flags.contains('v'))?;
        crate::compile::regexp::validate_pattern(&parser_source, flags)
            .map_err(|error| JsError(format!("SyntaxError: {error}").into()))?;
        let regex = catch_unwind(AssertUnwindSafe(|| {
            quench_regexp::Regex::with_flags(&parser_source, quench_regexp::Flags::from(flags))
        }))
        .map_err(|_| JsError("SyntaxError: invalid regular expression".into()))?
        .map_err(|error| {
            JsError(format!("SyntaxError: invalid regular expression: {error}").into())
        })?;
        Ok(regex)
    }
}

fn regexp_parser_source(
    source: &JsString,
    unicode: bool,
) -> Result<std::borrow::Cow<'_, str>, JsError> {
    let needs_projection = if unicode {
        char::decode_utf16(source.units().iter().copied()).any(|character| character.is_err())
    } else {
        source
            .units()
            .iter()
            .any(|unit| (HIGH_SURROGATE_START..=LOW_SURROGATE_END).contains(unit))
    };
    if !needs_projection {
        return Ok(std::borrow::Cow::Borrowed(source.host_string()));
    }
    use std::fmt::Write;
    let mut projected = String::with_capacity(source.host_string().len());
    let mut escaped = false;
    for character in char::decode_utf16(source.units().iter().copied()) {
        match character {
            Ok(character) if unicode || character.len_utf16() == 1 => {
                projected.push(character);
                escaped = character == '\\' && !escaped;
            }
            character => {
                if escaped {
                    if unicode {
                        return Err(JsError("SyntaxError: invalid identity escape".into()));
                    }
                    projected.pop();
                }
                const MAX_CODE_POINT_UNITS: usize = char::MAX.len_utf16();
                let mut buffer = [0; MAX_CODE_POINT_UNITS];
                let units = match character {
                    Ok(character) => character.encode_utf16(&mut buffer),
                    Err(error) => {
                        buffer[0] = error.unpaired_surrogate();
                        &mut buffer[..1]
                    }
                };
                for unit in units {
                    if unicode {
                        // Braces prevent raw/escaped surrogate merging.
                        write!(&mut projected, "\\u{{{unit:X}}}").unwrap();
                    } else {
                        // Legacy quantifiers bind to one UTF-16 unit.
                        write!(&mut projected, "\\u{unit:04X}").unwrap();
                    }
                }
                escaped = false;
            }
        }
    }
    Ok(std::borrow::Cow::Owned(projected))
}

fn escape_regexp_source(source: &JsString) -> JsString {
    if source.units().is_empty() {
        return "(?:)".into();
    }
    let mut escaped = Vec::new();
    let mut after_odd_backslashes = false;
    let mut in_character_class = false;
    for &unit in source.units() {
        match unit {
            REGEXP_DELIMITER if !after_odd_backslashes && !in_character_class => {
                escaped.extend([REGEXP_ESCAPE, REGEXP_DELIMITER]);
            }
            REGEXP_NEWLINE | REGEXP_CARRIAGE_RETURN => {
                if !after_odd_backslashes {
                    escaped.push(REGEXP_ESCAPE);
                }
                escaped.push(if unit == REGEXP_NEWLINE {
                    u16::from(b'n')
                } else {
                    u16::from(b'r')
                });
            }
            REGEXP_LINE_SEPARATOR | REGEXP_PARAGRAPH_SEPARATOR => {
                let escape = if unit == REGEXP_LINE_SEPARATOR {
                    [b'2' as u16, b'0' as u16, b'2' as u16, b'8' as u16]
                } else {
                    [b'2' as u16, b'0' as u16, b'2' as u16, b'9' as u16]
                };
                if !after_odd_backslashes {
                    escaped.push(REGEXP_ESCAPE);
                }
                escaped.push(u16::from(b'u'));
                escaped.extend(escape);
            }
            _ => escaped.push(unit),
        }
        if !after_odd_backslashes {
            if unit == u16::from(b'[') {
                in_character_class = true;
            } else if unit == u16::from(b']') {
                in_character_class = false;
            }
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

const REGEXP_FLAG_ACCESSORS: &[(&str, Native, char)] = &[
    ("hasIndices", Native::RegExpHasIndices, 'd'),
    ("global", Native::RegExpGlobal, 'g'),
    ("ignoreCase", Native::RegExpIgnoreCase, 'i'),
    ("multiline", Native::RegExpMultiline, 'm'),
    ("dotAll", Native::RegExpDotAll, 's'),
    ("unicode", Native::RegExpUnicode, 'u'),
    ("unicodeSets", Native::RegExpUnicodeSets, 'v'),
    ("sticky", Native::RegExpSticky, 'y'),
];

/// Upper bound on cached compiled matchers. Compiled automata are the largest per-RegExp
/// allocation, so the cache is capped; reaching the cap drops every entry and recompiles on demand.
const REGEXP_MATCHER_CACHE_LIMIT: usize = 64;

#[derive(Default)]
pub(super) struct RegExpMatcherCache {
    by_source: rustc_hash::FxHashMap<JsString, Vec<(String, Rc<quench_regexp::Regex>)>>,
    entries: usize,
}

impl RegExpMatcherCache {
    fn find(&self, source: &JsString, flags: &str) -> Option<Rc<quench_regexp::Regex>> {
        self.by_source
            .get(source)?
            .iter()
            .find_map(|(cached, matcher)| (cached == flags).then(|| Rc::clone(matcher)))
    }

    fn insert(&mut self, source: &JsString, flags: &str, matcher: Rc<quench_regexp::Regex>) {
        if self.entries >= REGEXP_MATCHER_CACHE_LIMIT {
            self.clear();
        }
        self.by_source
            .entry(source.clone())
            .or_default()
            .push((flags.to_owned(), matcher));
        self.entries += 1;
    }

    pub(super) fn clear(&mut self) {
        self.by_source.clear();
        self.entries = 0;
    }
}
