//! The steady RegExp call of a flat ASCII subject (`Vm::steady_ascii_direct`): the C-shaped
//! minimum around the matcher, decided in one look at the regexp, its realm's RegExp record
//! and the subject, with `lastIndex` stored only where the store changes it.
use super::*;
use super::ascii::ascii_search;
use super::steady::{ExecStart, exec_finish, exec_start_int};
use crate::code_dependencies::IntactFuse;

impl<H: Host> Vm<H> {
    /// `steady_regexp_call` of a flat ASCII subject searched in place, decided in one look at
    /// the regexp, its realm's RegExp record and the subject (`steady_ascii_plan`): a `test`
    /// asks the recorded exec proof there, and where it does not answer yet the full proof
    /// records it (`steady_ascii_plan_proven`). A failed search, most of a tokenizer's sticky
    /// tries, stores and answers here without building a `SteadyRead`, whose moves cost more
    /// than the decision. `None` leaves the call to `regexp_steady_state`, which decides every
    /// other case and fills the `lastIndex` layout cache.
    #[inline(always)]
    pub(super) fn steady_ascii_direct(&mut self, receiver: Value, subject: Value, exec: bool, result_used: bool) -> Option<Result<Value, JsError>> {
        let captures = result_used && exec;
        let (found, store) = match self.steady_ascii_plan(receiver, subject, !exec, captures)? {
            Planned::Ready(found, store) => (found, store),
            Planned::Unproven => self.steady_ascii_plan_proven(receiver, subject, captures)?,
        };
        Some(match found {
            AsciiFound::Failed => self.steady_ascii_failed(receiver, (exec, store)),
            AsciiFound::Held(end) => self.steady_ascii_held(receiver, end, (exec, store)),
            AsciiFound::Matched(regex, input, matched) => self.steady_ascii_match(receiver, subject, input, regex, matched, (exec, store), captures),
        })
    }

    /// `steady_ascii_plan` of a `test` whose recorded exec proof did not answer: the fuse,
    /// then `regexp_exec_is_builtin`, which records the proof the next call reads, and the plan
    /// made without asking again. Out of line: a loop takes it once.
    #[cold]
    #[inline(never)]
    fn steady_ascii_plan_proven(&mut self, receiver: Value, subject: Value, captures: bool) -> Option<(AsciiFound, IndexStore)> {
        // The fuse is proved for the prototype of the realm the search runs in, which
        // `steady_exec_holds` asks: proved for the VM's own `regexp_proto` alone, a search in
        // another realm (a benchmark's fresh global) never found its fuse and came here at
        // every call.
        let prototype = self.realm.intrinsics.regexp_intrinsics.get(&self.realm.globals)?.prototype;
        if !self.fuse_intact_at(IntactFuse::RegExpExec, prototype) || !self.regexp_exec_is_builtin(receiver) {
            return None;
        }
        match self.steady_ascii_plan(receiver, subject, false, captures)? {
            Planned::Ready(found, store) => Some((found, store)),
            Planned::Unproven => None,
        }
    }

    /// What a steady search of the RegExp `receiver` over the flat ASCII string `subject`
    /// finds, read with one lookup of each and of the realm's RegExp record: with
    /// `check_exec`, first whether `exec` is the builtin (`steady_exec_holds`,
    /// `Planned::Unproven` where its record does not answer); then the integer in the own
    /// writable data `lastIndex` slot (`steady_index_store`), the compiled program cached on
    /// the regexp's record, and the search over the subject's bytes. `None` leaves the call
    /// to `regexp_steady_state`.
    #[inline(always)]
    fn steady_ascii_plan(&self, receiver: Value, subject: Value, check_exec: bool, captures: bool) -> Option<Planned> {
        let Some(Cell::RegExp { object, meta }) = self.heap.get(receiver) else {
            return None;
        };
        let intrinsics = self.realm.intrinsics.regexp_intrinsics.get(&self.realm.globals)?;
        let shape = object.shape();
        if check_exec && !self.steady_exec_holds(intrinsics, object.proto(), shape) {
            return Some(Planned::Unproven);
        }
        let (store, exec) = self.steady_index_store(object, shape, meta.flags)?;
        let regex = meta.matcher.0.get()?;
        if !store.stateful && !captures && regex.matches_at_every_start() {
            return None;
        }
        let input = self.heap.string(subject)?;
        let Some(crate::vm::wtf16::Units::Ascii(bytes)) = input.existing_units() else {
            return None;
        };
        let search = AsciiSearch { meta: &**meta, regex, input, bytes, exec };
        Some(Planned::Ready(steady_ascii_find(intrinsics, search, captures)?, store))
    }

    /// Whether a RegExp of prototype `proto` and `shape` finds this realm's builtin `exec`,
    /// from what the realm's RegExp record holds: the prototype is the realm's, the recorded
    /// proof names the shape and its chain cell still holds (no own `exec`, the prototype's
    /// in a data slot, `RegExpExecProof`), and the exec fuse holds for that prototype, so the
    /// slot still holds the intrinsic. A `false` is no answer: the full proof decides.
    #[inline(always)]
    pub(super) fn steady_exec_holds(&self, intrinsics: &RegExpIntrinsics, proto: Value, shape: u32) -> bool {
        let proven = intrinsics.exec_proof.find(shape).is_some();
        proven
            && proto == intrinsics.prototype
            && self.heap.dependencies().fuses().intact(IntactFuse::RegExpExec, intrinsics.prototype) == Some(true)
    }

    /// The `lastIndex` of a steady search of the RegExp `object` of `shape` and `flags`: its own
    /// writable data slot (`regexp_last_index_layouts`) read in place, holding an integer;
    /// where it is stored (`IndexStore`), whether the search is sticky, and where it starts.
    /// A global or sticky regexp, which stores it, needs no method cache to name `lastIndex`
    /// (`method_caches_may_name`); any other only reads it. `None` leaves the call to
    /// `regexp_steady_state`, which fills the layout cache.
    #[inline(always)]
    fn steady_index_store(&self, object: &crate::heap::Object, shape: u32, flags: RegExpFlags) -> Option<(IndexStore, ExecStart)> {
        let slot = self.regexp_last_index_layouts.iter().find(|(cached, _)| *cached == shape)?.1 as usize;
        let last_index = if slot < crate::value_vec::INLINE_PROPERTY_COUNT { object.inline_property(slot) } else { None }?;
        let index = last_index.as_int()?;
        let exec = exec_start_int(flags, index);
        if exec.stateful && self.method_caches_may_name(self.runtime_atoms.last_index) {
            return None;
        }
        let store = IndexStore { slot, stateful: exec.stateful, held: usize::try_from(index).ok() };
        Some((store, exec))
    }

    /// A steady match ending at `end` that the legacy statics already hold: `true` (`undefined`
    /// for an exec whose result is unused), a stateful regexp's `lastIndex` stored.
    #[inline(always)]
    fn steady_ascii_held(&mut self, receiver: Value, end: usize, (exec, store): (bool, IndexStore)) -> Result<Value, JsError> {
        self.steady_store_index(receiver, store, exec_finish(store.stateful, Some(end)));
        Ok(if exec { Value::UNDEFINED } else { Value::TRUE })
    }

    /// A steady search that failed: `null` (`false` for `test`), a stateful regexp's
    /// `lastIndex` reset through its slot.
    #[inline(always)]
    pub(super) fn steady_ascii_failed(&mut self, receiver: Value, (exec, store): (bool, IndexStore)) -> Result<Value, JsError> {
        self.steady_store_index(receiver, store, exec_finish(store.stateful, None));
        Ok(if exec { Value::NULL } else { Value::FALSE })
    }

    /// Stores `stored` (`exec_finish`'s value, `None` where nothing is stored) in the RegExp
    /// `receiver`'s `lastIndex` slot where `store` says the store changes it.
    #[inline(always)]
    fn steady_store_index(&mut self, receiver: Value, store: IndexStore, stored: Option<usize>) {
        if let Some(value) = stored && store.changes(value) {
            self.heap.primitive_cell_property_set(receiver, store.slot, Value::number(value as f64));
        }
    }

    /// The outcome of a match `matched` found in place, with `lastIndex` stored through
    /// its slot where the regexp is stateful and the store changes it (`IndexStore`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn steady_ascii_match(
        &mut self,
        receiver: Value,
        subject: Value,
        input: JsString,
        regex: Rc<quench_regexp::Regex>,
        matched: quench_regexp::Match,
        (exec, store): (bool, IndexStore),
        captures: bool,
    ) -> Result<Value, JsError> {
        self.steady_store_index(receiver, store, exec_finish(store.stateful, Some(matched.range.end)));
        if captures {
            return self.with_call_roots([receiver, subject], |vm| vm.regexp_exec_result(receiver, subject, input, matched));
        }
        // Nothing reads the result: only the legacy statics remain, found again from the
        // match's start if read.
        let owner = self.regexp_legacy_owner(receiver)?;
        self.record_regexp_legacy_pending(owner, input, regex, matched.range.start);
        Ok(if exec { Value::UNDEFINED } else { Value::TRUE })
    }
}

/// What `steady_ascii_plan` read of a steady call: the search's outcome and its `lastIndex`
/// store, or a `test` whose exec proof is not recorded for the regexp's shape.
enum Planned {
    Ready(AsciiFound, IndexStore),
    Unproven,
}

/// The borrowed inputs of `steady_ascii_find`: the regexp's record and compiled program, the
/// subject and its flat ASCII bytes, and where and how the search starts.
struct AsciiSearch<'a> {
    meta: &'a crate::heap::RegExpMeta,
    regex: &'a Rc<quench_regexp::Regex>,
    input: &'a JsString,
    bytes: &'a [u8],
    exec: ExecStart,
}

/// What a steady search of flat ASCII bytes found.
enum AsciiFound {
    Failed,
    /// A match the legacy statics already hold, ending here.
    Held(usize),
    /// A match to record, with the regexp's program and the subject it keeps.
    Matched(Rc<quench_regexp::Regex>, JsString, quench_regexp::Match),
}

/// The search of `steady_ascii_plan`, in place over the subject's bytes (`ascii_search`), and
/// whether the legacy statics of the realm's RegExp record `intrinsics` already hold that very
/// match, which then needs no record: only where nothing reads the captures. Only a match to
/// record clones the program and the subject. `None` where the pattern has no matcher over bytes.
#[inline(always)]
fn steady_ascii_find(intrinsics: &RegExpIntrinsics, search: AsciiSearch<'_>, captures: bool) -> Option<AsciiFound> {
    let AsciiSearch { meta, regex, input, bytes, exec } = search;
    Some(match ascii_search(regex, bytes, exec.start, exec.sticky, captures)? {
        None => AsciiFound::Failed,
        Some(found) if !captures && intrinsics.legacy_pending_holds(meta, bytes, regex, found.range.start) => AsciiFound::Held(found.range.end),
        Some(found) => AsciiFound::Matched(Rc::clone(regex), input.clone(), found),
    })
}

/// Where a steady call stores its RegExp's `lastIndex`, and whether: a global or sticky
/// regexp stores the new one unless the slot already holds that integer (`held`), so a
/// failed sticky try from 0 writes nothing. Storing the value a writable data slot already
/// holds is unobservable.
#[derive(Clone, Copy)]
pub(super) struct IndexStore {
    pub(super) slot: usize,
    pub(super) stateful: bool,
    pub(super) held: Option<usize>,
}

impl IndexStore {
    /// Whether storing `value` changes the slot (`exec_finish` decides that a store is made).
    #[inline(always)]
    fn changes(self, value: usize) -> bool {
        self.held != Some(value)
    }
}
