//! The legacy `RegExp` statics (`RegExp.$1`, `RegExp.input`, `RegExp.lastMatch`, ...): what
//! the last builtin match of the realm records, found again only when a static is read.
use super::*;

/// The legacy `RegExp` statics: the last match, which holds the subject it was found in, and
/// the `RegExp.input` a script set since, which reads in place of that subject.
#[derive(Clone)]
pub(super) struct LegacyStatics {
    pub(super) last: LegacyRegExpMatch,
    pub(super) input_override: Option<JsString>,
}

impl LegacyStatics {
    pub(super) const EMPTY: Self = Self { last: LegacyRegExpMatch::Empty, input_override: None };

    /// What `RegExp.input` reads, or `None` after a match of a subclass or other non-builtin
    /// that poisoned the statics and no script has set it since.
    pub(super) fn input(&self) -> Option<JsString> {
        self.input_override.clone().or_else(|| match &self.last {
            LegacyRegExpMatch::Empty => Some(JsString::from_units(&[])),
            LegacyRegExpMatch::Matched { input, .. } | LegacyRegExpMatch::Pending { input, .. } => Some(input.clone()),
            LegacyRegExpMatch::Invalid => None,
        })
    }
}

#[derive(Clone)]
pub(super) enum LegacyRegExpMatch {
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
        /// Where `input`'s ASCII bytes live and how many, where they are a shared allocation
        /// (`pending_storage`): held by `input`, so no other text has them while this is.
        storage: Option<(usize, usize)>,
    },
    Invalid,
}

/// Legacy match data belongs to the embedding caller, not host builtin setup.
/// These owned strings/match ranges contain no heap references and survive GC.
pub(in crate::vm) struct LegacyRegExpSnapshot(LegacyStatics);

impl Vm {
    pub(in crate::vm) fn snapshot_regexp_legacy(&self) -> LegacyRegExpSnapshot {
        match self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
        {
            Some(state) => LegacyRegExpSnapshot(state.legacy.clone()),
            None => LegacyRegExpSnapshot(LegacyStatics::EMPTY),
        }
    }

    pub(in crate::vm) fn restore_regexp_legacy(&mut self, snapshot: LegacyRegExpSnapshot) {
        if let Some(state) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get_mut(&self.realm.globals)
        {
            state.legacy = snapshot.0;
        }
    }
}

impl Vm {
    pub(in crate::vm) fn install_regexp_legacy_accessors(
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

    pub(in crate::vm) fn install_regexp_legacy_accessor(
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

    pub(in crate::vm) fn regexp_legacy_getter_native(
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
        match self.regexp_legacy_text(selector) {
            Some(value) => Ok(self.heap.alloc_string(value)),
            None => Err(self.type_error(p, "RegExp legacy match state is unavailable".into())),
        }
    }

    /// A legacy static getter of the intrinsic `RegExp` (`RegExp.$1`, `lastMatch`) called as
    /// `callee` with `receiver`, which runs no guest code: its text, where the receiver is the
    /// realm's constructor and the match state is available; `None` leaves any throw to the
    /// native.
    pub(in crate::vm) fn regexp_legacy_getter_in_place(&mut self, callee: Value, receiver: Value) -> Option<Value> {
        let record = self.heap.function(callee)?;
        let env = record.env;
        if self.record_realm(record) != self.realm.globals || !self.same_value(receiver, self.regexp_intrinsic_constructor()) {
            return None;
        }
        let selector = env.as_number()? as Atom;
        let text = self.regexp_legacy_text(selector)?;
        Some(self.heap.alloc_string(text))
    }

    /// The text the legacy static `selector` names, from the last match the realm made;
    /// `None` where the state is unavailable.
    pub(in crate::vm) fn regexp_legacy_text(&mut self, selector: Atom) -> Option<JsString> {
        self.resolve_regexp_legacy_match();
        let name = self.atom_name(selector);
        let state = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .expect("active realm has RegExp intrinsics");
        if matches!(name, "input" | "$_") {
            state.legacy.input()
        } else {
            match &state.legacy.last {
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
        }
    }

    pub(in crate::vm) fn regexp_legacy_setter_native(
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
            .legacy
            .input_override = Some(value);
        Ok(Value::UNDEFINED)
    }

    /// Finds a pending legacy match (`LegacyRegExpMatch::Pending`) again, so the statics
    /// read its captures.
    pub(in crate::vm) fn resolve_regexp_legacy_match(&mut self) {
        let Some(LegacyRegExpMatch::Pending { input, matcher, start, .. }) = self
            .realm
            .intrinsics
            .regexp_intrinsics
            .get(&self.realm.globals)
            .map(|state| state.legacy.last.clone())
        else {
            return;
        };
        let subject = self.regexp_subject(&input, &matcher);
        let matched = matcher
            .find_in_subject(&subject, start)
            .next()
            .expect("a pending legacy match begins at its start");
        if let Some(state) = self.realm.intrinsics.regexp_intrinsics.get_mut(&self.realm.globals) {
            state.legacy.last = LegacyRegExpMatch::Matched { input, matched };
        }
    }

    /// The legacy statics' owner of the RegExp `this`, read from its cell: a caller that
    /// records a match of `this` but holds no owner reads it once, here.
    pub(in crate::vm) fn regexp_legacy_owner(&self, this: Value) -> Result<crate::heap::RegExpLegacyOwner, JsError> {
        match self.heap.get(this) {
            Some(Cell::RegExp { meta, .. }) => Ok(meta.legacy_constructor),
            _ => Err(JsError("RegExp instance unavailable after match".into())),
        }
    }

    /// The legacy `RegExp` static state a successful builtin exec leaves behind, for a RegExp
    /// whose legacy owner is `owner`.
    pub(in crate::vm) fn record_regexp_legacy_match(
        &mut self,
        owner: crate::heap::RegExpLegacyOwner,
        input: JsString,
        matched: quench_regexp::Match,
    ) {
        self.record_regexp_legacy(owner, input, |input| LegacyRegExpMatch::Matched { input, matched });
    }

    /// `record_regexp_legacy_match` for the match `matcher` finds from `start`, found again
    /// only if a static is read.
    pub(in crate::vm) fn record_regexp_legacy_pending(
        &mut self,
        owner: crate::heap::RegExpLegacyOwner,
        input: JsString,
        matcher: Rc<quench_regexp::Regex>,
        start: usize,
    ) {
        let storage = pending_storage(&input);
        self.record_regexp_legacy(owner, input, |input| LegacyRegExpMatch::Pending { input, matcher, start, storage });
    }

    /// Records `matched` over `input` as the legacy statics' state, when the RegExp's owner
    /// `owner` is the active realm's `RegExp` constructor (or a disabled one clears them).
    pub(super) fn record_regexp_legacy(
        &mut self,
        owner: crate::heap::RegExpLegacyOwner,
        input: JsString,
        matched: impl FnOnce(JsString) -> LegacyRegExpMatch,
    ) {
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
                    let previous = std::mem::replace(&mut state.legacy, LegacyStatics { last: matched(input), input_override: None });
                    if let LegacyRegExpMatch::Matched { matched, .. } = previous.last {
                        matched.recycle();
                    }
                }
                crate::heap::RegExpLegacyOwner::Disabled(_) => {
                    state.legacy = LegacyStatics { last: LegacyRegExpMatch::Invalid, input_override: None };
                }
            }
        }
    }
}

impl RegExpIntrinsics {
    /// Whether this realm's legacy statics already hold the pending match that
    /// `record_regexp_legacy_pending` would record for the RegExp of `meta` over the ASCII
    /// `bytes`: the same matcher over the same input storage from the same start, as a loop
    /// testing one regexp on one string records. Read from the record a steady call already
    /// looked up (`Vm::steady_ascii_plan`), with no second lookup of the realm's.
    #[inline(always)]
    pub(super) fn legacy_pending_holds(&self, meta: &crate::heap::RegExpMeta, bytes: &[u8], matcher: &Rc<quench_regexp::Regex>, start: usize) -> bool {
        let LegacyRegExpMatch::Pending { input: held, matcher: held_matcher, start: held_start, storage } = &self.legacy.last else {
            return false;
        };
        let enabled = matches!(meta.legacy_constructor, crate::heap::RegExpLegacyOwner::Enabled(owner) if owner == self.constructor);
        if !enabled || self.legacy.input_override.is_some() || *held_start != start || !Rc::ptr_eq(held_matcher, matcher) {
            return false;
        }
        match storage {
            Some(storage) => *storage == (bytes.as_ptr() as usize, bytes.len()),
            // Other text, short text above all, is the same input where its units are the same.
            None => matches!(held.existing_units(), Some(super::wtf16::Units::Ascii(held)) if held == bytes),
        }
    }
}

/// Where the ASCII bytes of `input` live and how many, where they are an immutable shared
/// allocation (`FlatUnits::Ascii`, `AsciiSlice`) that `input` keeps: `None` for any other text.
fn pending_storage(input: &JsString) -> Option<(usize, usize)> {
    let flat = input.existing_flat()?;
    if !matches!(flat, FlatUnits::Ascii(_) | FlatUnits::AsciiSlice(_)) {
        return None;
    }
    match flat.view() {
        super::wtf16::Units::Ascii(bytes) => Some((bytes.as_ptr() as usize, bytes.len())),
        super::wtf16::Units::Latin1(_) | super::wtf16::Units::Wide(_) => None,
    }
}
