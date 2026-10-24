use super::wtf16::FlatUnits;
use super::*;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Recently matched strings whose prepared subjects are kept (`Vm::regexp_subject`): a
/// scanner's input plus the few strings it tests between matches.
const REGEXP_SUBJECT_CACHE_ENTRIES: usize = 4;

/// A subject prepared for one operation's searches (`Vm::regexp_subject`).
pub(super) enum PreparedSubject {
    /// Built for this operation alone, as its searches keep nothing in it.
    Local(quench_regexp::Subject),
    /// Shared through the subject cache, which keeps its decoded units.
    Shared(Rc<quench_regexp::Subject>),
}

impl std::ops::Deref for PreparedSubject {
    type Target = quench_regexp::Subject;

    fn deref(&self) -> &quench_regexp::Subject {
        match self {
            Self::Local(subject) => subject,
            Self::Shared(subject) => subject,
        }
    }
}
// A short string's units fit a subject's inline text.
const _: () = assert!(super::wtf16::SHORT_ASCII_CAPACITY <= quench_regexp::INLINE_SUBJECT_BYTES);

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
    /// The own-property layout every builtin exec result shares, recorded from the first.
    pub(super) result_template: Option<RegExpResultTemplate>,
}

/// An exec result's `index`, `input` and `groups` layout. `anchor` is an object of that
/// shape that keeps it alive; the slots locate each property in it.
#[derive(Clone, Copy)]
pub(super) struct RegExpResultTemplate {
    pub(super) anchor: Value,
    index_slot: usize,
    input_slot: usize,
    groups_slot: usize,
}

/// Where an exec result's `index`, `input` and `groups` sit in its recorded layout.
#[derive(Clone, Copy)]
struct RegExpResultSlots {
    index: usize,
    input: usize,
    groups: usize,
}

impl RegExpResultSlots {
    /// The property values in storage order.
    fn values(self, index: Value, input: Value, groups: Value) -> [Value; REGEXP_RESULT_PROPERTY_COUNT] {
        let mut values = [Value::UNDEFINED; REGEXP_RESULT_PROPERTY_COUNT];
        values[self.index] = index;
        values[self.input] = input;
        values[self.groups] = groups;
        values
    }
}
/// `index`, `input` and `groups`.
const REGEXP_RESULT_PROPERTY_COUNT: usize = 3;

#[derive(Clone)]
enum LegacyRegExpMatch {
    Empty,
    Matched {
        input: JsString,
        matched: quench_regexp::Match,
    },
    /// The match `matcher` finds in `input` from `start`, where a match is known to begin:
    /// found again only when a static is read (`resolve_regexp_legacy_match`), as most
    /// matches' statics are never read.
    Pending {
        input: JsString,
        matcher: Rc<quench_regexp::Regex>,
        start: usize,
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
            result_template: None,
        }
    }
}

/// Legacy match data belongs to the embedding caller, not host builtin setup.
/// These owned strings/match ranges contain no heap references and survive GC.
pub(super) struct LegacyRegExpSnapshot {
    input: Option<JsString>,
    matched: LegacyRegExpMatch,
}

impl<H: Host> Vm<H> {
    pub(super) fn snapshot_regexp_legacy(&self) -> LegacyRegExpSnapshot {
        match self.realm.intrinsics.regexp_intrinsics.get(&self.realm.globals) {
            Some(state) => LegacyRegExpSnapshot {
                input: state.legacy_input.clone(), matched: state.legacy_match.clone(),
            },
            None => LegacyRegExpSnapshot {
                input: Some(JsString::from_units(&[])), matched: LegacyRegExpMatch::Empty,
            },
        }
    }

    pub(super) fn restore_regexp_legacy(&mut self, snapshot: LegacyRegExpSnapshot) {
        if let Some(state) = self.realm.intrinsics.regexp_intrinsics.get_mut(&self.realm.globals) {
            state.legacy_input = snapshot.input;
            state.legacy_match = snapshot.matched;
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

/// Receiver and `RegExp.prototype` shapes verified by `regexp_builtin_receiver_matcher` to leave
/// exec, flags and every flag accessor builtin. Shapes are immutable and every key or
/// attribute change, accessor functions included, moves an object to a new shape, so the proof
/// holds while both shapes match; only the data property `exec`, at `exec_slot` of the
/// prototype, is reread.
#[derive(Clone, Copy)]
pub(super) struct RegExpBuiltinLayout {
    /// The realm whose `RegExp.prototype` (`prototype_object`) the proof was made for.
    realm: Value,
    prototype_object: Value,
    receiver: u32,
    prototype: u32,
    exec_slot: u32,
}

/// A RegExp receiver shape and `RegExp.prototype` shape that together prove `symbol` absent
/// from the receiver and an own data property of the prototype at `slot`. Shapes hold
/// attributes, so the proof holds while both match; the slot's value is reread.
#[derive(Clone, Copy)]
pub(super) struct RegExpSymbolLayout {
    /// The realm whose `RegExp.prototype` (`prototype_object`) the proof was made for.
    realm: Value,
    prototype_object: Value,
    symbol: Value,
    receiver: u32,
    prototype: u32,
    slot: u32,
}

/// The receiver shapes a program's RegExps commonly share, each proven once: a literal's,
/// a `new RegExp` result's (its `Constructed` transition), and a subclass instance's.
pub(super) const REGEXP_RECEIVER_SHAPES: usize = 4;

/// Records `entry` in a shape-proof cache of at most `capacity` entries, evicting the oldest.
fn remember_shape_proof<T>(cache: &mut Vec<T>, capacity: usize, entry: T) {
    if cache.len() == capacity {
        cache.remove(0);
    }
    cache.push(entry);
}

/// The symbol methods a string method looks up on a RegExp argument: `@@replace`,
/// `@@split`, `@@match`, `@@matchAll` and `@@search`, each proven per receiver shape.
pub(super) const REGEXP_SYMBOL_LAYOUTS: usize = 5;

/// The replace value of a fast `RegExp.prototype[@@replace]`: a template already converted
/// to a string, or a callable invoked per match.
#[derive(Clone, Copy)]
enum FastReplacement<'a> {
    Template(&'a JsString),
    Function(Value),
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
        self.set_builtin_function_name(constructor, "RegExp")?;
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Match) {
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Replace) {
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Search) {
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Split) {
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::MatchAll) {
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
        if let Some(symbol) = self.well_known_symbols.get(WellKnownSymbol::Species) {
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
                let input_value =
                    vm.string_value_from(args.first().copied().unwrap_or(Value::UNDEFINED), &input);
                vm.with_call_roots([input_value], |vm| {
                    if let Some((matcher, flags)) = vm.regexp_builtin_receiver_matcher(receiver) {
                        let replacement = match template.as_ref() {
                            Some(template) => FastReplacement::Template(template),
                            None => FastReplacement::Function(replacement),
                        };
                        return vm.regexp_symbol_replace_fast(
                            p,
                            receiver,
                            &input,
                            input_value,
                            replacement,
                            &matcher,
                            flags,
                            true,
                        );
                    }
                    let flags = vm.regexp_flags_text(p, receiver)?;
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
                                    vm.heap.alloc_string(string)
                                };
                                captures.push(capture);
                                vm.active_call_roots.push(capture);
                            }
                            let atom = vm.intern_atom("groups");
                            let groups = vm.get_property(p, result, atom)?;
                            let text = vm.with_call_roots([groups], |vm| {
                                if callable {
                                    let matched = vm.heap.alloc_string(matched.clone());
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
                    Ok(vm.heap.alloc_string(output))
                })
            },
        )
    }

    fn regexp_builtin_receiver_matcher(
        &mut self,
        receiver: Value,
    ) -> Option<(Rc<quench_regexp::Regex>, RegExpFlags)> {
        let Some(Cell::RegExp { meta, .. }) = self.heap.get(receiver) else {
            return None;
        };
        let flags = meta.flags;
        let global = flags.contains(RegExpFlags::GLOBAL);
        // A non-global exec reads lastIndex through ToLength and updates it when sticky;
        // only a numeric lastIndex without stickiness leaves both unobservable.
        if !global && flags.contains(RegExpFlags::STICKY) {
            return None;
        }
        let (prototype, exec_slot) = self.regexp_builtin_layout_proof(receiver)?;
        let exec = self
            .heap
            .property_get(self.object_data(prototype)?, exec_slot)?;
        if !self.is_native_function(exec, Native::RegExpExec) {
            return None;
        }
        if !global {
            self.regexp_last_index_value(receiver)?.1.as_number()?;
        }
        let Some(Cell::RegExp { meta, .. }) = self.heap.get(receiver) else {
            return None;
        };
        Some((Self::regexp_matcher(meta).ok()?, flags))
    }

    /// `GetMethod(receiver, symbol)` for a RegExp `receiver` whose shape and prototype's
    /// shape prove the method an ordinary data property of `RegExp.prototype`
    /// (`RegExpSymbolLayout`); `None` leaves the lookup to the general path.
    pub(super) fn regexp_proven_symbol_method(
        &mut self,
        receiver: Value,
        symbol: Value,
    ) -> Option<Value> {
        if !matches!(self.heap.get(receiver), Some(Cell::RegExp { .. })) {
            return None;
        }
        let receiver_data = self.object_data(receiver)?;
        let (proto, receiver_shape) = (receiver_data.proto(), receiver_data.shape());
        let realm = self.realm.globals;
        // A recorded proof names its realm's prototype, so a match needs no intrinsics lookup.
        if let Some(layout) = self.regexp_symbol_layouts.iter().find(|layout| {
            layout.symbol == symbol && layout.realm == realm && layout.prototype_object == proto
                && layout.receiver == receiver_shape
        }) && let (slot, Some(prototype_data)) = (layout.slot as usize, self.object_data(proto))
            && prototype_data.shape() == layout.prototype
        {
            return self.heap.property_get(prototype_data, slot);
        }
        let prototype = self.realm.intrinsics.regexp_intrinsics.get(&realm)?.prototype;
        if proto != prototype {
            return None;
        }
        let prototype_shape = self.object_data(prototype)?.shape();
        let slot = self.verify_regexp_symbol_layout(receiver, prototype, symbol)?;
        // A changed prototype shape invalidates every proof this realm made against the old one.
        self.regexp_symbol_layouts
            .retain(|layout| layout.realm != realm || layout.prototype == prototype_shape);
        remember_shape_proof(
            &mut self.regexp_symbol_layouts,
            REGEXP_SYMBOL_LAYOUTS * REGEXP_RECEIVER_SHAPES,
            RegExpSymbolLayout {
                realm,
                prototype_object: prototype,
                symbol,
                receiver: receiver_shape,
                prototype: prototype_shape,
                slot: slot as u32,
            },
        );
        self.heap.property_get(self.object_data(prototype)?, slot)
    }

    /// Checks `regexp_proven_symbol_method`'s proof for the current shapes and returns the
    /// prototype's slot of `symbol`.
    #[cold]
    fn verify_regexp_symbol_layout(
        &self,
        receiver: Value,
        prototype: Value,
        symbol: Value,
    ) -> Option<usize> {
        let key = PropertyKey::symbol(symbol);
        let shared = |object: Value| {
            self.object_data(object)
                .is_some_and(|data| !self.shape_is_dictionary(data.shape()))
        };
        if !shared(receiver) || !shared(prototype) || self.property_attributes(receiver, key).is_some()
        {
            return None;
        }
        if self.property_attributes(prototype, key)?.accessor {
            return None;
        }
        self.object_property_slot(prototype, key).map(|(_, slot)| slot)
    }

    /// The shape-determined half of `regexp_builtin_receiver_matcher`'s proof for the RegExp
    /// `receiver`: its prototype is `RegExp.prototype`, neither shadows `exec`, `flags` or a
    /// flag accessor, and the prototype's accessors are the builtins. Returns the prototype
    /// and the slot of its `exec`, whose value the caller still reads.
    fn regexp_builtin_layout_proof(&mut self, receiver: Value) -> Option<(Value, usize)> {
        let receiver_data = self.object_data(receiver)?;
        let (proto, receiver_shape) = (receiver_data.proto(), receiver_data.shape());
        let realm = self.realm.globals;
        // A recorded proof names its realm's prototype, so a match needs no intrinsics lookup.
        let proto_shape = self.object_data(proto).map(|data| data.shape());
        if let Some(layout) = self.regexp_builtin_layouts.iter().find(|layout| {
            layout.realm == realm
                && layout.prototype_object == proto
                && layout.receiver == receiver_shape
                && Some(layout.prototype) == proto_shape
        }) {
            return Some((proto, layout.exec_slot as usize));
        }
        let prototype = self.realm.intrinsics.regexp_intrinsics.get(&realm)?.prototype;
        if proto != prototype {
            return None;
        }
        let prototype_shape = self.object_data(prototype)?.shape();
        let exec_slot = self.verify_regexp_builtin_layout(receiver, prototype)?;
        // A changed prototype shape invalidates every proof this realm made against the old one.
        self.regexp_builtin_layouts
            .retain(|layout| layout.realm != realm || layout.prototype == prototype_shape);
        remember_shape_proof(
            &mut self.regexp_builtin_layouts,
            REGEXP_RECEIVER_SHAPES,
            RegExpBuiltinLayout {
                realm,
                prototype_object: prototype,
                receiver: receiver_shape,
                prototype: prototype_shape,
                exec_slot: exec_slot as u32,
            },
        );
        Some((prototype, exec_slot))
    }

    /// `ToString(Get(R, "flags"))`. Where the builtin layout holds
    /// (`regexp_builtin_layout_proof`), the `flags` getter and each flag getter it calls only
    /// read the RegExp's own flags, so the result is those flags in the getter's order, the
    /// order of `REGEXP_FLAG_ACCESSORS`.
    fn regexp_flags_text(&mut self, p: &ResidualProgram, receiver: Value) -> Result<JsString, JsError> {
        if matches!(self.heap.get(receiver), Some(Cell::RegExp { .. }))
            && self.regexp_builtin_layout_proof(receiver).is_some()
            && let Some(Cell::RegExp { meta, .. }) = self.heap.get(receiver)
        {
            return Ok(JsString::from_str(&meta.flags.text()));
        }
        let atom = self.intern_atom("flags");
        let value = self.get_property(p, receiver, atom)?;
        self.coerce_js_string(p, value)
    }

    /// Checks the shape-determined half of `regexp_builtin_receiver_matcher`'s proof and returns
    /// the slot of `RegExp.prototype.exec`.
    #[cold]
    fn verify_regexp_builtin_layout(&mut self, receiver: Value, prototype: Value) -> Option<usize> {
        let RuntimeAtoms {
            regexp_exec: exec,
            regexp_flags: flags_atom,
            regexp_flag_accessors,
            ..
        } = self.runtime_atoms;
        if self
            .property_attributes(receiver, PropertyKey::string(exec))
            .is_some()
            || self
                .property_attributes(receiver, PropertyKey::string(flags_atom))
                .is_some()
            || self.regexp_last_index_slot(receiver).is_none()
            || !self.regexp_prototype_native_property(prototype, exec, Native::RegExpExec, false)
            || !self.regexp_prototype_native_property(
                prototype,
                flags_atom,
                Native::RegExpFlags,
                true,
            )
        {
            return None;
        }
        for ((_, native, _), atom) in REGEXP_FLAG_ACCESSORS.iter().zip(regexp_flag_accessors) {
            if self
                .property_attributes(receiver, PropertyKey::string(atom))
                .is_some()
                || !self.regexp_prototype_native_property(prototype, atom, *native, true)
            {
                return None;
            }
        }
        self.object_property_slot(prototype, PropertyKey::string(exec))
            .map(|(_, slot)| slot)
    }

    /// The slot of `receiver`'s own writable data `lastIndex`, proven once per receiver shape.
    #[inline(always)]
    fn regexp_last_index_slot(&mut self, receiver: Value) -> Option<usize> {
        let shape = self.object_data(receiver)?.shape();
        if let Some(&(_, slot)) = self
            .regexp_last_index_layouts
            .iter()
            .find(|(cached, _)| *cached == shape)
        {
            return Some(slot as usize);
        }
        self.regexp_last_index_slot_miss(receiver, shape)
    }

    /// The own writable data `lastIndex` of the RegExp `receiver`: its slot
    /// (`regexp_last_index_slot`) and value, read through one lookup of the RegExp's cell.
    #[inline(always)]
    fn regexp_last_index_value(&mut self, receiver: Value) -> Option<(usize, Value)> {
        let Some(Cell::RegExp { object, .. }) = self.heap.get(receiver) else {
            return None;
        };
        let shape = object.shape();
        if let Some(&(_, slot)) = self.regexp_last_index_layouts.iter().find(|(cached, _)| *cached == shape) {
            return Some((slot as usize, self.heap.property_get(object, slot as usize)?));
        }
        let slot = self.regexp_last_index_slot_miss(receiver, shape)?;
        Some((slot, self.heap.property_get(self.object_data(receiver)?, slot)?))
    }

    /// `regexp_last_index_slot` of a shape the layout cache does not hold: the slot of an own
    /// writable data `lastIndex`, cached for `shape`. Out of line, so the hit saves nothing.
    #[cold]
    #[inline(never)]
    fn regexp_last_index_slot_miss(&mut self, receiver: Value, shape: u32) -> Option<usize> {
        let key = PropertyKey::string(self.runtime_atoms.last_index);
        let (_, slot) = self.object_property_slot(receiver, key)?;
        let attributes = self.property_attributes(receiver, key)?;
        if !attributes.writable || attributes.accessor {
            return None;
        }
        remember_shape_proof(
            &mut self.regexp_last_index_layouts,
            REGEXP_RECEIVER_SHAPES,
            (shape, slot as u32),
        );
        Some(slot)
    }

    /// `Get(R, "lastIndex")`, read from its slot when it is an own writable data property.
    fn regexp_last_index(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
    ) -> Result<Value, JsError> {
        if let Some(slot) = self.regexp_last_index_slot(receiver)
            && let Some(value) = self
                .object_data(receiver)
                .and_then(|object| self.heap.property_get(object, slot))
        {
            return Ok(value);
        }
        self.get_property(p, receiver, self.runtime_atoms.last_index)
    }

    /// `Set(R, "lastIndex", value, true)`, stored in its slot when it is an own writable data
    /// property no method cache can observe.
    fn store_regexp_last_index(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        value: Value,
    ) -> Result<(), JsError> {
        let last_index = self.runtime_atoms.last_index;
        if !self.method_caches_may_name(last_index)
            && let Some(slot) = self.regexp_last_index_slot(receiver)
        {
            self.heap.property_set(receiver, slot, value);
            return Ok(());
        }
        self.set_property_with_program_mode(p, receiver, last_index, value, true)
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
        self.is_native_function(value, expected)
    }

    fn is_native_function(&self, value: Value, expected: Native) -> bool {
        matches!(
            self.heap.function(value),
            Some(FunctionRecord {
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
        pattern: PatternValidity,
    ) -> Result<Rc<quench_regexp::Regex>, JsError> {
        if let Some(matcher) = self.regexp_matchers.find(source, flags) {
            return Ok(matcher);
        }
        let matcher = Rc::new(Self::compile_regexp(source, flags, pattern)?);
        self.regexp_matchers
            .insert(source, flags, Rc::clone(&matcher));
        Ok(matcher)
    }

    /// The prepared form of `input` for the searches of `matcher`. A subject keeps only the
    /// units it decodes (`Regex::searches_undecoded`), and preparing one touches no unit, so
    /// a search that decodes nothing prepares it in place; another finds it among the
    /// recently matched strings, rebuilt only when it is not one of them. A scanner
    /// alternates matches over its input with checks of short tokens, so remembering a few
    /// keeps the input's decoding across them.
    fn regexp_subject(&mut self, input: &JsString, matcher: &quench_regexp::Regex) -> PreparedSubject {
        let flat = input.flat_unkept();
        let text = match &*flat {
            // A short subject is copied inline, so its decoding, at most a few units, is
            // never worth finding again.
            FlatUnits::Short(text) => {
                return PreparedSubject::Local(quench_regexp::Subject::short_ascii(text.as_str()));
            }
            FlatUnits::Ascii(_) | FlatUnits::AsciiSlice(_) | FlatUnits::Wide(_) => &*flat,
            // A matcher subject owns whole wide storage, so a slice's run is copied into one;
            // it is not kept, as nothing else holds that copy to find it by.
            FlatUnits::WideSlice(_) => {
                let crate::vm::wtf16::Units::Wide(units) = flat.view() else {
                    unreachable!("a wide slice views wide units")
                };
                return PreparedSubject::Local(quench_regexp::Subject::new(Rc::from(units)));
            }
        };
        let fresh = || match text {
            // Wide storage always holds a non-ASCII unit (`FlatUnits::Wide`).
            FlatUnits::Wide(units) => quench_regexp::Subject::from_non_ascii(Rc::clone(units)),
            ascii => {
                let (text, range) = ascii.shared_ascii().expect("short subjects returned above");
                quench_regexp::Subject::from_ascii(Rc::clone(text), range)
            }
        };
        if matcher.searches_undecoded(!matches!(text, FlatUnits::Wide(_))) {
            return PreparedSubject::Local(fresh());
        }
        let cached = self.regexp_subjects.iter().position(|subject| match text {
            FlatUnits::Wide(units) => subject.is_text_of(units),
            ascii => ascii
                .shared_ascii()
                .is_some_and(|(text, range)| subject.is_ascii_text_of(text, &range)),
        });
        // The cache is ordered by recency: a hit moves to the front, and a miss takes the
        // least recent entry's place, reusing its allocation when no match holds it.
        if let Some(position) = cached {
            self.regexp_subjects[..=position].rotate_right(1);
            return PreparedSubject::Shared(Rc::clone(&self.regexp_subjects[0]));
        }
        if self.regexp_subjects.len() < REGEXP_SUBJECT_CACHE_ENTRIES {
            self.regexp_subjects.push(Rc::new(fresh()));
        } else {
            let least_recent = self.regexp_subjects.last_mut().expect("the cache is full");
            match Rc::get_mut(least_recent) {
                Some(reused) => *reused = fresh(),
                None => *least_recent = Rc::new(fresh()),
            }
        }
        self.regexp_subjects.rotate_right(1);
        PreparedSubject::Shared(Rc::clone(&self.regexp_subjects[0]))
    }

    /// `RegExp.prototype[@@replace]` for a receiver whose exec, flags and lastIndex are its
    /// builtins (`regexp_builtin_receiver_matcher`). The spec runs every exec before computing
    /// any replacement; with the builtin exec those execs run no guest code, so matching
    /// collects the results directly. A replacer then observes the same state: lastIndex
    /// reset by the final failed exec of a global match, and the last match's legacy statics.
    #[allow(clippy::too_many_arguments)]
    fn regexp_symbol_replace_fast(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        input_value: Value,
        replacement: FastReplacement<'_>,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
        result_used: bool,
    ) -> Result<Value, JsError> {
        // Matching, slicing and substitution all read the input; a rope the program keeps
        // (V8-v7 RegExp's input variants) would otherwise keep a flat copy of its text for its
        // life once replaced: 837 KiB of RegExp's 4.3 MB malloc peak (massif, 2026-10-22).
        let input = &input.unkept_flat();
        let subject = self.regexp_subject(input, matcher);
        if let FastReplacement::Template(template) = replacement {
            if !template.units().iter().any(|unit| unit == u16::from(b'$')) {
                return self.regexp_replace_with_literal(
                    p, receiver, input, &subject, template, matcher, flags, result_used,
                );
            }
        }
        let matches = if flags.contains(RegExpFlags::GLOBAL) {
            self.regexp_global_matches(
                p,
                receiver,
                input,
                flags,
                |start| matcher.find_in_subject(&subject, start).next(),
                |found| found.range.clone(),
            )?
        } else {
            matcher
                .find_in_subject(&subject, 0)
                .next()
                .into_iter()
                .collect()
        };
        if let Some(last) = matches.last() {
            self.record_regexp_legacy_match(receiver, input.clone(), last.clone())?;
        }

        let mut output = super::wtf16::JsStringBuilder::default();
        let mut next_source = 0;
        for found in &matches {
            let position = found.range.start;
            let matched = JsString::from_unit_run(input.units().slice(found.range.clone()));
            let captures = found
                .captures
                .iter()
                .map(|range| {
                    range.clone().map_or(Value::UNDEFINED, |range| {
                        self.heap.alloc_string(JsString::from_unit_run(
                            input.units().slice(range),
                        ))
                    })
                })
                .collect::<Vec<_>>();
            let named = regexp_named_capture_ranges(found);
            let groups = self.regexp_groups_object(&named, input.units())?;
            let text = self.with_call_roots(captures.iter().copied().chain([groups]), |vm| {
                match replacement {
                    FastReplacement::Template(template) => vm.replacement_substitution(
                        p, template, input, position, &matched, &captures, groups,
                    ),
                    FastReplacement::Function(function) => {
                        let matched = vm.heap.alloc_string(matched.clone());
                        let mut args = Vec::with_capacity(captures.len() + 4);
                        args.push(matched);
                        args.extend(captures.iter().copied());
                        args.push(Value::number(position as f64));
                        args.push(input_value);
                        if !groups.is_undefined() {
                            args.push(groups);
                        }
                        let value = vm.call_value(p, function, Value::UNDEFINED, &args)?;
                        vm.coerce_js_string(p, value)
                    }
                }
            })?;
            output.append_slice(input, next_source..position);
            output.append(&text);
            next_source = found.range.end;
        }
        output.append_slice(input, next_source..input.units().len());
        let output = self.string_build_result(p, output.finish())?;
        Ok(self.heap.alloc_string(output))
    }

    /// Every match of a global builtin exec loop from index 0, as RegExpBuiltinExec leaves
    /// them: lastIndex 0 before and after, sticky matches contiguous, and an empty
    /// match advancing the search by one string index.
    fn regexp_global_matches<T>(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        flags: RegExpFlags,
        mut find: impl FnMut(usize) -> Option<T>,
        range: impl Fn(&T) -> Range<usize>,
    ) -> Result<Vec<T>, JsError> {
        self.store_regexp_last_index(p, receiver, Value::number(0.0))?;
        let sticky = flags.contains(RegExpFlags::STICKY);
        let unicode = flags.unicode_mode();
        let mut matches = Vec::new();
        let mut search_start = 0;
        while let Some(found) = find(search_start) {
            let found_range = range(&found);
            if sticky && found_range.start != search_start {
                break;
            }
            search_start = if found_range.is_empty() {
                advance_string_index_units(input.units(), found_range.end, unicode)
            } else {
                found_range.end
            };
            matches.push(found);
        }
        // The exec loop leaves lastIndex 0 after its failing match, the value stored above:
        // the matcher runs no guest code, and lastIndex is an own data property (a
        // non-writable one made that store throw), so a second store observes nothing.
        Ok(matches)
    }

    /// `regexp_symbol_replace_fast` for a template without `$`: GetSubstitution returns the
    /// template itself, so no match's captures are observable except the last one's, which
    /// the legacy statics record. A global replace therefore finds match ranges only and
    /// recaptures the last match, which a search from its start finds again.
    #[allow(clippy::too_many_arguments)]
    fn regexp_replace_with_literal(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        subject: &quench_regexp::Subject,
        template: &JsString,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
        result_used: bool,
    ) -> Result<Value, JsError> {
        let (ranges, last) = if flags.contains(RegExpFlags::GLOBAL) {
            let searcher = matcher.range_searcher(subject);
            let ranges = self.regexp_global_matches(
                p,
                receiver,
                input,
                flags,
                |start| searcher.find(start),
                Range::clone,
            )?;
            // Only the last match's captures are observable, through the legacy statics,
            // which find it again from its start if they are read.
            let last = ranges.last().map(|range| Err(range.start));
            (ranges, last)
        } else {
            let found = matcher.find_in_subject(subject, 0).next();
            (
                found.iter().map(|found| found.range.clone()).collect(),
                found.map(Ok),
            )
        };
        let Some(last) = last else {
            if !result_used {
                return Ok(Value::UNDEFINED);
            }
            return Ok(self.heap.alloc_string(input.clone()));
        };
        match last {
            Ok(found) => self.record_regexp_legacy_match(receiver, input.clone(), found)?,
            Err(start) => self.record_regexp_legacy_pending(receiver, input.clone(), matcher, start)?,
        }
        // Nothing reads the result: building it runs no guest code, so only its one
        // observable outcome, a length past the limit raising, needs the length.
        if !result_used {
            let kept = ranges.iter().try_fold(input.units().len(), |length, range| {
                (length - range.len()).checked_add(template.units().len())
            });
            if kept.is_some_and(|length| length <= super::wtf16::MAX_STRING_UNITS) {
                return Ok(Value::UNDEFINED);
            }
        }
        let mut output = super::wtf16::JsStringBuilder::default();
        let mut next_source = 0;
        for range in ranges {
            output.append_slice(input, next_source..range.start);
            output.append(template);
            next_source = range.end;
        }
        output.append_slice(input, next_source..input.units().len());
        let output = self.string_build_result(p, output.finish())?;
        Ok(self.heap.alloc_string(output))
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
            if units.at(cursor) != u16::from(b'$') || cursor + 1 == units.len() {
                output.append_slice(template, cursor..cursor + 1);
                cursor += 1;
                continue;
            }
            match units.at(cursor + 1) {
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
                    let first = usize::from(units.at(cursor + 1) - u16::from(b'0'));
                    let second = units
                        .get(cursor + 2)
                        .filter(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(unit))
                        .map(|unit| {
                            first * REPLACEMENT_CAPTURE_RADIX + usize::from(unit - u16::from(b'0'))
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
                            let Some(value) = self.heap.string(value) else {
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
                    let Some(end) = units
                        .slice(cursor + 2..units.len())
                        .iter()
                        .position(|unit| unit == u16::from(b'>'))
                        .map(|offset| cursor + 2 + offset)
                    else {
                        output.append_slice(template, cursor..cursor + 1);
                        cursor += 1;
                        continue;
                    };
                    let name = self.heap.alloc_string(JsString::from_unit_run(
                        units.slice(cursor + 2..end),
                    ));
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
                    let input = vm.string_value_from(
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                        &input,
                    );
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
                // An object limit's conversion runs guest code, which may change what the
                // splitter constructed before it would observe (`exec` on its prototype).
                let limit_arg = args.get(1).copied();
                let limit_runs_guest_code = limit_arg.is_some_and(|limit| vm.is_object_like(limit));
                if let Some((matcher, flags)) = (!limit_runs_guest_code)
                    .then(|| vm.regexp_unobservable_splitter(receiver, &input))
                    .flatten()
                {
                    let limit = vm.regexp_split_limit(p, limit_arg)?;
                    let array = vm.new_array(Vec::new());
                    if limit == 0 {
                        return Ok(array);
                    }
                    return vm.with_call_roots([array], |vm| {
                        vm.regexp_split_builtin(receiver, &matcher, &input, array, limit, flags.unicode_mode())
                    });
                }
                let constructor = vm.regexp_species_constructor(p, receiver)?;
                let (unicode, splitter) = vm.with_call_roots([constructor], |vm| {
                    let mut flags = vm.regexp_flags_text(p, receiver)?;
                    let unicode =
                        flags.host_string().contains('u') || flags.host_string().contains('v');
                    if !flags.host_string().contains('y') {
                        flags.push_js_string(&JsString::from_str("y"));
                    }
                    let flags = vm.heap.alloc_string(flags);
                    let splitter = vm.construct_value(p, constructor, &[receiver, flags])?;
                    Ok::<_, JsError>((unicode, splitter))
                })?;
                let array = vm.new_array(Vec::new());
                let input_value =
                    vm.string_value_from(args.first().copied().unwrap_or(Value::UNDEFINED), &input);
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
                    if let Some(matcher) = vm.regexp_builtin_exec_matcher(splitter) {
                        return vm.regexp_split_builtin(
                            splitter, &matcher, &input, array, limit, unicode,
                        );
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
                            let piece = vm.heap.alloc_string(JsString::from_unit_run(
                                input.units().slice(p_index..q),
                            ));
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
                    let tail = vm.heap.alloc_string(JsString::from_unit_run(
                        input.units().slice(p_index..input.units().len()),
                    ));
                    vm.append_fresh_array_element(array, tail);
                    Ok(array)
                })
            },
        )
    }

    /// The matcher of `regexp` when `RegExpExec(regexp, …)` resolves to the builtin exec:
    /// a RegExp of this realm with the intrinsic prototype, no own `exec`, and the native
    /// `exec` data property on that prototype. While no guest code runs, that resolution
    /// cannot change.
    fn regexp_builtin_exec_matcher(&self, regexp: Value) -> Option<Rc<quench_regexp::Regex>> {
        let Some(Cell::RegExp { meta, .. }) = self.heap.get(regexp) else {
            return None;
        };
        let matcher = Self::regexp_matcher(meta).ok()?;
        let intrinsics = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)?;
        let exec = self.runtime_atoms.regexp_exec;
        (self.object_data(regexp)?.proto() == intrinsics.prototype
            && self
                .property_attributes(regexp, PropertyKey::string(exec))
                .is_none()
            && self.regexp_prototype_native_property(
                intrinsics.prototype,
                exec,
                Native::RegExpExec,
                false,
            ))
        .then_some(matcher)
    }

    /// `RegExp.prototype[@@split]` steps 19–25 for a sticky `splitter` this call created,
    /// whose exec is the builtin (`regexp_builtin_exec_matcher`). Each failing sticky exec
    /// at `q` leaves no trace, and an unanchored search from `q` finds the first position
    /// whose sticky exec succeeds, with that exec's match; so one search replaces the run
    /// of per-position execs. Only the last successful exec's legacy static state is
    /// observable, and `splitter` itself never escapes.
    fn regexp_split_builtin(
        &mut self,
        splitter: Value,
        matcher: &quench_regexp::Regex,
        input: &JsString,
        array: Value,
        limit: usize,
        unicode: bool,
    ) -> Result<Value, JsError> {
        let size = input.units().len();
        let subject = self.regexp_subject(input, matcher);
        let mut last_match = None;
        let mut p_index = 0usize;
        let mut q = 0usize;
        while q < size {
            let Some(matched) = matcher.find_in_subject(&subject, q).next() else {
                break;
            };
            // The spec's loop only tries sticky execs below `size`.
            if matched.range.start >= size {
                break;
            }
            q = matched.range.start;
            let end = matched.range.end.min(size);
            let captures = matched.captures.clone();
            last_match = Some(matched);
            if end == p_index {
                q = advance_string_index_units(input.units(), q, unicode);
                continue;
            }
            let piece = self.heap.alloc_string(JsString::from_unit_run(
                input.units().slice(p_index..q),
            ));
            if self.append_fresh_array_element(array, piece) >= limit {
                return self.finish_regexp_split_builtin(splitter, input, last_match, array);
            }
            p_index = end;
            for capture in captures {
                let value = capture.map_or(Value::UNDEFINED, |range| {
                    self.heap.alloc_string(JsString::from_unit_run(
                        input.units().slice(range),
                    ))
                });
                if self.append_fresh_array_element(array, value) >= limit {
                    return self.finish_regexp_split_builtin(splitter, input, last_match, array);
                }
            }
            q = p_index;
        }
        let tail = self.heap.alloc_string(JsString::from_unit_run(
            input.units().slice(p_index..input.units().len()),
        ));
        self.append_fresh_array_element(array, tail);
        self.finish_regexp_split_builtin(splitter, input, last_match, array)
    }

    /// Leaves the legacy static state of the last successful exec in `regexp_split_builtin`.
    fn finish_regexp_split_builtin(
        &mut self,
        splitter: Value,
        input: &JsString,
        last_match: Option<quench_regexp::Match>,
        array: Value,
    ) -> Result<Value, JsError> {
        if let Some(matched) = last_match {
            self.record_regexp_legacy_match(splitter, input.clone(), matched)?;
        }
        Ok(array)
    }

    /// The matcher and flags of the splitter `@@split` would construct for `receiver` over the
    /// non-empty `input`, when constructing it is unobservable and the splitter would behave
    /// as `receiver`'s own builtin matcher with `y`: the receiver's exec, flags and flag
    /// accessors are builtin (`regexp_builtin_receiver_matcher`), `SpeciesConstructor`
    /// resolves through the prototype's data `constructor` to this realm's `RegExp` and its
    /// builtin `@@species` getter, `IsRegExp` reads a data `@@match`, and the receiver's
    /// legacy owner is that constructor, the owner a splitter it constructs gets. The splitter
    /// never escapes, so the split runs with the receiver in its place
    /// (`regexp_split_builtin`). The caller takes it only for a limit whose conversion runs no
    /// guest code, since the spec constructs the splitter before converting the limit.
    fn regexp_unobservable_splitter(
        &mut self,
        receiver: Value,
        input: &JsString,
    ) -> Option<(Rc<quench_regexp::Regex>, RegExpFlags)> {
        if input.units().len() == 0 {
            return None;
        }
        let intrinsic = self.regexp_intrinsic_constructor();
        let owner = match self.heap.get(receiver) {
            Some(Cell::RegExp { meta, .. }) => meta.legacy_constructor,
            _ => return None,
        };
        if !matches!(owner, crate::heap::RegExpLegacyOwner::Enabled(constructor) if constructor == intrinsic) {
            return None;
        }
        let matched = self.regexp_builtin_receiver_matcher(receiver)?;
        let match_symbol = self.well_known_symbols.get(WellKnownSymbol::Match)?;
        self.regexp_proven_symbol_method(receiver, match_symbol)?;
        let constructor_atom = self.intern_atom("constructor");
        let constructor_key = PropertyKey::string(constructor_atom);
        let prototype = self.object_data(receiver)?.proto();
        let constructor_is_intrinsic = self.property_attributes(receiver, constructor_key).is_none()
            && self
                .property_attributes(prototype, constructor_key)
                .is_some_and(|attributes| !attributes.accessor)
            && self.own_property(prototype, constructor_atom) == Some(intrinsic);
        let species = self.well_known_symbols.get(WellKnownSymbol::Species)?;
        let species_is_builtin = self
            .property_attributes(intrinsic, PropertyKey::symbol(species))
            .and_then(|attributes| attributes.accessor.then_some(attributes.getter).flatten())
            .is_some_and(|getter| self.is_native_function(getter, Native::RegExpSpecies));
        (constructor_is_intrinsic && species_is_builtin).then_some(matched)
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
                let species = vm.species_of(p, constructor)?;
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
                    let flags = vm.regexp_flags_text(p, receiver)?;
                    let value = vm.heap.alloc_string(flags.clone());
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
                    Ok(vm.heap.alloc_iterator(IteratorRecord {
                        object: Self::empty_object(vm.regexp_string_iterator_proto),
                        source: matcher,
                        kind: IteratorKind::RegExpStringMatchAll,
                        index: 0,
                        done: false,
                        ext: crate::heap::IteratorExt {
                            position_high: 0,
                            next_method: None,
                            helper: Some(Box::new(IteratorHelper::RegExpStringMatchAll {
                                input,
                                global: flags.contains('g'),
                                unicode: flags.contains('u') || flags.contains('v'),
                            })),
                            helper_running: false,
                            helper_started: false,
                            generator: None,
                        },
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
        // A number is its own primitive and number, so its conversion runs no guest code.
        if let Some(number) = value.as_number() {
            return Ok(regexp_to_length(number));
        }
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
            .get(WellKnownSymbol::Match)
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
                if let Some((matcher, flags)) = vm.regexp_builtin_receiver_matcher(receiver)
                    && flags.contains(RegExpFlags::GLOBAL)
                {
                    return vm.regexp_symbol_match_global_fast(p, receiver, &input, &matcher, flags);
                }
                let flags = vm.regexp_flags_text(p, receiver)?;
                if !flags.host_string().contains('g') {
                    let input = vm.string_value_from(
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                        &input,
                    );
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
                let input_value =
                    vm.string_value_from(args.first().copied().unwrap_or(Value::UNDEFINED), &input);
                vm.with_call_roots([array, input_value], |vm| {
                    loop {
                        let result = vm.regexp_exec_value(p, receiver, input_value)?;
                        if result.is_null() {
                            let Some(elements) = vm.heap.array_elements(array) else {
                                unreachable!("match owns its fresh result array");
                            };
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
                            let matched = vm.heap.alloc_string(matched);
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

    /// Global `RegExp.prototype[@@match]` for a receiver whose exec, flags and lastIndex are
    /// its builtins (`regexp_builtin_receiver_matcher`): those execs run no guest code, so the
    /// matches are collected directly. lastIndex ends at zero and the legacy statics hold
    /// the last match, as the spec's exec loop leaves them.
    fn regexp_symbol_match_global_fast(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        input: &JsString,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
    ) -> Result<Value, JsError> {
        let subject = self.regexp_subject(input, matcher);
        // Only the last match's captures are observable (the legacy statics), so every match
        // keeps its range alone; the statics find the last again from its start if read.
        let ranges = self.regexp_global_matches(
            p,
            receiver,
            input,
            flags,
            |start| matcher.find_in_subject(&subject, start).next().map(|found| found.range),
            Range::clone,
        )?;
        let Some(last) = ranges.last() else {
            return Ok(Value::NULL);
        };
        self.record_regexp_legacy_pending(receiver, input.clone(), matcher, last.start)?;
        let matched = ranges
            .into_iter()
            .map(|range| self.heap.alloc_string(input.substring(range)))
            .collect();
        Ok(self.new_array(matched))
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
                .alloc_string(escape_regexp_source(&meta.source))),
            (Native::RegExpSource, _)
                if self
                    .realm
                    .intrinsics
                    .regexp_intrinsics
                    .get(&self.realm.globals)
                    .is_some_and(|intrinsics| intrinsics.prototype == this) =>
            {
                Ok(self.heap.alloc_string("(?:)".into()))
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
            Some(Cell::RegExp { meta, .. }) => meta.flags,
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
            .position(|(_, accessor, _)| *accessor == native)
            .map(RegExpFlags::at)
            .ok_or_else(|| JsError("invalid RegExp flag accessor".into()))?;
        Ok(if flags.contains(flag) { Value::TRUE } else { Value::FALSE })
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
            Ok(vm.heap.alloc_string(flags.into()))
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
        self.resolve_regexp_legacy_match();
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
                LegacyRegExpMatch::Pending { .. } => unreachable!("resolved above"),
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
                    Some(range.map_or_else(
                        || JsString::from_units(&[]),
                        |range| JsString::from_unit_run(input.units().slice(range)),
                    ))
                }
            }
        };
        match value {
            Some(value) => Ok(self.heap.alloc_string(value)),
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
                    original_flags: flags_omitted.then(|| meta.flags.text()),
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

    pub(super) fn regexp_from_source(
        &mut self,
        prototype: Value,
        source: JsString,
        flags: String,
        legacy_constructor: crate::heap::RegExpLegacyOwner,
    ) -> Result<Value, JsError> {
        let matcher = self.cached_regexp_matcher(&source, &flags, PatternValidity::Unchecked)?;
        let flags = RegExpFlags::validated(&flags);
        self.regexp_from_matcher(prototype, source, flags, matcher, legacy_constructor)
    }

    pub(super) fn regexp_literal(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        site_index: usize,
    ) -> Result<Value, JsError> {
        self.regexp_literal_of(p, self.frames[frame].program, site_index)
    }

    /// The new RegExp object regular expression literal site `site_index` of `p`, the
    /// residual of `program`, evaluates to.
    pub(super) fn regexp_literal_of(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        site_index: usize,
    ) -> Result<Value, JsError> {
        let site = p
            .regexp_literal_sites
            .get(site_index)
            .ok_or_else(|| JsError::validation("RegExp literal site is outside program".into()))?;
        let source = self
            .programs
            .constant(program, site.pattern_constant as usize)
            .and_then(|value| self.heap.string(value).map(|source| source.clone()))
            .ok_or_else(|| JsError::validation("RegExp literal pattern is not a string".into()))?;
        let flags_text = self
            .programs
            .constant(program, site.flags_constant as usize)
            .filter(|value| self.heap.string(*value).is_some())
            .ok_or_else(|| JsError::validation("RegExp literal flags are not a string".into()))?;
        let (prototype, constructor) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .map(|intrinsics| (intrinsics.prototype, intrinsics.constructor))
            .expect("RegExp intrinsics are installed for the active realm");
        let owner = crate::heap::RegExpLegacyOwner::Enabled(constructor);
        let meta = match self.programs.regexp_literal_meta(program, site_index) {
            Some(meta) => meta,
            None => {
                let flags = self.heap.string(flags_text).expect("checked above").host_string().to_owned();
                let meta = self
                    .cached_regexp_matcher(&source, &flags, PatternValidity::EarlyChecked)
                    .map(|matcher| {
                        Rc::new(crate::heap::RegExpMeta {
                            source: source.clone(),
                            flags: RegExpFlags::validated(&flags),
                            legacy_constructor: owner,
                            matcher: crate::heap::RegExpMatcher::compiled(matcher),
                        })
                    })
                    .map_err(|error| Rc::<str>::from(error.to_string()));
                if !self.programs.cache_regexp_literal_meta(program, site_index, meta.clone()) {
                    return Err(JsError::validation(
                        "RegExp literal cache site is invalid".into(),
                    ));
                }
                meta
            }
        }
        .map_err(|message| JsError(message.to_string().into()))?;
        // A site's record names the realm it was first evaluated in; another realm's
        // evaluation owns a record of its own.
        let meta = if meta.legacy_constructor.constructor() == constructor {
            meta
        } else {
            Rc::new(crate::heap::RegExpMeta { legacy_constructor: owner, ..(*meta).clone() })
        };
        self.regexp_from_meta(prototype, meta)
    }

    pub(super) fn regexp_from_matcher(
        &mut self,
        prototype: Value,
        source: JsString,
        flags: RegExpFlags,
        matcher: Rc<quench_regexp::Regex>,
        legacy_constructor: crate::heap::RegExpLegacyOwner,
    ) -> Result<Value, JsError> {
        let matcher = crate::heap::RegExpMatcher::compiled(matcher);
        let meta = Rc::new(crate::heap::RegExpMeta { source, flags, legacy_constructor, matcher });
        self.regexp_from_meta(prototype, meta)
    }

    /// A new RegExp object of `prototype` with the record `meta`.
    fn regexp_from_meta(
        &mut self,
        prototype: Value,
        meta: Rc<crate::heap::RegExpMeta>,
    ) -> Result<Value, JsError> {
        let object = self.heap.alloc(Cell::RegExp {
            object: Box::new(Self::empty_object(prototype)),
            meta,
        });
        // Every new RegExp object starts in the same shape, so the `lastIndex` definition the
        // first one recorded applies again.
        let start = self.object_data(object).map(Object::shape);
        if let Some((source, target)) = self.regexp_object_shapes
            && start == Some(source)
        {
            self.heap.property_add(object, Value::number(0.0), target);
            return Ok(object);
        }
        let last_index_atom = self.intern_atom("lastIndex");
        self.set_property_with_attributes(
            object,
            last_index_atom,
            Value::number(0.0),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        )?;
        if let Some(source) = start
            && let Some(target) = self.object_data(object).map(Object::shape)
            && !self.shape_is_dictionary(source)
            && !self.shape_is_dictionary(target)
        {
            self.regexp_object_shapes = Some((source, target));
        }
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
                    (meta.source.clone(), meta.flags.text())
                } else {
                    vm.regexp_initialization_strings(p, pattern, flags)?
                };

                let matcher = match vm.cached_regexp_matcher(&source, &flags, PatternValidity::Unchecked) {
                    Ok(matcher) => matcher,
                    Err(error) => {
                        let message = error.to_string();
                        let message = message.strip_prefix("SyntaxError: ").unwrap_or(&message);
                        return vm.syntax_error_result(p, message).map(|_| Value::UNDEFINED);
                    }
                };
                let Some(Cell::RegExp { meta, .. }) = vm.heap.get_mut(receiver) else {
                    unreachable!("RegExp receiver slot was validated before compilation")
                };
                // The record may be a literal site's, shared with its other objects.
                let meta = Rc::make_mut(meta);
                meta.source = source;
                meta.flags = RegExpFlags::validated(&flags);
                meta.matcher = crate::heap::RegExpMatcher::compiled(matcher);
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
            source.units().extend_into(&mut output);
            output.push(REGEXP_DELIMITER);
            flags.units().extend_into(&mut output);
            vm.string_from_units(&output)
        })
    }

    pub(super) fn regexp_escape_native(
        &mut self,
        program: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let value = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(input) = self.heap.string(value) else {
            return Err(self.type_error(program, "RegExp.escape requires a string value".into()));
        };
        Ok(self.heap.alloc_string(escape_regexp_string(&input)))
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
                // With the builtin exec, only the match and its effects are observable: the
                // result array it would build never escapes.
                if vm.regexp_builtin_exec_matcher(receiver).is_some() {
                    let matched = vm.regexp_builtin_match(p, receiver, &input)?;
                    let found = matched.is_some();
                    if let Some(matched) = matched {
                        vm.record_regexp_legacy_match(receiver, input, matched)?;
                    }
                    return Ok(if found { Value::TRUE } else { Value::FALSE });
                }
                let input =
                    vm.string_value_from(args.first().copied().unwrap_or(Value::UNDEFINED), &input);
                let result = vm.regexp_exec_value(p, receiver, input)?;
                Ok(if result.is_null() {
                    Value::FALSE
                } else {
                    Value::TRUE
                })
            },
        )
    }

    /// `RegExpBuiltinExec` steps 2–15 for the RegExp `this`: reads and updates `lastIndex`
    /// and matches `input`, leaving the result's construction to the caller.
    fn regexp_builtin_match(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        input: &JsString,
    ) -> Result<Option<quench_regexp::Match>, JsError> {
        let last_index = self.regexp_last_index(p, this)?;
        let last_index = self.regexp_to_length_value(p, last_index)?;
        self.regexp_match_from(p, this, input, last_index, None)
    }

    /// `regexp_builtin_match` past its `lastIndex` read: the match from `last_index` where
    /// the flags make it stateful, storing the new `lastIndex` in `known_slot` when the
    /// caller found its own writable data slot (`regexp_last_index_slot`) and no method
    /// cache can observe the store, else as `store_regexp_last_index` does.
    fn regexp_match_from(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        input: &JsString,
        last_index: usize,
        known_slot: Option<usize>,
    ) -> Result<Option<quench_regexp::Match>, JsError> {
        let (regex, stateful, sticky) = match self.heap.get(this) {
            Some(Cell::RegExp { meta, .. }) => (
                Self::regexp_matcher(meta)?,
                meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY)),
                meta.flags.contains(RegExpFlags::STICKY),
            ),
            _ => {
                return Err(
                    self.type_error(p, "RegExp method called on incompatible receiver".into())
                );
            }
        };
        let start = if stateful { last_index } else { 0 };
        let store = |vm: &mut Self, value: Value| match known_slot {
            Some(slot) => {
                vm.heap.property_set(this, slot, value);
                Ok(())
            }
            None => vm.store_regexp_last_index(p, this, value),
        };
        let units = input.units();
        if (stateful && start > units.len())
            || regex.excludes_search(units.len(), start, (start < units.len()).then(|| units.at(start)))
        {
            if stateful {
                store(self, Value::number(0.0))?;
            }
            return Ok(None);
        }
        let subject = self.regexp_subject(input, &regex);
        let matched = if sticky {
            // A start inside a surrogate pair resolves to the next code point, which a
            // sticky match may not move to.
            regex
                .find_sticky_in_subject(&subject, start)
                .next()
                .filter(|matched| matched.range.start == start)
        } else {
            regex.find_in_subject(&subject, start).next()
        };
        if stateful {
            let last_index = matched.as_ref().map_or(0, |matched| matched.range.end);
            store(self, Value::number(last_index as f64))?;
        }
        Ok(matched)
    }

    /// `RegExp.prototype.exec` or `test` when the call runs no guest code and raises
    /// nothing, so it observes no frame (`lane_regexp_call`): a RegExp receiver, a string
    /// subject, an own writable data `lastIndex` holding a Number (`regexp_last_index_slot`,
    /// so its conversion and any store run no guest code), and for `test` the builtin `exec`
    /// (`regexp_builtin_exec_matcher`). The call is the native's own; `None` leaves any other
    /// call to the general path.
    pub(super) fn steady_regexp_call(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        subject: Value,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        if !matches!(native, Native::RegExpExec | Native::RegExpTest)
            || !matches!(self.heap.get(receiver), Some(Cell::RegExp { .. }))
            || !self.is_string(subject)
            || self.method_caches_may_name(self.runtime_atoms.last_index)
        {
            return None;
        }
        let (slot, last_index) = self.regexp_last_index_value(receiver)?;
        if last_index.as_number().is_none()
            || (native == Native::RegExpTest && self.regexp_builtin_exec_matcher(receiver).is_none())
        {
            return None;
        }
        // A search the prefilter excludes fails as the matcher's would: `null` from exec and
        // `false` from test, and a global or sticky regexp's `lastIndex` reset through the
        // slot `regexp_match_from` stores through.
        if let Some(stateful) = self.regexp_prefilter_rejects(receiver, subject, last_index) {
            if stateful {
                self.heap.property_set(receiver, slot, Value::number(0.0));
            }
            return Some(Ok(if native == Native::RegExpExec { Value::NULL } else { Value::FALSE }));
        }
        let arguments = [subject];
        if native == Native::RegExpExec {
            return Some(self.regexp_steady_exec(p, receiver, subject, slot, last_index, result_used));
        }
        Some(self.regexp_test(p, receiver, &arguments))
    }

    /// `String.prototype.replace(regexp, string)` when the call runs no guest code, so it
    /// observes no frame (`lane_regexp_call`): a string receiver and replacement, and a
    /// RegExp whose `@@replace` is this realm's builtin (`regexp_proven_symbol_method`) and
    /// whose exec, flags and lastIndex are builtin (`regexp_builtin_receiver_matcher`).
    /// Those make the native's steps the builtin `@@replace`'s fast path, which this runs;
    /// `None` leaves any other call to the general path.
    pub(super) fn steady_string_replace(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        arguments: &[Value],
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        let [search, replacement] = *arguments.get(..2)?.as_array::<2>()?;
        if native != Native::StringReplace
            || !self.is_string(this)
            || !self.is_string(replacement)
        {
            return None;
        }
        let symbol = self.well_known_symbols.get(WellKnownSymbol::Replace)?;
        let method = self.regexp_proven_symbol_method(search, symbol)?;
        if !matches!(
            self.heap.function(method),
            Some(FunctionRecord { kind: FunctionKind::Native(Native::RegExpSymbolReplace), realm, .. })
                if *realm == self.realm.globals
        ) {
            return None;
        }
        let (matcher, flags) = self.regexp_builtin_receiver_matcher(search)?;
        let input = self.heap.string(this)?.clone();
        let template = self.heap.string(replacement)?.clone();
        Some(self.with_call_roots([this, search, replacement], |vm| {
            vm.regexp_symbol_replace_fast(
                p,
                search,
                &input,
                this,
                FastReplacement::Template(&template),
                &matcher,
                flags,
                result_used,
            )
        }))
    }

    /// `regexp_builtin_exec` of `steady_regexp_call`'s case: a string subject, whose
    /// conversion runs no guest code, and `lastIndex` the Number in the own data `slot`, so
    /// neither its conversion nor the store runs guest code or allocates. A failed match is
    /// `null` with nothing to root; a match builds its result as the builtin does.
    fn regexp_steady_exec(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        subject: Value,
        slot: usize,
        last_index: Value,
        result_used: bool,
    ) -> Result<Value, JsError> {
        let input = self.coerce_js_string(p, subject)?;
        let last_index = self.regexp_to_length_value(p, last_index)?;
        let Some(matched) = self.regexp_match_from(p, receiver, &input, last_index, Some(slot))? else {
            return Ok(Value::NULL);
        };
        // Nothing reads the result: building it allocates and runs no guest code, so only
        // the match's own effect, the legacy statics, remains.
        if !result_used {
            self.record_regexp_legacy_match(receiver, input, matched)?;
            return Ok(Value::UNDEFINED);
        }
        self.with_call_roots([receiver, subject], |vm| vm.regexp_exec_result(receiver, subject, input, matched))
    }

    /// Whether `regexp_match_from`'s search of `subject` from `last_index` is one its
    /// compiled regexp's prefilter excludes (`Regex::excludes_search`), decided where neither
    /// conversion has an effect: a flat string subject and a Number `lastIndex`. `Some`
    /// carries whether the regexp is global or sticky; `None` leaves the read to the general
    /// path.
    fn regexp_prefilter_rejects(&self, receiver: Value, subject: Value, last_index: Value) -> Option<bool> {
        let Some(Cell::RegExp { meta, .. }) = self.heap.get(receiver) else {
            return None;
        };
        let regex = meta.matcher.0.get()?;
        let input = self.heap.string(subject)?;
        let units = input.existing_flat()?;
        let last_index = regexp_to_length(last_index.as_number()?);
        let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
        let start = if stateful { last_index } else { 0 };
        let length = input.len();
        ((stateful && start > length) || regex.excludes_search(length, start, units.unit(start)))
            .then_some(stateful)
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
            let Some(matched) = vm.regexp_builtin_match(p, this, &input)? else {
                return Ok(Value::NULL);
            };
            vm.regexp_exec_result(this, args.first().copied().unwrap_or(Value::UNDEFINED), input, matched)
        })
    }

    /// The array `RegExp.prototype.exec` returns for `matched` of regexp `this` over `input`,
    /// the string its `argument` converted to, which the caller roots with `this`; it records
    /// the match as the legacy RegExp statics' last.
    fn regexp_exec_result(
        &mut self,
        this: Value,
        argument: Value,
        input: JsString,
        matched: quench_regexp::Match,
    ) -> Result<Value, JsError> {
        {
            let vm = self;
            let has_indices = match vm.heap.get(this) {
                Some(Cell::RegExp { meta, .. }) => meta.flags.contains(RegExpFlags::HAS_INDICES),
                _ => false,
            };
            // Each matched substring is copied out (`JsString::from_unit_run`): captures are
            // short, and a copy measured cheaper than `JsString::substring`'s sharing
            // (V8-v7 RegExp Score run, callgrind: 50 against 75 instructions a capture).
            let values = std::iter::once(Some(matched.range.clone()))
                .chain(matched.captures.iter().cloned())
                .map(|range| {
                    range.map_or(Value::UNDEFINED, |range| {
                        vm.heap.alloc_string(JsString::from_unit_run(input.units().slice(range)))
                    })
                })
                .collect::<Vec<_>>();
            let named = regexp_named_capture_ranges(&matched);
            // The groups object is made before the result array: no guest code runs and no
            // collection is taken between them, so the order is unobservable.
            let groups = vm.regexp_groups_object(&named, input.units())?;
            let index = Value::number(matched.range.start as f64);
            let input_value = vm.string_value_from(argument, &input);
            let array = crate::heap::ArrayObject::new(Self::empty_object(vm.array_proto), values);
            let result = match vm.regexp_result_layout() {
                // The recorded layout: the array is allocated with its properties, by the
                // same storage steps `initialize_regexp_result` takes on the published cell.
                Some((shape, properties)) => vm.heap.alloc_array_with_properties(
                    array,
                    shape,
                    &properties.values(index, input_value, groups),
                ),
                None => {
                    let result = vm.heap.alloc(Cell::Array { object: array });
                    vm.initialize_regexp_result(result, index, input_value, groups)?;
                    result
                }
            };
            if has_indices {
                let indices = vm.regexp_indices_array(&matched, &named)?;
                let indices_atom = vm.intern_atom("indices");
                vm.set_property(result, indices_atom, indices)?;
            }
            vm.record_regexp_legacy_match(this, input, matched)?;
            Ok(result)
        }
    }

    /// The realm's recorded exec result layout (`RegExpResultTemplate`): the shape its
    /// arrays take and where each of `index`, `input` and `groups` sits.
    fn regexp_result_layout(&self) -> Option<(u32, RegExpResultSlots)> {
        let template = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)?
            .result_template?;
        let shape = self.object_data(template.anchor)?.shape();
        Some((
            shape,
            RegExpResultSlots {
                index: template.index_slot,
                input: template.input_slot,
                groups: template.groups_slot,
            },
        ))
    }

    /// Gives a fresh exec `result` array its `index`, `input` and `groups` properties: from
    /// the realm's recorded layout when there is one, else by ordinary property creation,
    /// whose resulting layout is then recorded.
    fn initialize_regexp_result(
        &mut self,
        result: Value,
        index: Value,
        input: Value,
        groups: Value,
    ) -> Result<(), JsError> {
        let globals = self.realm.globals;
        let template = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&globals)
            .and_then(|intrinsics| intrinsics.result_template);
        if let Some(template) = template {
            let shape = self
                .object_data(template.anchor)
                .expect("RegExp result template anchor is an object")
                .shape();
            let mut properties = [Value::UNDEFINED; REGEXP_RESULT_PROPERTY_COUNT];
            properties[template.index_slot] = index;
            properties[template.input_slot] = input;
            properties[template.groups_slot] = groups;
            self.heap
                .initialize_object_properties(result, shape, &properties);
            return Ok(());
        }
        let atoms = [
            self.intern_atom("index"),
            self.intern_atom("input"),
            self.intern_atom("groups"),
        ];
        for (atom, value) in atoms.into_iter().zip([index, input, groups]) {
            self.set_property(result, atom, value)?;
        }
        let slots = atoms.map(|atom| self.object_property_slot(result, PropertyKey::string(atom)));
        let [
            Some((shape, index_slot)),
            Some((_, input_slot)),
            Some((_, groups_slot)),
        ] = slots
        else {
            return Ok(());
        };
        if self.shapes[shape as usize].storage_len() != REGEXP_RESULT_PROPERTY_COUNT {
            return Ok(());
        }
        let proto = self
            .object_data(result)
            .expect("RegExp result is an array")
            .proto();
        let anchor = self.heap.alloc_object_with_properties(
            proto,
            shape,
            &[Value::UNDEFINED; REGEXP_RESULT_PROPERTY_COUNT],
        );
        if let Some(intrinsics) = self.realm.intrinsics.regexp_intrinsics.get_mut(&globals) {
            intrinsics.result_template = Some(RegExpResultTemplate {
                anchor,
                index_slot,
                input_slot,
                groups_slot,
            });
        }
        Ok(())
    }

    /// Finds a pending legacy match (`LegacyRegExpMatch::Pending`) again, so the statics
    /// read its captures.
    fn resolve_regexp_legacy_match(&mut self) {
        let Some(LegacyRegExpMatch::Pending { input, matcher, start }) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .map(|state| state.legacy_match.clone())
        else {
            return;
        };
        let subject = self.regexp_subject(&input, &matcher);
        let matched = matcher
            .find_in_subject(&subject, start)
            .next()
            .expect("a pending legacy match begins at its start");
        if let Some(state) = self.realm.intrinsics.regexp_intrinsics.get_mut(&self.realm.globals) {
            state.legacy_match = LegacyRegExpMatch::Matched { input, matched };
        }
    }

    /// The legacy `RegExp` static state a successful builtin exec of `this` leaves behind.
    fn record_regexp_legacy_match(
        &mut self,
        this: Value,
        input: JsString,
        matched: quench_regexp::Match,
    ) -> Result<(), JsError> {
        self.record_regexp_legacy(this, input, |input| LegacyRegExpMatch::Matched { input, matched })
    }

    /// `record_regexp_legacy_match` for the match `matcher` finds from `start`, found again
    /// only if a static is read.
    fn record_regexp_legacy_pending(
        &mut self,
        this: Value,
        input: JsString,
        matcher: &Rc<quench_regexp::Regex>,
        start: usize,
    ) -> Result<(), JsError> {
        let matcher = Rc::clone(matcher);
        self.record_regexp_legacy(this, input, |input| LegacyRegExpMatch::Pending { input, matcher, start })
    }

    fn record_regexp_legacy(
        &mut self,
        this: Value,
        input: JsString,
        matched: impl FnOnce(JsString) -> LegacyRegExpMatch,
    ) -> Result<(), JsError> {
        let owner = match self.heap.get(this) {
            Some(Cell::RegExp { meta, .. }) => meta.legacy_constructor,
            _ => return Err(JsError("RegExp instance unavailable after match".into())),
        };
        let state = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get_mut(&self.realm.globals)
            .expect("active realm has RegExp intrinsics");
        // Constructors are objects, whose SameValue is identity.
        if owner.constructor() == state.constructor {
            match owner {
                crate::heap::RegExpLegacyOwner::Enabled(_) => {
                    state.legacy_input = Some(input.clone());
                    state.legacy_match = matched(input);
                }
                crate::heap::RegExpLegacyOwner::Disabled(_) => {
                    state.legacy_input = None;
                    state.legacy_match = LegacyRegExpMatch::Invalid;
                }
            }
        }
        Ok(())
    }

    pub(super) fn regexp_groups_object(
        &mut self,
        named: &[(String, Option<Range<usize>>)],
        input: super::wtf16::Units<'_>,
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
                self.heap.alloc_string(JsString::from_unit_run(
                    input.slice(range.clone()),
                ))
            });
            self.set_property(groups, atom, value)?;
        }
        Ok(groups)
    }

    fn regexp_indices_array(
        &mut self,
        matched: &quench_regexp::Match,
        named: &[(String, Option<Range<usize>>)],
    ) -> Result<Value, JsError> {
        let ranges = std::iter::once(Some(matched.range.clone()))
            .chain(matched.captures.iter().cloned())
            .collect::<Vec<_>>();
        let entries = ranges
            .into_iter()
            .map(|range| self.regexp_index_pair(range))
            .collect::<Vec<_>>();
        let indices = self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                entries,
            ),
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

    fn regexp_index_pair(&mut self, range: Option<Range<usize>>) -> Value {
        let Some(range) = range else {
            return Value::UNDEFINED;
        };
        let pair = [
            Value::number(range.start as f64),
            Value::number(range.end as f64),
        ];
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(
                Self::empty_object(self.array_proto),
                pair,
            ),
        })
    }

    /// `meta`'s matcher, compiled now if it has none (`RegExpMatcher`): the pattern compiled
    /// when the record was first made, so it compiles again.
    pub(super) fn regexp_matcher(meta: &crate::heap::RegExpMeta) -> Result<Rc<quench_regexp::Regex>, JsError> {
        if let Some(matcher) = meta.matcher.0.get() {
            return Ok(Rc::clone(matcher));
        }
        let matcher = Rc::new(Self::compile_regexp(&meta.source, &meta.flags.text(), PatternValidity::EarlyChecked)?);
        Ok(Rc::clone(meta.matcher.0.get_or_init(|| matcher)))
    }

    pub(super) fn compile_regexp(
        source: &JsString,
        flags: &str,
        pattern: PatternValidity,
    ) -> Result<quench_regexp::Regex, JsError> {
        quench_regexp::validate_flags(flags)
            .map_err(|error| JsError(format!("SyntaxError: {error}").into()))?;
        let parser_source =
            regexp_parser_source(source, flags.contains('u') || flags.contains('v'))?;
        // The matcher's compilation parses the pattern with the checks
        // `validate_property_escapes` would parse it for.
        if pattern == PatternValidity::Unchecked {
            crate::compile::regexp::validate_pattern_text(&parser_source, flags)
                .map_err(|error| JsError(format!("SyntaxError: {error}").into()))?;
        }
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
        char::decode_utf16(source.units().iter()).any(|character| character.is_err())
    } else {
        source
            .units()
            .iter()
            .any(|unit| (HIGH_SURROGATE_START..=LOW_SURROGATE_END).contains(&unit))
    };
    if !needs_projection {
        return Ok(std::borrow::Cow::Borrowed(source.host_string()));
    }
    use std::fmt::Write;
    let mut projected = String::with_capacity(source.host_string().len());
    let mut escaped = false;
    for character in char::decode_utf16(source.units().iter()) {
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
    for unit in source.units().iter() {
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

pub(super) fn advance_string_index_units(
    input: super::wtf16::Units<'_>,
    index: usize,
    unicode: bool,
) -> usize {
    if unicode
        && input.get(index).is_some_and(|unit| {
            (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(&unit)
                && input
                    .get(index + 1)
                    .is_some_and(|next| (LOW_SURROGATE_START..=LOW_SURROGATE_END).contains(&next))
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
) -> Vec<(String, Option<Range<usize>>)> {
    let mut groups: Vec<(String, Option<Range<usize>>)> = Vec::new();
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
        if (HIGH_SURROGATE_START..=HIGH_SURROGATE_END).contains(&unit)
            && units
                .get(index + 1)
                .is_some_and(|next| (LOW_SURROGATE_START..=LOW_SURROGATE_END).contains(&next))
        {
            let scalar = 0x1_0000
                + ((u32::from(unit) - u32::from(HIGH_SURROGATE_START)) << 10)
                + u32::from(units.at(index + 1))
                - u32::from(LOW_SURROGATE_START);
            if let Some(character) = char::from_u32(scalar) {
                escape_regexp_character(&mut escaped, character, index == 0);
            }
            index += 2;
        } else if (HIGH_SURROGATE_START..=LOW_SURROGATE_END).contains(&unit) {
            escaped.push_str(&format!("\\u{unit:04x}"));
            index += 1;
        } else {
            if let Some(character) = char::from_u32(u32::from(unit)) {
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

/// A RegExp's flags (its [[OriginalFlags]]) as the set of letters they name. A valid flags
/// string names each letter of `REGEXP_FLAG_ACCESSORS` at most once and nothing observes the
/// order it was written in, so the set is the whole fact: bit `index` is the letter of
/// accessor `index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RegExpFlags(u8);

const _: () = assert!(REGEXP_FLAG_ACCESSORS.len() <= u8::BITS as usize);

impl RegExpFlags {
    pub(crate) const HAS_INDICES: Self = Self::letter('d');
    pub(crate) const GLOBAL: Self = Self::letter('g');
    pub(crate) const STICKY: Self = Self::letter('y');
    /// Either Unicode mode: `u`, or `v` (unicodeSets).
    const UNICODE_MODES: Self = Self::letter('u').with(Self::letter('v'));

    /// The flag of `REGEXP_FLAG_ACCESSORS[index]`.
    pub(crate) const fn at(index: usize) -> Self {
        Self(1 << index)
    }

    const fn letter(letter: char) -> Self {
        let mut index = 0;
        while index < REGEXP_FLAG_ACCESSORS.len() {
            if REGEXP_FLAG_ACCESSORS[index].2 == letter {
                return Self::at(index);
            }
            index += 1;
        }
        panic!("not a RegExp flag letter")
    }

    /// The set a flags string names, or `None` when it names an unknown letter or one twice.
    pub(crate) fn of(text: &str) -> Option<Self> {
        text.chars().try_fold(Self(0), |flags, letter| {
            let index = REGEXP_FLAG_ACCESSORS
                .iter()
                .position(|(_, _, flag)| *flag == letter)?;
            (!flags.contains(Self::at(index))).then(|| flags.with(Self::at(index)))
        })
    }

    /// The set of flags a compiled matcher was built from: compilation validated `text`.
    pub(crate) fn validated(text: &str) -> Self {
        Self::of(text).expect("compiled RegExp flags are valid")
    }

    pub(crate) const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub(crate) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub(crate) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    pub(crate) const fn unicode_mode(self) -> bool {
        self.intersects(Self::UNICODE_MODES)
    }

    /// The flags' letters in accessor order, the order the `flags` getter spells them.
    pub(crate) fn text(self) -> String {
        REGEXP_FLAG_ACCESSORS
            .iter()
            .enumerate()
            .filter(|(index, _)| self.contains(Self::at(*index)))
            .map(|(_, (_, _, letter))| *letter)
            .collect()
    }
}

pub(super) const REGEXP_FLAG_ACCESSORS: &[(&str, Native, char)] = &[
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

impl crate::snapshot::Snap for RegExpIntrinsics {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        let Self { constructor, prototype, legacy_input, legacy_match, result_template } = self;
        writer.value(*constructor);
        writer.value(*prototype);
        writer.bool(legacy_input.is_some());
        if let Some(input) = legacy_input {
            input.write_snapshot(writer);
        }
        writer.bool(match legacy_match {
            LegacyRegExpMatch::Empty => false,
            LegacyRegExpMatch::Invalid => true,
            LegacyRegExpMatch::Matched { .. } | LegacyRegExpMatch::Pending { .. } => {
                return Err(crate::snapshot::SnapshotError::unsupported("legacy RegExp match data"));
            }
        });
        writer.bool(result_template.is_some());
        if let Some(RegExpResultTemplate { anchor, index_slot, input_slot, groups_slot }) = result_template {
            writer.value(*anchor);
            writer.len(*index_slot);
            writer.len(*input_slot);
            writer.len(*groups_slot);
        }
        Ok(())
    }

    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let constructor = reader.value()?;
        let prototype = reader.value()?;
        let legacy_input = if reader.bool()? { Some(JsString::read_snapshot(reader)?) } else { None };
        let legacy_match = if reader.bool()? { LegacyRegExpMatch::Invalid } else { LegacyRegExpMatch::Empty };
        let result_template = if reader.bool()? {
            Some(RegExpResultTemplate {
                anchor: reader.value()?,
                index_slot: reader.len()?,
                input_slot: reader.len()?,
                groups_slot: reader.len()?,
            })
        } else {
            None
        };
        Ok(Self { constructor, prototype, legacy_input, legacy_match, result_template })
    }
}

/// What is known of a pattern's validity when its matcher is compiled. A literal's pattern
/// passed its program's early errors (`compile::early::regexp_early_error`), and a snapshot's
/// was compiled once already, so neither is validated again; any other pattern is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PatternValidity {
    EarlyChecked,
    Unchecked,
}
