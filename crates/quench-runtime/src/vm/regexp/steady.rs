//! RegExp calls made in place, in the lane, when they run no guest code and raise nothing.
use super::*;

impl<H: Host> Vm<H> {
    /// `RegExp.prototype.exec` or `test` when the call runs no guest code and raises
    /// nothing, so it observes no frame (`lane_regexp_call`): a RegExp receiver, a string
    /// subject, an own writable data `lastIndex` holding a Number (`regexp_last_index_slot`,
    /// so its conversion and any store run no guest code), and for `test` the builtin `exec`
    /// (`regexp_builtin_exec_matcher`). The call is the native's own; `None` leaves any other
    /// call to the general path.
    pub(in crate::vm) fn steady_regexp_call(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        receiver: Value,
        subject: Value,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        if !matches!(native, Native::RegExpExec | Native::RegExpTest)
            || self.method_caches_may_name(self.runtime_atoms.last_index)
        {
            return None;
        }
        let (slot, last_index, prefilter) = self.regexp_steady_state(receiver, subject)?;
        if native == Native::RegExpTest && self.regexp_builtin_exec_matcher(receiver).is_none() {
            return None;
        }
        // A search the prefilter excludes fails as the matcher's would: `null` from exec and
        // `false` from test, and a global or sticky regexp's `lastIndex` reset through the
        // slot `regexp_match_from` stores through.
        if let Prefilter::Excludes { stateful } = prefilter {
            if stateful {
                self.heap.property_set(receiver, slot, Value::number(0.0));
            }
            return Some(Ok(if native == Native::RegExpExec { Value::NULL } else { Value::FALSE }));
        }
        // `test` with the builtin exec is that exec whose result array never escapes.
        let exec = native == Native::RegExpExec;
        let steady = SteadyMatch { slot, prefiltered: prefilter == Prefilter::Admits, captures: result_used && exec };
        let result = self.regexp_steady_exec(p, receiver, subject, last_index, steady);
        Some(if exec { result } else { result.map(|result| Value::boolean(!result.is_null())) })
    }

    /// `String.prototype.match(regexp)` of a string under `steady_string_replace`'s proofs
    /// for `@@match`: a non-global match is the builtin exec of the string
    /// (`steady_regexp_call`), a global one `regexp_symbol_match_global_fast`.
    pub(in crate::vm) fn steady_string_match(
        &mut self,
        p: &ResidualProgram,
        this: Value,
        search: Value,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        let (matcher, flags) = self.steady_symbol_matcher(this, search, WellKnownSymbol::Match, Native::RegExpSymbolMatch)?;
        if !flags.contains(RegExpFlags::GLOBAL) {
            return self.steady_regexp_call(p, Native::RegExpExec, search, this, result_used);
        }
        let input = self.heap.string(this)?.clone();
        Some(self.with_call_roots([this, search], |vm| vm.regexp_symbol_match_builtin(p, search, this, input, &matcher, flags)))
    }

    /// The matcher and flags of the RegExp `search` when a string method of the string
    /// `this` would call `search[symbol]`, this realm's builtin `native`, and that builtin's
    /// exec, flags and lastIndex reads are the builtins' (`regexp_builtin_receiver_matcher`).
    fn steady_symbol_matcher(
        &mut self,
        this: Value,
        search: Value,
        symbol: WellKnownSymbol,
        native: Native,
    ) -> Option<(Rc<quench_regexp::Regex>, RegExpFlags)> {
        if !self.is_string(this) {
            return None;
        }
        let symbol = self.well_known_symbols.get(symbol)?;
        if let Some(proven) = self.steady_symbol_matcher_proven(search, symbol, native) {
            return proven;
        }
        let method = self.regexp_proven_symbol_method(search, symbol)?;
        match self.heap.function(method) {
            Some(FunctionRecord { kind: FunctionKind::Native(found), realm, .. })
                if *found == native && *realm == self.realm.globals => {}
            _ => return None,
        }
        self.regexp_builtin_receiver_matcher(search)
    }

    /// `steady_symbol_matcher` where both of its proofs are recorded for the receiver's and
    /// the prototype's shapes: `Some(None)` where a recorded proof's value no longer holds (the
    /// method or `exec` was replaced), `None` where a proof is not recorded yet, which the
    /// general route records.
    #[inline(always)]
    fn steady_symbol_matcher_proven(
        &mut self,
        search: Value,
        symbol: Value,
        native: Native,
    ) -> Option<Option<(Rc<quench_regexp::Regex>, RegExpFlags)>> {
        let Some(Cell::RegExp { object, meta }) = self.heap.get(search) else {
            return None;
        };
        let (flags, proto, receiver_shape) = (meta.flags, object.proto(), object.shape());
        let global = flags.contains(RegExpFlags::GLOBAL);
        if !global && flags.contains(RegExpFlags::STICKY) {
            return None;
        }
        if !self.steady_layouts_proven(proto, receiver_shape, symbol, native)? {
            return Some(None);
        }
        if !global && self.regexp_last_index_value(search).and_then(|(_, value)| value.as_number()).is_none() {
            return Some(None);
        }
        let Some(Cell::RegExp { meta, .. }) = self.heap.get(search) else {
            return Some(None);
        };
        Some(Self::regexp_matcher(meta).ok().map(|matcher| (matcher, flags)))
    }

    /// Whether the method for `symbol` and `exec` of a RegExp of `receiver_shape` and prototype
    /// `proto` are the builtins': one flag read while the prototype-values watch the layout
    /// holds is published, else both slots read, which restarts the watch. `None` where a shape
    /// proof is not recorded.
    fn steady_layouts_proven(&mut self, proto: Value, receiver_shape: u32, symbol: Value, native: Native) -> Option<bool> {
        let realm = self.realm.globals;
        let prototype = self.object_data(proto)?;
        let prototype_shape = prototype.shape();
        let position = self.regexp_symbol_layouts.iter().position(|layout| {
            layout.symbol == symbol && layout.realm == realm && layout.prototype_object == proto
                && layout.receiver == receiver_shape && layout.prototype == prototype_shape
        })?;
        let layout = &self.regexp_symbol_layouts[position];
        if layout.steady.as_ref().is_some_and(|watch| watch.is_published()) {
            return Some(true);
        }
        let method_slot = layout.slot as usize;
        let exec_slot = self.regexp_builtin_layouts.iter().find(|layout| {
            layout.realm == realm && layout.prototype_object == proto
                && layout.receiver == receiver_shape && layout.prototype == prototype_shape
        })?.exec_slot as usize;
        let proven = self.builtin_values_at(proto, method_slot, exec_slot, native)?;
        if proven {
            self.regexp_symbol_layouts[position].steady = crate::heap::watch_prototype_values(prototype_shape);
        }
        Some(proven)
    }

    /// Whether the prototype `proto`'s values at `method_slot` and `exec_slot` are `native` of
    /// this realm and the builtin `exec`.
    fn builtin_values_at(&mut self, proto: Value, method_slot: usize, exec_slot: usize, native: Native) -> Option<bool> {
        let prototype = self.object_data(proto)?;
        let method = self.heap.property_get(prototype, method_slot)?;
        let exec = self.heap.property_get(prototype, exec_slot)?;
        let realm = self.realm.globals;
        let method_is_builtin = matches!(
            self.heap.function(method),
            Some(FunctionRecord { kind: FunctionKind::Native(found), realm: owner, .. }) if *found == native && *owner == realm
        );
        Some(method_is_builtin && self.is_native_function(exec, Native::RegExpExec))
    }

    /// `String.prototype.replace(regexp, string)` when the call runs no guest code, so it
    /// observes no frame (`lane_regexp_call`): a string receiver and replacement, and a
    /// RegExp whose `@@replace` is this realm's builtin (`regexp_proven_symbol_method`) and
    /// whose exec, flags and lastIndex are builtin (`regexp_builtin_receiver_matcher`).
    /// Those make the native's steps the builtin `@@replace`'s fast path, which this runs;
    /// `None` leaves any other call to the general path.
    pub(in crate::vm) fn steady_string_replace(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        arguments: &[Value],
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        let [search, replacement] = *arguments.get(..2)?.as_array::<2>()?;
        if native != Native::StringReplace || !self.is_string(replacement) {
            return None;
        }
        let (matcher, flags) = self.steady_symbol_matcher(this, search, WellKnownSymbol::Replace, Native::RegExpSymbolReplace)?;
        let input = self.heap.string(this)?.clone();
        let template = self.heap.string(replacement)?.clone();
        if let Some(replaced) = self.replace_ascii_input(p, search, this, &input, &template, &matcher, flags, result_used) {
            return Some(replaced);
        }
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

    /// `regexp_search`'s search of an input already flat ASCII, in place, with no prepared
    /// subject to make and drop: the matcher reads its bytes (`Regex::find_ascii`). The outer
    /// `None` leaves the search to the prepared subject.
    pub(super) fn regexp_search_ascii(
        regex: &quench_regexp::Regex,
        input: &JsString,
        start: usize,
        sticky: bool,
        steady: Option<SteadyMatch>,
    ) -> Option<Option<quench_regexp::Match>> {
        let Some(crate::vm::wtf16::Units::Ascii(bytes)) = input.existing_flat().map(|flat| flat.view()) else {
            return None;
        };
        if steady.is_some_and(|steady| !steady.captures) {
            return regex.find_range_ascii(bytes, start, sticky).map(|found| found.map(quench_regexp::Match::native));
        }
        regex.find_ascii(bytes, start, sticky, true)
    }

    /// `steady_string_replace` of a flat ASCII subject by a flat ASCII template with no `$`:
    /// GetSubstitution returns the template, so the match ranges are all the replacement
    /// reads. `None` leaves any other call, or a pattern with no compiled matcher over
    /// bytes, to the general route (`regexp_replace_with_literal`).
    #[allow(clippy::too_many_arguments)]
    fn replace_ascii_input(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        this: Value,
        input: &JsString,
        template: &JsString,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        use crate::vm::wtf16::Units::Ascii;
        let (Some(Ascii(bytes)), Some(Ascii(literal))) =
            (input.existing_flat().map(|flat| flat.view()), template.existing_flat().map(|flat| flat.view()))
        else {
            return None;
        };
        if flags.contains(RegExpFlags::STICKY) || literal.contains(&b'$') {
            return None;
        }
        let global = flags.contains(RegExpFlags::GLOBAL);
        let ranges = ascii_match_ranges(matcher, bytes, global)?;
        // The exec loop of a global replace starts from lastIndex 0 and leaves it 0.
        if global && let Err(error) = self.store_regexp_last_index(p, receiver, Value::number(0.0)) {
            return Some(Err(error));
        }
        Some(self.ascii_replace_result(p, receiver, this, input, matcher, (bytes, literal), &ranges, result_used))
    }

    /// The result of a replace of `ranges` of `input`'s bytes by `literal` (`replace_ascii_input`),
    /// after its effect on the RegExp legacy statics: the subject itself where nothing matched.
    #[allow(clippy::too_many_arguments)]
    fn ascii_replace_result(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        this: Value,
        input: &JsString,
        matcher: &Rc<quench_regexp::Regex>,
        (bytes, literal): (&[u8], &[u8]),
        ranges: &[Range<usize>],
        result_used: bool,
    ) -> Result<Value, JsError> {
        let Some(last) = ranges.last() else {
            return Ok(if result_used { this } else { Value::UNDEFINED });
        };
        self.record_regexp_legacy_pending(receiver, input.clone(), matcher, last.start)?;
        let length = ranges
            .iter()
            .try_fold(bytes.len(), |length, range| (length - range.len()).checked_add(literal.len()))
            .filter(|length| *length <= super::wtf16::MAX_STRING_UNITS);
        let Some(length) = length else {
            return Err(self.range_error(p, "Invalid string length".into()));
        };
        if !result_used {
            return Ok(Value::UNDEFINED);
        }
        Ok(self.heap.alloc_string(ascii_replaced(bytes, literal, ranges, length)))
    }

    /// `regexp_builtin_exec` of `steady_regexp_call`'s case: a string subject, whose
    /// conversion runs no guest code, and `lastIndex` the Number in the own data slot, so
    /// neither its conversion nor the store runs guest code or allocates. A failed match is
    /// `null` with nothing to root; a match builds its result as the builtin does.
    fn regexp_steady_exec(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        subject: Value,
        last_index: Value,
        steady: SteadyMatch,
    ) -> Result<Value, JsError> {
        let input = self.coerce_js_string(p, subject)?;
        let last_index = self.regexp_to_length_value(p, last_index)?;
        let Some(matched) = self.regexp_match_from(p, receiver, &input, last_index, Some(steady))? else {
            return Ok(Value::NULL);
        };
        // Nothing reads the result: building it allocates and runs no guest code, so only
        // the match's own effect, the legacy statics, remains, and they find its captures
        // again from its start only if read.
        if !steady.captures {
            let Some(Cell::RegExp { meta, .. }) = self.heap.get(receiver) else {
                unreachable!("a steady call's receiver is a RegExp");
            };
            let matcher = Self::regexp_matcher(meta)?;
            self.record_regexp_legacy_pending(receiver, input, &matcher, matched.range.start)?;
            return Ok(Value::UNDEFINED);
        }
        self.with_call_roots([receiver, subject], |vm| vm.regexp_exec_result(receiver, subject, input, matched))
    }

    /// What a steady call (`steady_regexp_call`) of the RegExp `receiver` over the string
    /// `subject` reads, in one look at the regexp: the slot and Number value of its own
    /// writable data `lastIndex` (`regexp_last_index_slot`), and what its compiled regexp's
    /// prefilter says of `regexp_match_from`'s search (`Prefilter`). A `None` result leaves
    /// the call to the general path.
    #[inline(always)]
    fn regexp_steady_state(&mut self, receiver: Value, subject: Value) -> Option<(usize, Value, Prefilter)> {
        let Some(Cell::RegExp { object, .. }) = self.heap.get(receiver) else {
            return None;
        };
        let shape = object.shape();
        let slot = match self.regexp_last_index_layouts.iter().find(|(cached, _)| *cached == shape) {
            Some(&(_, slot)) => slot as usize,
            None => self.regexp_last_index_slot_miss(receiver, shape)?,
        };
        let Some(Cell::RegExp { object, meta }) = self.heap.get(receiver) else {
            return None;
        };
        // A RegExp's `lastIndex`, made first, usually lies in its inline storage.
        let inline = (slot < crate::value_vec::INLINE_PROPERTY_COUNT).then(|| object.inline_property(slot)).flatten();
        let last_index = match inline {
            Some(value) => value,
            None => self.heap.property_get(object, slot)?,
        };
        // A deleted slot is no Number either.
        let index = last_index.as_number()?;
        let input = self.heap.string(subject)?;
        let (Some(regex), Some(units)) = (meta.matcher.0.get(), input.existing_flat()) else {
            return Some((slot, last_index, Prefilter::Undecided));
        };
        let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
        let start = if stateful { regexp_to_length(index) } else { 0 };
        let length = input.len();
        let rejected = (stateful && start > length) || regex.excludes_search(length, start, meta.flags.contains(RegExpFlags::STICKY), || units.unit(start));
        Some((slot, last_index, if rejected { Prefilter::Excludes { stateful } } else { Prefilter::Admits }))
    }
}

/// What a steady call's prefilter (`Regex::excludes_search`) says of its search, decided
/// where neither conversion has an effect, over a flat subject.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefilter {
    /// No compiled matcher or no flat subject: the search decides.
    Undecided,
    /// The search may match, and needs no second look.
    Admits,
    /// No match, and whether the regexp is global or sticky, whose `lastIndex` resets.
    Excludes { stateful: bool },
}

/// What a steady call knows of its match (`regexp_match_from`): the slot of the RegExp's
/// own writable data `lastIndex`, which no method cache observes a store to; whether the
/// prefilter already admitted the search (`Prefilter::Admits`); and whether anything reads
/// the match's captures.
#[derive(Clone, Copy)]
pub(super) struct SteadyMatch {
    pub(super) slot: usize,
    pub(super) prefiltered: bool,
    pub(super) captures: bool,
}

/// The ranges of the matches of `bytes` a replace reads: the first, or each of a global
/// search (an empty match advances it by one unit, a whole code point in ASCII). `None`
/// where the pattern has no compiled matcher over bytes.
fn ascii_match_ranges(matcher: &quench_regexp::Regex, bytes: &[u8], global: bool) -> Option<smallvec::SmallVec<[Range<usize>; 8]>> {
    let mut ranges = smallvec::SmallVec::new();
    let mut next = matcher.find_range_ascii(bytes, 0, false)?;
    while let Some(range) = next {
        let resume = if range.is_empty() { range.end + 1 } else { range.end };
        ranges.push(range);
        if !global {
            break;
        }
        next = matcher.find_range_ascii(bytes, resume, false).flatten();
    }
    Some(ranges)
}

/// `bytes` with each of `ranges` replaced by `literal`, written once into storage of its
/// total `length`.
fn ascii_replaced(bytes: &[u8], literal: &[u8], ranges: &[Range<usize>], length: usize) -> JsString {
    let mut parts: smallvec::SmallVec<[&[u8]; 17]> = smallvec::SmallVec::new();
    let mut copied = 0;
    for range in ranges {
        parts.push(&bytes[copied..range.start]);
        parts.push(literal);
        copied = range.end;
    }
    parts.push(&bytes[copied..]);
    JsString::from_ascii_parts(length, parts.into_iter())
}
