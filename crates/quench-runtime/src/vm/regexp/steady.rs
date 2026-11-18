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
        let exec = native == Native::RegExpExec;
        // Proved once, the fuse answers `regexp_exec_is_builtin` with no read of the slot.
        if !exec && (!self.fuse_intact(crate::code_dependencies::IntactFuse::RegExpExec) || !self.regexp_exec_is_builtin(receiver)) {
            return None;
        }
        if let Some(outcome) = self.steady_ascii_direct(receiver, subject, exec, result_used) {
            return Some(outcome);
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
                self.heap.primitive_cell_property_set(receiver, slot, Value::number(0.0));
            }
            return Ok(if exec { Value::NULL } else { Value::FALSE });
        }
        let steady = SteadyMatch { slot, prefiltered: prefilter == Prefilter::Admits, captures: result_used && exec };
        let result = self.regexp_steady_exec(p, receiver, subject, last_index, steady);
        if exec { result } else { result.map(|result| Value::boolean(!result.is_null())) }
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
                self.heap.primitive_cell_property_set(receiver, slot, Value::number(0.0));
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
            self.heap.primitive_cell_property_set(receiver, slot, Value::number(matched.range.end as f64));
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
        // The prototype's exec proved the intrinsic and unchanged since (`IntactFuse`) needs
        // no read of its slot.
        let fuses = self.heap.dependencies().fuses();
        if fuses.intact(crate::code_dependencies::IntactFuse::RegExpExec, intrinsics.prototype) == Some(true) {
            return true;
        }
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

    /// `steady_regexp_call` of a flat ASCII subject searched in place, with the regexp's
    /// `lastIndex` layout cached and its value an integer: a failed search, most of a
    /// tokenizer's sticky tries, stores and answers here without building a `SteadyRead`,
    /// whose moves cost more than the decision. `None` leaves the call to
    /// `regexp_steady_state`, which decides every other case and fills the layout cache.
    #[inline(always)]
    fn steady_ascii_direct(&mut self, receiver: Value, subject: Value, exec: bool, result_used: bool) -> Option<Result<Value, JsError>> {
        let captures = result_used && exec;
        let Some(Cell::RegExp { object, meta }) = self.heap.get(receiver) else {
            return None;
        };
        let shape = object.shape();
        let slot = self.regexp_last_index_layouts.iter().find(|(cached, _)| *cached == shape)?.1 as usize;
        let last_index = if slot < crate::value_vec::INLINE_PROPERTY_COUNT { object.inline_property(slot) } else { None }?;
        let (stateful, sticky, start) = steady_start(meta.flags, last_index.as_int()?);
        let regex = meta.matcher.0.get()?;
        if !stateful && !captures && regex.matches_at_every_start() {
            return None;
        }
        let input = self.heap.string(subject)?;
        let crate::vm::wtf16::Units::Ascii(bytes) = input.existing_flat()?.view() else {
            return None;
        };
        if !captures {
            let range = match self.steady_range(meta, regex, bytes, (start, sticky))? {
                RangeFound::Failed => return Some(self.steady_ascii_failed(receiver, (exec, stateful, slot))),
                RangeFound::Held(end) => return Some(self.steady_ascii_held(receiver, end, (exec, stateful, slot))),
                RangeFound::Record(range) => range,
            };
            let (regex, input) = (Rc::clone(regex), input.clone());
            return Some(self.steady_ascii_match(receiver, subject, input, regex, quench_regexp::Match::native(range), (exec, stateful, slot), false));
        }
        let found = regex.find_ascii(bytes, start, sticky, captures)?;
        let Some(found) = found else {
            return Some(self.steady_ascii_failed(receiver, (exec, stateful, slot)));
        };
        let (regex, input) = (Rc::clone(regex), input.clone());
        Some(self.steady_ascii_match(receiver, subject, input, regex, found, (exec, stateful, slot), captures))
    }

    /// The range a steady search of a RegExp of `meta` over `bytes` finds where nothing reads
    /// its captures, and whether the legacy statics already hold that very match, which then
    /// needs no record.
    #[inline(always)]
    fn steady_range(&self, meta: &crate::heap::RegExpMeta, regex: &Rc<quench_regexp::Regex>, bytes: &[u8], (start, sticky): (usize, bool)) -> Option<RangeFound> {
        Some(match regex.find_range_ascii(bytes, start, sticky)? {
            None => RangeFound::Failed,
            Some(range) if self.regexp_legacy_pending_holds(meta, bytes, regex, range.start) => RangeFound::Held(range.end),
            Some(range) => RangeFound::Record(range),
        })
    }

    /// A steady match ending at `end` that the legacy statics already hold: `true` (`undefined`
    /// for an exec whose result is unused), a stateful regexp's `lastIndex` stored.
    #[inline(always)]
    fn steady_ascii_held(&mut self, receiver: Value, end: usize, (exec, stateful, slot): (bool, bool, usize)) -> Result<Value, JsError> {
        if stateful {
            self.heap.primitive_cell_property_set(receiver, slot, Value::number(end as f64));
        }
        Ok(if exec { Value::UNDEFINED } else { Value::TRUE })
    }

    /// A steady search that failed: `null` (`false` for `test`), a stateful regexp's
    /// `lastIndex` reset through its slot.
    #[inline(always)]
    fn steady_ascii_failed(&mut self, receiver: Value, (exec, stateful, slot): (bool, bool, usize)) -> Result<Value, JsError> {
        if stateful {
            self.heap.primitive_cell_property_set(receiver, slot, Value::number(0.0));
        }
        Ok(if exec { Value::NULL } else { Value::FALSE })
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

/// `steady_search` of an ASCII rope, searched in a reused buffer of its bytes
/// (`JsString::with_ascii_rope_bytes`): the match keeps the rope and positions in it.
#[cold]
#[inline(never)]
fn steady_rope_search(
    meta: &crate::heap::RegExpMeta,
    regex: &Rc<quench_regexp::Regex>,
    input: &JsString,
    slot: usize,
    last_index: Value,
    index: f64,
    captures: bool,
) -> SteadyRead {
    let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
    let start = if stateful { crate::units::to_length(index) } else { 0 };
    let sticky = meta.flags.contains(RegExpFlags::STICKY);
    match input.with_ascii_rope_bytes(|bytes| regex.find_ascii(bytes, start, sticky, captures)) {
        Some(Some(found)) => SteadyRead::Ascii {
            slot,
            stateful,
            matched: found.map(|found| (Rc::clone(regex), input.clone(), found)),
        },
        _ => SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided),
    }
}

/// `regexp_steady_state`'s search of the flat `input` from the regexp's `lastIndex` number `index`.
#[inline(always)]
fn steady_search(meta: &crate::heap::RegExpMeta, input: &JsString, slot: usize, last_index: Value, index: f64, captures: bool) -> SteadyRead {
    let Some(regex) = meta.matcher.0.get() else {
        return SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided);
    };
    let Some(units) = input.existing_flat() else {
        return steady_rope_search(meta, regex, input, slot, last_index, index, captures);
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

/// Whether a RegExp of `flags` is global or sticky, whether sticky, and where its search
/// starts, from the integer `lastIndex` `index`.
#[inline(always)]
fn steady_start(flags: RegExpFlags, index: i32) -> (bool, bool, usize) {
    let stateful = flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
    let start = if stateful { usize::try_from(index).unwrap_or(0) } else { 0 };
    (stateful, flags.contains(RegExpFlags::STICKY), start)
}

/// What `steady_range` found.
enum RangeFound {
    Failed,
    /// A match the legacy statics already hold, ending here.
    Held(usize),
    /// A match to record.
    Record(Range<usize>),
}
