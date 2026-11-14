//! RegExp calls made in place, in the lane, when they run no guest code and raise nothing.
use super::*;
use super::ascii::AsciiReplace;

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
        let exec = native == Native::RegExpExec;
        if !exec && !self.regexp_exec_is_builtin(receiver) {
            return None;
        }
        let read = self.regexp_steady_state(receiver, subject, result_used && exec)?;
        Some(self.steady_regexp_search(p, receiver, subject, exec, read, result_used))
    }

    /// `steady_regexp_call`'s search once its state is read: a search the prefilter excludes
    /// fails as the matcher's would (`null` from exec, `false` from test, a global or sticky
    /// regexp's `lastIndex` reset through its slot); `test` with the builtin exec is that
    /// exec whose result array never escapes.
    fn steady_regexp_search(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        subject: Value,
        exec: bool,
        read: SteadyRead,
        result_used: bool,
    ) -> Result<Value, JsError> {
        let (slot, last_index, prefilter) = match read {
            SteadyRead::Ascii { slot, stateful, matched } => return self.steady_ascii_outcome(receiver, subject, (exec, stateful, slot), matched, result_used),
            SteadyRead::Prefiltered(slot, last_index, prefilter) => (slot, last_index, prefilter),
        };
        if let Prefilter::Excludes { stateful } = prefilter {
            if stateful {
                self.heap.property_set(receiver, slot, Value::number(0.0));
            }
            return Ok(if exec { Value::NULL } else { Value::FALSE });
        }
        let steady = SteadyMatch { slot, prefiltered: prefilter == Prefilter::Admits, captures: result_used && exec };
        let result = self.regexp_steady_exec(p, receiver, subject, last_index, steady);
        if exec { result } else { result.map(|result| Value::boolean(!result.is_null())) }
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
        if let Some(replaced) = self.replace_ascii_input(p, search, this, replacement, &matcher, flags, result_used) {
            return Some(replaced);
        }
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
    /// reads. The subject and template are borrowed from the heap while the ranges and the
    /// result are made, and only the subject is kept (by the legacy statics). `None` leaves
    /// any other call, or a pattern with no compiled matcher over bytes, to the general
    /// route (`regexp_replace_with_literal`).
    #[allow(clippy::too_many_arguments)]
    fn replace_ascii_input(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        this: Value,
        replacement: Value,
        matcher: &Rc<quench_regexp::Regex>,
        flags: RegExpFlags,
        result_used: bool,
    ) -> Option<Result<Value, JsError>> {
        let global = flags.contains(RegExpFlags::GLOBAL);
        let mut ranges = smallvec::SmallVec::new();
        let AsciiReplace { output, too_long } = AsciiReplace::plan(&self.heap, (this, replacement), matcher, flags, result_used, &mut ranges)?;
        // The exec loop of a global replace starts from lastIndex 0 and leaves it 0.
        let holds_zero = |vm: &mut Self| vm.regexp_last_index_value(receiver).is_some_and(|(_, held)| held.as_number() == Some(0.0));
        if global && !holds_zero(self) && let Err(error) = self.store_regexp_last_index(p, receiver, Value::number(0.0)) {
            return Some(Err(error));
        }
        let Some(last) = ranges.last() else {
            return Some(Ok(if result_used { this } else { Value::UNDEFINED }));
        };
        let input = self.heap.string(this)?.clone();
        if let Err(error) = self.record_regexp_legacy_pending(receiver, input, Rc::clone(matcher), last.start) {
            return Some(Err(error));
        }
        Some(match output {
            _ if too_long => Err(self.range_error(p, "Invalid string length".into())),
            None => Ok(Value::UNDEFINED),
            Some(replaced) => Ok(self.heap.alloc_string(replaced)),
        })
    }

    /// The outcome of a flat ASCII subject's search made in place (`SteadyRead::Ascii`): a failed
    /// match is `null` (`false` for `test`) with a stateful regexp's `lastIndex` reset through
    /// its slot; a match stores the new one and builds the result as `regexp_steady_exec` does.
    fn steady_ascii_outcome(
        &mut self,
        receiver: Value,
        subject: Value,
        (exec, stateful, slot): (bool, bool, usize),
        matched: Option<(Rc<quench_regexp::Regex>, JsString, quench_regexp::Match)>,
        result_used: bool,
    ) -> Result<Value, JsError> {
        let Some((regex, input, matched)) = matched else {
            if stateful {
                self.heap.property_set(receiver, slot, Value::number(0.0));
            }
            return Ok(if exec { Value::NULL } else { Value::FALSE });
        };
        self.steady_ascii_match(receiver, subject, input, regex, matched, (exec, stateful, slot), result_used && exec)
    }

    /// The outcome of a match `matched` found in place, with `lastIndex` stored through
    /// its slot where the regexp is stateful.
    #[allow(clippy::too_many_arguments)]
    fn steady_ascii_match(
        &mut self,
        receiver: Value,
        subject: Value,
        input: JsString,
        regex: Rc<quench_regexp::Regex>,
        matched: quench_regexp::Match,
        (exec, stateful, slot): (bool, bool, usize),
        captures: bool,
    ) -> Result<Value, JsError> {
        if stateful {
            self.heap.property_set(receiver, slot, Value::number(matched.range.end as f64));
        }
        if captures {
            return self.with_call_roots([receiver, subject], |vm| vm.regexp_exec_result(receiver, subject, input, matched));
        }
        // Nothing reads the result: only the legacy statics remain, found again from the
        // match's start if read.
        self.record_regexp_legacy_pending(receiver, input, regex, matched.range.start)?;
        Ok(if exec { Value::UNDEFINED } else { Value::TRUE })
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
            self.record_regexp_legacy_pending(receiver, input, matcher, matched.range.start)?;
            return Ok(Value::UNDEFINED);
        }
        self.with_call_roots([receiver, subject], |vm| vm.regexp_exec_result(receiver, subject, input, matched))
    }

    /// The proof of `regexp_builtin_exec_matcher` that `regexp`'s exec is the builtin, without
    /// the matcher: a caller that reads the matcher itself clones nothing for it.
    #[inline(always)]
    pub(in crate::vm) fn regexp_exec_is_builtin(&self, regexp: Value) -> bool {
        let Some(Cell::RegExp { object, .. }) = self.heap.get(regexp) else {
            return false;
        };
        let Some(intrinsics) = self.realm.intrinsics.regexp_intrinsics.get(&self.realm.globals) else {
            return false;
        };
        if object.proto() != intrinsics.prototype {
            return false;
        }
        let (shape, epoch) = (object.shape(), crate::heap::prototype_layout_epoch());
        let proof = match intrinsics.exec_proof.get() {
            Some(proof) if proof.shape == shape && proof.epoch == epoch => proof,
            _ => match self.prove_regexp_exec(intrinsics, shape, epoch) {
                Some(proof) => proof,
                None => return false,
            },
        };
        let Some(data) = self.object_data(intrinsics.prototype) else {
            return false;
        };
        let exec = if proof.exec_slot < crate::value_vec::INLINE_PROPERTY_COUNT { data.inline_property(proof.exec_slot) } else { None };
        let Some(exec) = exec.or_else(|| self.heap.property_get(data, proof.exec_slot)) else {
            return false;
        };
        if intrinsics.builtin_exec.get() == Some(exec) {
            return true;
        }
        let builtin = self.is_native_function(exec, Native::RegExpExec);
        if builtin {
            intrinsics.builtin_exec.set(Some(exec));
        }
        builtin
    }

    /// What a steady call (`steady_regexp_call`) of the RegExp `receiver` over the string
    /// `subject` reads, in one look at the regexp: the slot and Number value of its own
    /// writable data `lastIndex` (`regexp_last_index_slot`), and what its compiled regexp's
    /// prefilter says of `regexp_match_from`'s search (`Prefilter`). A `None` result leaves
    /// the call to the general path.
    #[inline(always)]
    fn regexp_steady_state(&mut self, receiver: Value, subject: Value, captures: bool) -> Option<SteadyRead> {
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
        let inline = if slot < crate::value_vec::INLINE_PROPERTY_COUNT { object.inline_property(slot) } else { None };
        let last_index = match inline {
            Some(value) => value,
            None => self.heap.property_get(object, slot)?,
        };
        // A deleted slot is no Number either.
        let index = last_index.as_number()?;
        let input = self.heap.string(subject)?;
        Some(steady_search(meta, input, slot, last_index, index, captures))
    }
}

/// `regexp_steady_state`'s search of the flat `input` from the regexp's `lastIndex` number `index`.
#[inline(always)]
fn steady_search(meta: &crate::heap::RegExpMeta, input: &JsString, slot: usize, last_index: Value, index: f64, captures: bool) -> SteadyRead {
    let (Some(regex), Some(units)) = (meta.matcher.0.get(), input.existing_flat()) else {
        return SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided);
    };
    let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
    let start = if stateful { crate::units::to_length(index) } else { 0 };
    let sticky = meta.flags.contains(RegExpFlags::STICKY);
    if !stateful && !captures && regex.matches_at_every_start() {
        // Nothing reads the match but where it starts (test, or an exec whose result is
        // unused, and no lastIndex to store): a search from 0 matches at 0. The legacy
        // statics find the match again from there if read (`record_regexp_legacy_pending`).
        return SteadyRead::Ascii { slot, stateful, matched: Some((Rc::clone(regex), input.clone(), quench_regexp::Match::native(0..0))) };
    }
    if let crate::vm::wtf16::Units::Ascii(bytes) = units.view() {
        // A flat ASCII subject is searched in place, which makes the prefilter's tests
        // itself (`Regex::find_ascii`): the match outlives the borrows of the regexp and the
        // subject, which a result or the legacy statics keep, so only a match clones them.
        if let Some(found) = regex.find_ascii(bytes, start, sticky, captures) {
            let matched = match found {
                Some(found) => Some((Rc::clone(regex), input.clone(), found)),
                None => None,
            };
            return SteadyRead::Ascii { slot, stateful, matched };
        }
        return SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided);
    }
    let length = input.len();
    let rejected = (stateful && start > length) || regex.excludes_search(length, start, sticky, || units.unit(start));
    SteadyRead::Prefiltered(slot, last_index, if rejected { Prefilter::Excludes { stateful } } else { Prefilter::Admits })
}

/// What `regexp_steady_state` read of a steady call.
enum SteadyRead {
    /// A flat ASCII subject searched in place: the regexp's `lastIndex` slot, whether it is
    /// global or sticky, and the match with the regexp and subject it keeps.
    Ascii { slot: usize, stateful: bool, matched: Option<(Rc<quench_regexp::Regex>, JsString, quench_regexp::Match)> },
    /// The slot and value of `lastIndex`, and what the prefilter says of the search.
    Prefiltered(usize, Value, Prefilter),
}

/// What a steady call's prefilter (`Regex::excludes_search`) says of its search, decided
/// where neither conversion has an effect, over a flat subject.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Prefilter {
    /// No compiled matcher, no flat subject, or an ASCII one whose search makes the prefilter's
    /// tests itself: the search decides.
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
