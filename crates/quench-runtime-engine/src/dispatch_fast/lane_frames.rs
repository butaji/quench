//! The one frame (`/mnt/project-files/calls-frames/one-frame-one-call.md`, steps F1 and F4,
//! Ledger (3)): every reader of the lane's activations and every return, whether the activation
//! is a record (`Activation`) or a plain activation's header in the value stack (call protocol
//! v2 stage C), reads it as an `ActivationView`. No read through it spills the plain activations (`LaneStack::spill_plain`), and
//! the readers stop naming the record, so a later step can take the record away from the plain
//! frames' readers without touching them again.
//!
//! Writers edit a frame where it is (one-frame-one-call.md F4 (3)): a header's side record
//! (`LaneStack::recomplete_top`, `name_top_record`, `frame_walk`'s `recomplete_at` and
//! `attach_payload`) or a record (`records_mut`). An `ActivationView` is a copy.
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

pub(crate) use quench_runtime::vm::engine_types::ActivationView;

#[quench_macros::ext(ActivationViewOps1)]
impl ActivationView {
    /// The record it becomes (a spill's, a materialization's): a record's own again.
    #[inline(always)]
    pub(crate) fn record(&self) -> Activation {
        Activation {
            callee: self.callee, this: self.this, body: self.body, window: self.window,
            function: self.function, call: self.call, caller_base: self.caller_base, caller_registers: self.caller_registers,
            caller_program: self.caller_program, kind: self.kind,
        }
    }
    /// The caller's call record, which its return completes.
    #[inline(always)] pub(crate) fn call(&self) -> Ip { self.call }
    /// The caller's records base.
    #[inline(always)] pub(crate) fn caller_base(&self) -> Ip { self.caller_base }
    /// What its completion carries (`SidePayload`).
    #[inline(always)] pub(crate) fn payload(&self) -> SidePayload { self.payload }

    /// What its return yields to its call; `None` for Wasm and an unlinked native frame.
    #[inline(always)]
    pub(crate) fn completion(&self) -> Option<Completion> { self.script().map(|script| script.completion) }

    /// A script frame's callee (a generator's, `lane_generator_returned`); `undefined` for any other.
    #[inline(always)]
    pub(crate) fn callee(&self) -> Value { self.script().map_or(Value::UNDEFINED, |script| script.callee) }

    /// The memory view its caller runs with: a Wasm callee's caller's memory, else the script
    /// view (`ActivationKind::Wasm`).
    #[inline(always)]
    pub(crate) fn caller_memory(&self, cx: *mut LaneContext) -> MemoryView {
        match self.kind {
            ActivationKind::Wasm { memory, .. } => memory,
            ActivationKind::Native | ActivationKind::Script(_) | ActivationKind::WasmEntry { .. } => MemoryView::script(cx),
        }
    }

    /// An export's frame's result type, which its JavaScript caller takes
    /// (`ActivationKind::WasmEntry`); `None` for any other frame.
    #[inline(always)]
    pub(crate) fn export_result(&self) -> Option<Option<quench_runtime::WasmType>> {
        match self.kind {
            ActivationKind::WasmEntry { result, .. } => Some(result),
            ActivationKind::Native | ActivationKind::Script(_) | ActivationKind::Wasm { .. } => None,
        }
    }
}

pub(crate) use quench_runtime::vm::engine_types::Side;

pub(crate) use quench_runtime::vm::engine_types::NO_NATIVE;

pub(crate) use quench_runtime::vm::engine_types::SidePayload;

#[quench_macros::ext(SidePayloadOps1)]
impl SidePayload {
    /// A settling frame's promise.
    pub(crate) fn settles(self) -> Option<quench_runtime::heap::RootId> {
        let Self::Settles(rooted) = self else { unreachable!("each settling activation has its entry") };
        rooted
    }

    /// An import's callee's Wasm caller.
    pub(crate) fn to_wasm(self) -> ToWasmCall {
        let Self::ToWasm(call) = self else { unreachable!("each import activation has its entry") };
        call
    }

    /// A trap's check.
    pub(crate) fn trap(self) -> quench_runtime::vm::activation::TrapCheck {
        let Self::Trapped(check) = self else { unreachable!("each trapped activation has its entry") };
        check
    }
}

#[quench_macros::ext(SideOps1)]
impl Side {
    /// The side record of a script activation pushed as a header with `flags`, `None` where the
    /// header is all of it. Its payload comes after the push (`LaneStack::attach_payload`).
    #[inline(always)]
    pub(crate) fn of(script: &ScriptActivation, caller_program: ProgramId, flags: u64) -> Option<Self> {
        (flags != 0).then_some(Self {
            native: NO_NATIVE,
            completion: script.completion,
            forwards: script.forwards,
            kept: script.kept,
            environment: script.environment,
            caller_program,
            record: script.record,
            payload: SidePayload::None,
        })
    }

    /// The side record of a header that had none: it completes as `completion` and keeps,
    /// carries and owns nothing.
    #[inline(always)]
    pub(crate) fn plain(completion: Completion, caller_program: ProgramId) -> Self {
        Self { native: NO_NATIVE, completion, forwards: false, kept: 0, environment: false, caller_program, record: std::ptr::null_mut(), payload: SidePayload::None }
    }

    /// The side-record flags its header carries (`flags_of` of the activation it describes).
    #[inline(always)]
    pub(crate) fn flags(&self) -> u64 {
        Self::flags_for(self.forwards || self.kept != 0, self.environment, self.completion, self.record)
    }

    /// The side-record flags a script activation's header carries (`frame::SIDE_FLAGS`): 0 for
    /// one that completes plainly and keeps, forwards and owns nothing.
    #[inline(always)]
    pub(crate) fn flags_of(script: &ScriptActivation) -> u64 {
        Self::flags_for(script.forwards || script.kept != 0, script.environment, script.completion, script.record)
    }

    /// The side flags row (`staged::call_complete::side_flags`) of a frame's words.
    #[inline(always)]
    fn flags_for(keeps: bool, environment: bool, completion: Completion, record: *mut quench_runtime::vm::activation::FrameRecord) -> u64 {
        quench_runtime::staged::call_complete::concrete_side_flags(
            keeps,
            environment,
            completion.crosses(),
            completion.needs_completion_flag(),
            !record.is_null(),
        )
    }
}

pub(crate) use quench_runtime::vm::engine_types::header_sided;

#[quench_macros::ext(LaneStackOps3)]
impl LaneStack {
    /// The innermost frame, read where it is: no spill and no record built for a plain one.
    #[inline(always)]
    pub(crate) fn top_frame(&self) -> Option<ActivationView> {
        if self.plain.depth.get() != 0 {
            return Some(self.plain_frame(Registers(self.plain.registers.get()), self.innermost_side(), self.plain.program.get()));
        }
        // SAFETY: the reference ends here.
        let record = unsafe { self.records_unspilled() }.last()?;
        Some(ActivationView::of_record(record, self.innermost_record_payload(record)))
    }

    /// What the side record of `record`, the innermost record and frame, carries, where it has one.
    #[inline(always)]
    fn innermost_record_payload(&self, record: &Activation) -> SidePayload {
        if !Self::record_sided(record) { return SidePayload::None; }
        // SAFETY: as `walk_innermost`; the copy ends the read.
        unsafe { (*self.sides.get()).last().expect("a sided record has its side record").payload }
    }

    /// The side record of the innermost activation, where it is a plain one whose header
    /// carries one.
    #[inline(always)]
    pub(crate) fn innermost_side(&self) -> Option<Side> {
        if self.plain.depth.get() == 0 || !header_sided(Registers(self.plain.registers.get())) {
            return None;
        }
        // SAFETY: as `walk_innermost`; the copy ends the read.
        unsafe { (*self.sides.get()).last().copied() }
    }

    /// Whether a plain activation pushed by code running `program` may share the plain
    /// activations' words: at depth 0, or where the innermost's caller runs it, or where the
    /// innermost carries a side record (whose own caller program `PlainFrames::program` names
    /// once it crosses, until a plain push over it names the running one).
    pub(crate) fn plain_program_holds(&self, program: ProgramId) -> bool {
        self.plain.depth.get() == 0 || self.plain.program.get() == program || self.innermost_side().is_some()
    }

    /// Pop the innermost plain activation, header and side record, yielding the words its return
    /// reads (`ActivationView`, one-frame-one-call.md F4): the header's pop row
    /// (`staged::protocol::pop_plain`), then the side record its flags select
    /// (`staged::call_complete::side_completion`). No record is built. The window stays for the
    /// caller to release (`lane_restore_caller`), as a popped record's does; the records and
    /// their bound are untouched.
    ///
    /// # Safety
    /// A plain activation runs (`plain.depth > 0`).
    pub(crate) unsafe fn pop_plain_frame(&mut self) -> ActivationView {
        debug_assert!(self.plain.depth.get() != 0, "a plain activation runs");
        let window = Registers(self.plain.registers.get());
        let packed = quench_runtime::staged::call_complete::frame_packed(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()), window.0.expose_provenance() as u64,
        );
        let side = quench_runtime::staged::call_complete::concrete_side_completion(packed);
        let side = side.sided.then(|| *self.sides.get_mut().last().expect("a sided header has its side record"));
        let returned = self.plain_frame(window, side, self.plain.program.get());
        quench_runtime::staged::protocol::pop_plain(
            &mut quench_runtime::staged::Concrete::new(std::ptr::null()), self.plain_frames_word() as u64, window.0.expose_provenance() as u64,
        );
        // The row named the header's caller window; a trampoline's header names its scratch
        // window, its record the real one, which the next plain activation's walk starts from.
        self.plain.registers.set(returned.caller_registers().0);
        if let Some(side) = side {
            self.sides.get_mut().pop();
            // The caller program of the plain activation below, where it carries no side record
            // (it runs its caller's program, which the popped one's caller ran).
            self.plain.program.set(side.caller_program);
        }
        returned
    }

    /// Give the innermost activation, a script one, `completion` after its push (a setter's
    /// store, a proxy trap, a native's callback, a Wasm import's callee), where it is
    /// (one-frame-one-call.md F4): a plain header's side record and its flags
    /// (`edit_top_side`), else the innermost record. No spill.
    pub(crate) fn recomplete_top(&mut self, completion: Completion) {
        if self.edit_top_side(|side| side.completion = completion) { return; }
        if let Some(ActivationKind::Script(script)) = self.records_mut().last_mut().map(|activation| activation.kind_mut()) {
            script.completion = completion;
        }
    }

    /// Name `record`, an eval record the innermost activation now owns (`lane_eval`), from the
    /// activation where it is: a plain header's side record, which `frame::OWNS_FLAG` tags
    /// (`edit_top_side`), else the innermost record's `ScriptActivation::record`. No spill.
    pub(crate) fn name_top_record(&mut self, record: *mut quench_runtime::vm::activation::FrameRecord) {
        if self.edit_top_side(|side| side.record = record) { return; }
        if let Some(ActivationKind::Script(script)) = self.records_mut().last_mut().map(|activation| activation.kind_mut()) {
            script.record = record;
        }
    }

    /// Edit the side record of the innermost activation where it is a plain header
    /// (one-frame-one-call.md F4 (3)): its side record, or a fresh one for a header that had
    /// none, then stored as the top of the side records, the innermost's (`store_side`). A
    /// trampoline's header (a native's callback, an array step's) takes its trampoline's words
    /// as its own first, and its trampoline's completion into its side record, as its spill
    /// into a record took them. `false`, editing nothing, where the innermost is a record.
    fn edit_top_side(&mut self, edit: impl FnOnce(&mut Side)) -> bool {
        if self.plain.depth.get() == 0 { return false; }
        let window = Registers(self.plain.registers.get());
        let sided = header_sided(window);
        let mut side = if sided {
            *self.sides.get_mut().last().expect("a sided header has its side record")
        } else {
            let frame = self.plain_frame(window, None, self.plain.program.get());
            window.set_plain_caller(frame.call(), frame.caller_registers(), frame.caller_base());
            Side::plain(frame.completion().expect("a plain frame is a script frame"), frame.caller_program())
        };
        edit(&mut side);
        // The innermost's side record is the top one.
        let at = self.sides.get_mut().len() - usize::from(sided);
        self.store_side(window, sided, at, side);
        true
    }

    /// Drop the innermost activation where it is: a plain one's header and side record
    /// (`pop_plain_frame`), else the innermost record with its side record, the headers it
    /// sealed unsealed (`unseal_plain`). No spill; the window stays for the caller to release,
    /// as `LaneActivations::discard_top` leaves it.
    ///
    /// # Safety
    /// An activation runs, and it holds no Wasm storage (`Activation::frame_storage_bytes`).
    #[inline(always)]
    pub(crate) unsafe fn discard_top_frame(&mut self) {
        if self.plain.depth.get() != 0 {
            // SAFETY: a plain activation runs, tested above.
            let _ = unsafe { self.pop_plain_frame() };
            return;
        }
        let records = self.records_mut();
        let top = *records.last().expect("an activation runs");
        // SAFETY: the caller's contract: the records hold the innermost activation.
        unsafe { records.discard_top() };
        let _ = self.pop_record_side(&top);
        self.unseal_plain();
    }

    /// The records alone, for writing where they are: no spill. The proved calls' bound
    /// (`PlainFrames::bound`) names neither the records' count nor the values' place any more.
    #[inline(always)]
    pub(crate) fn records_mut(&mut self) -> &mut LaneActivations {
        self.plain.bound.set(0);
        self.frames_changed();
        self.activation_records.get_mut()
    }

    /// How many records there are (native, Wasm and heap-window frames, and prepared ones
    /// emitted code's entry spilled): a record's index bound.
    #[inline(always)]
    pub(crate) fn record_count(&self) -> usize {
        // SAFETY: reads the length alone.
        unsafe { self.records_unspilled() }.len()
    }

    /// How many activations the records' allocation holds, plain ones counted against it
    /// (`spill_plain` writes them there).
    #[inline(always)]
    pub(crate) fn frame_capacity(&self) -> usize {
        // SAFETY: reads the capacity alone.
        unsafe { self.records_unspilled() }.capacity()
    }

    /// Whether the innermost activation is a native frame the lane has not linked
    /// (`LaneActivations::top_is_native`); a plain header never is one.
    #[inline(always)]
    pub(crate) fn top_is_native(&self) -> bool {
        if self.plain.depth.get() != 0 {
            return super::native_frames::header_packed(Registers(self.plain.registers.get())) & quench_runtime::staged::frame::NATIVE_FLAG != 0;
        }
        // SAFETY: the reference ends here.
        unsafe { self.records_unspilled() }.top_is_native()
    }

    /// How many frames on top belong to a running native body (`LaneActivations::native_depth`):
    /// none above a plain header, whose prefix names no callee.
    #[inline]
    pub(crate) fn native_depth(&self) -> usize {
        let headers = self.header_native_depth();
        headers + self.record_native_depth_in_run(headers)
    }

    /// The native record frames in the run that continues into the records: only when every plain header
    /// is native (`headers`), since the records under a header that is not native are not in the run.
    pub(crate) fn record_native_depth_in_run(&self, headers: usize) -> usize {
        if headers != self.plain.depth.get() { return 0; }
        // SAFETY: the reference ends here.
        unsafe { self.records_unspilled() }.native_depth()
    }

    /// The innermost frame while a native body runs in it (`top_native`): a linked native
    /// header or a record whose callee still runs its body; none for a plain header.
    #[inline(always)]
    pub(crate) fn top_native(&self) -> Option<ActivationView> {
        self.top_frame().filter(|frame| !frame.callee.is_null() && frame.body != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(function: u32, caller_registers: *mut Value, completion: Completion, kept: u16) -> Activation {
        Activation::of_parts(
            function, std::ptr::null(), std::ptr::null(), Registers(caller_registers), ProgramId::MAIN,
            ActivationKind::Script(ScriptActivation {
                callee: Value::UNDEFINED,
                argc: 1,
                completion,
                forwards: kept != 0,
                kept,
                environment: false,
                record: std::ptr::null_mut(),
            }),
        )
    }

    /// One-frame-one-call.md F4: every script activation of the lane's windows is a header, its
    /// side record pushed beside it where its flags name one; the view, the header pop and the
    /// spill read the same records a record push would have held.
    #[test]
    fn a_side_record_rides_with_its_header_and_spills_into_its_record() {
        let mut stack = LaneStack::default();
        let base = stack.values.as_mut_ptr();
        let windows = [16, 48, 80, 112].map(|at| base.wrapping_add(at));
        let shapes = [(Completion::Plain, 0), (Completion::Plain, 5), (Completion::Cross, 0), (Completion::Constructs, 0)];
        for (index, (&window, &(completion, kept))) in windows.iter().zip(&shapes).enumerate() {
            let caller = if index == 0 { std::ptr::null_mut() } else { windows[index - 1] };
            // SAFETY: the windows lie in the values' reservation, each past the context words.
            unsafe { stack.push_activation(script(index as u32, caller, completion, kept), window) };
        }
        assert_eq!(stack.activation_count(), 4);
        // SAFETY: no walk runs.
        assert_eq!(unsafe { (*stack.sides.get()).len() }, 3, "the plain one carries no side record");
        let viewed: Vec<(u32, Completion, u16)> = stack.frames()
            .map(|frame| { let script = frame.script().unwrap(); (frame.function(), script.completion, script.kept) })
            .collect();
        assert!(viewed == vec![(0, Completion::Plain, 0), (1, Completion::Plain, 5), (2, Completion::Cross, 0), (3, Completion::Constructs, 0)]);
        assert!(stack.top_frame().unwrap().script().unwrap().completion == Completion::Constructs);
        assert!(stack.top_frame().unwrap().completion() == Some(Completion::Constructs));
        let returns: Vec<Option<Completion>> = stack.frames_innermost().map(|frame| frame.completion()).collect();
        assert!(returns == vec![Some(Completion::Constructs), Some(Completion::Cross), Some(Completion::Plain), Some(Completion::Plain)]);
        let popped = stack.pop_activation().unwrap();
        assert!(popped.completion() == Some(Completion::Constructs));
        assert_eq!(popped.window_ptr(), windows[3]);
        assert_eq!(popped.caller_registers().0, windows[2]);
        assert_eq!(stack.plain.registers.get(), windows[2]);
        // The rest spill into the records a record push would have written.
        let records = stack.activations();
        assert_eq!(records.len(), 3);
        assert_eq!(records[1].script().unwrap().kept, 5);
        assert!(records[1].script().unwrap().forwards);
        assert!(records[2].script().unwrap().completion == Completion::Cross);
        assert_eq!(records[2].window_ptr(), windows[2]);
        assert_eq!(stack.plain.depth.get(), 0);
        // SAFETY: no walk runs.
        assert!(unsafe { (*stack.sides.get()).is_empty() }, "a spill takes every side record into its record");
    }
}
