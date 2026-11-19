//! The one frame view (`/mnt/project-files/calls-frames/one-frame-one-call.md`, step F1): every
//! reader of the lane's activations, whether the activation is a record (`Activation`) or a plain
//! activation's header in the value stack (call protocol v2 stage C), reads it through
//! `FrameView`. No read through it spills the plain activations (`LaneStack::spill_plain`), and
//! the readers stop naming the record, so a later step can take the record away from the plain
//! frames' readers without touching them again.
//!
//! Writers edit a frame where it is (one-frame-one-call.md F4 (3)): a header's side record
//! (`LaneStack::recomplete_top`, `name_top_record`, `frame_walk`'s `recomplete_at` and
//! `attach_payload`) or a record (`records_mut`). A view is a copy.
use super::*;

/// One activation as a reader sees it: a record, or a plain header (with its side record, where
/// its flags name one) read as the record its spill would write (`LaneStack::plain_record`), so
/// a reader answers the same either way.
#[derive(Clone, Copy)]
pub(in crate::vm) struct FrameView {
    record: Activation,
}

#[allow(dead_code)] // the accessors the remaining readers (F2/F3) take over from `Activation`
impl FrameView {
    #[inline(always)]
    fn of_record(record: &Activation) -> Self { Self { record: *record } }

    /// The function it runs.
    #[inline(always)] pub(in crate::vm) fn function(&self) -> u32 { self.record.function() }
    /// The caller's call record, which its return completes.
    #[inline(always)] pub(super) fn call(&self) -> Ip { self.record.call() }
    /// The caller's records base.
    #[inline(always)] pub(super) fn caller_base(&self) -> Ip { self.record.caller_base() }
    /// The caller's registers.
    #[inline(always)] pub(super) fn caller_registers(&self) -> Registers { self.record.caller_registers() }
    /// The program its caller runs.
    #[inline(always)] pub(in crate::vm) fn caller_program(&self) -> ProgramId { self.record.caller_program() }
    /// The pc of the call record it was entered from, in its caller's body; `None` for a native
    /// frame the lane has not linked (`Activation::caller_pc`).
    #[inline(always)] pub(in crate::vm) fn caller_pc(&self) -> Option<usize> { self.record.caller_pc() }
    /// What kind of activation it is (a plain header is a `Script` activation that completes
    /// plainly, returns to a native through the callback trampoline, or steps an array
    /// iteration through the array steps' trampoline).
    #[inline(always)] pub(super) fn kind(&self) -> ActivationKind { *self.record.kind() }
    /// Its JavaScript frame state; `None` for Wasm and an unlinked native frame.
    #[inline(always)] pub(super) fn script(&self) -> Option<ScriptActivation> { self.record.script().copied() }
    /// The address of its first register.
    #[inline(always)] pub(in crate::vm) fn window_ptr(&self) -> *mut Value { self.record.window_ptr() }
    /// Its prefix's WINDOW word, mark included (`Activation::window_word`).
    #[inline(always)] pub(in crate::vm) fn window_word(&self) -> usize { self.record.window_word() }
    /// The Wasm frame payload it accounts for (`Activation::frame_storage_bytes`).
    #[inline(always)] pub(super) fn frame_storage_bytes(&self) -> usize { self.record.frame_storage_bytes() }
    /// Its native prefix (`ShadowFrame::EMPTY` but the window for a plain header).
    #[inline(always)] pub(in crate::vm) fn native(&self) -> super::super::compiled::ShadowFrame { self.record.native }
}

/// What a Wasm activation's return reads past the words every frame's does.
#[derive(Clone, Copy)]
pub(super) enum WasmReturn {
    /// A Wasm callee's: its caller's memory (`ActivationKind::Wasm`).
    Inner(MemoryView),
    /// An export's: the result type its JavaScript caller takes (`ActivationKind::WasmEntry`).
    Entry(Option<crate::WasmType>),
}

/// The words a frame's return reads, and no more (one-frame-one-call.md F4): a plain frame's
/// header words with its side record (`LaneStack::plain_return`), or a record's
/// (`FrameReturn::of_record`: native frames, Wasm, heap windows). What the pop of a frame
/// yields (`LaneStack::pop_activation`) and every completion consumer takes
/// (`lane_restore_caller`, `lane_resume_after_call`, `body_calls::ends_run`,
/// `setters::completed_value`, the generator and array-step completions), so a plain frame's
/// return rebuilds no record.
#[derive(Clone, Copy)]
pub(super) struct FrameReturn {
    /// The function it ran.
    #[allow(dead_code)] // read by the frame tests and the activations bench
    pub(super) function: u32,
    /// The caller's call record, which the return completes.
    pub(super) call: Ip,
    pub(super) caller_registers: Registers,
    pub(super) caller_base: Ip,
    /// The program its caller runs.
    pub(super) caller_program: ProgramId,
    /// Its WINDOW word, heap mark included (`Activation::window_word`).
    pub(super) window: usize,
    /// What its return yields to its call; `None` for Wasm and an unlinked native frame.
    pub(super) completion: Option<Completion>,
    /// A script frame's callee (a generator's, `lane_generator_returned`).
    pub(super) callee: Value,
    /// A Wasm frame's own return words.
    pub(super) wasm: Option<WasmReturn>,
    /// What its completion carries, from its side record (`SidePayload`).
    pub(super) payload: SidePayload,
}

impl FrameReturn {
    /// The return words of a record.
    #[inline(always)]
    pub(super) fn of_record(record: &Activation) -> Self {
        let (completion, callee) = record.script().map_or((None, Value::UNDEFINED), |script| (Some(script.completion), script.callee));
        Self {
            function: record.function(),
            call: record.call(),
            caller_registers: record.caller_registers(),
            caller_base: record.caller_base(),
            caller_program: record.caller_program(),
            window: record.window_word(),
            completion,
            callee,
            wasm: match *record.kind() {
                ActivationKind::Wasm { memory, .. } => Some(WasmReturn::Inner(memory)),
                ActivationKind::WasmEntry { result, .. } => Some(WasmReturn::Entry(result)),
                ActivationKind::Native | ActivationKind::Script(_) => None,
            },
            payload: SidePayload::None,
        }
    }

    /// The return words of a record with its side record, where it has one (`LaneStack::record_sided`).
    #[inline(always)]
    pub(super) fn of_sided_record(record: &Activation, side: Option<Side>) -> Self {
        Self { payload: side.map_or(SidePayload::None, |side| side.payload), ..Self::of_record(record) }
    }

    /// The address of its first register (`Activation::window_ptr`).
    #[inline(always)]
    pub(super) fn window_ptr(&self) -> *mut Value { (self.window & !HEAP_WINDOW) as *mut Value }

    /// The memory view its caller runs with (`Activation::caller_memory`).
    #[inline(always)]
    pub(super) fn caller_memory<H: Host>(&self, cx: *mut LaneContext<H>) -> MemoryView {
        match self.wasm {
            Some(WasmReturn::Inner(memory)) => memory,
            Some(WasmReturn::Entry(_)) | None => MemoryView::script(cx),
        }
    }
}

/// What a frame holds beyond its 8-word header (one-frame-one-call.md F4): one entry of
/// `LaneStack::sides` per frame that has one, innermost last, tagged by its header's flags
/// (`frame::SIDE_FLAGS`, `staged::call_complete::side_flags`), or, for a record, by its
/// completion (`LaneStack::record_sided`, which reads only `payload`). A frame finds its own as
/// the top of the side records while it is the innermost; a walk pairs frames with sides
/// innermost first, taking one side per sided frame (`frame_walk::FrameWalk`).
#[derive(Clone, Copy)]
pub(super) struct Side {
    /// What its return yields to its call (`frame::COMPLETION_FLAG`, `frame::CROSSES_FLAG`).
    pub(super) completion: Completion,
    /// It keeps the values passed past its registers (`frame::KEEPS_FLAG`, `ScriptActivation::forwards`).
    pub(super) forwards: bool,
    /// The register of the first value it keeps, 0 for none (`ScriptActivation::kept`).
    pub(super) kept: u16,
    /// Its environment and frame locals follow its registers (`frame::ENV_FLAG`).
    pub(super) environment: bool,
    /// The program its caller runs: another than its own where it crosses.
    pub(super) caller_program: ProgramId,
    /// The eval record it owns (`frame::OWNS_FLAG`, `LaneStack::records`), null for none: a
    /// record's `ScriptActivation::record`.
    pub(super) record: *mut super::super::activation::FrameRecord,
    /// What its completion carries (one-frame-one-call.md F4 (3)).
    pub(super) payload: SidePayload,
}

/// What a frame's completion carries past its header, in its side record, so it pushes and pops
/// with its frame (one-frame-one-call.md F4 (3)); its frame takes it when it materializes
/// (`Vm::materialize_lane_activations`). Exactly the frames whose completion is one of these,
/// a native frame's skipped constructor apart (its descriptor's), carry one.
#[derive(Clone, Copy)]
pub(super) enum SidePayload {
    None,
    /// `Completion::Forwarder`: the forwarding constructor the construction skipped.
    Forwarder(SkippedForwarder),
    /// `Completion::SettlesPromise`: the call's rooted promise, which its return resolves.
    /// `None` for a call whose result only the `await` after it reads
    /// (`ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG`): its promise is made only where one
    /// is observable, and a primitive return reaches the `await` as it is.
    Settles(Option<crate::heap::RootId>),
    /// `Completion::ToWasm`: what its return restores of its Wasm caller.
    ToWasm(ToWasmCall),
    /// `Completion::ReadsTrapped` and `AssignsTrapped`: what the trap owes its target's
    /// invariants (`CallCompletion::Trapped`).
    Trapped(super::super::activation::TrapCheck),
}

impl SidePayload {
    /// A settling frame's promise.
    pub(super) fn settles(self) -> Option<crate::heap::RootId> {
        let Self::Settles(rooted) = self else { unreachable!("each settling activation has its entry") };
        rooted
    }

    /// An import's callee's Wasm caller.
    pub(super) fn to_wasm(self) -> ToWasmCall {
        let Self::ToWasm(call) = self else { unreachable!("each import activation has its entry") };
        call
    }

    /// A trap's check.
    pub(super) fn trap(self) -> super::super::activation::TrapCheck {
        let Self::Trapped(check) = self else { unreachable!("each trapped activation has its entry") };
        check
    }

    /// A forwarder frame's skipped constructor; `None` for a native frame's, its descriptor's.
    pub(super) fn forwarder(self) -> Option<SkippedForwarder> {
        match self { Self::Forwarder(skipped) => Some(skipped), _ => None }
    }
}

impl Side {
    /// The side record of a script activation pushed as a header with `flags`, `None` where the
    /// header is all of it. Its payload comes after the push (`LaneStack::attach_payload`).
    #[inline(always)]
    pub(super) fn of(script: &ScriptActivation, caller_program: ProgramId, flags: u64) -> Option<Self> {
        (flags != 0).then_some(Self {
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
    pub(super) fn plain(completion: Completion, caller_program: ProgramId) -> Self {
        Self { completion, forwards: false, kept: 0, environment: false, caller_program, record: std::ptr::null_mut(), payload: SidePayload::None }
    }

    /// The side-record flags its header carries (`flags_of` of the activation it describes).
    #[inline(always)]
    pub(super) fn flags(&self) -> u64 {
        Self::flags_for(self.forwards || self.kept != 0, self.environment, self.completion, self.record)
    }

    /// The side-record flags a script activation's header carries (`frame::SIDE_FLAGS`): 0 for
    /// one that completes plainly and keeps, forwards and owns nothing.
    #[inline(always)]
    pub(super) fn flags_of(script: &ScriptActivation) -> u64 {
        Self::flags_for(script.forwards || script.kept != 0, script.environment, script.completion, script.record)
    }

    /// The side flags row (`staged::call_complete::side_flags`) of a frame's words.
    #[inline(always)]
    fn flags_for(keeps: bool, environment: bool, completion: Completion, record: *mut super::super::activation::FrameRecord) -> u64 {
        crate::staged::call_complete::concrete_side_flags(
            keeps,
            environment,
            completion.crosses(),
            !matches!(completion, Completion::Plain | Completion::Cross),
            !record.is_null(),
        )
    }
}

/// Whether the frame header below `window` carries a side record.
#[inline(always)]
pub(super) fn header_sided(window: Registers) -> bool {
    !crate::staged::call_complete::concrete_frame_completes_plainly(window.0.expose_provenance() as u64)
}

impl LaneStack {
    /// The record the plain activation whose window is `window` becomes when it spills, and its
    /// caller's window, the next plain activation's where one remains below it. `side` is its
    /// side record where its header carries one (`header_sided`), and `program` the program its
    /// caller runs where it carries none: the innermost's is `PlainFrames::program`, each other's
    /// the caller program of the frame above it. A native's callback (`CallbackTrampoline`),
    /// always the outermost plain activation, becomes `PreparedShape::of`'s record. An in-lane
    /// array step's callback (`steps_return`), whose caller base word is its step's index,
    /// becomes the `Completion::ArrayStep` record `lane_begin_array_iteration` pushed before,
    /// with the call record, caller registers and base its step holds. The one definition
    /// `spill_plain` and the frame view share.
    pub(super) fn plain_record(&self, window: Registers, side: Option<Side>, program: ProgramId) -> (Activation, Registers) {
        let returned = self.plain_return(window, side, program);
        let (function, argc, callee) = window.plain_callee();
        let (forwards, kept, environment, record) =
            side.map_or((false, 0, false, std::ptr::null_mut()), |side| (side.forwards, side.kept, side.environment, side.record));
        let mut activation = Activation::of_parts(
            crate::vm::compiled::ShadowFrame::EMPTY, function, returned.call, returned.caller_base, returned.caller_registers, returned.caller_program,
            ActivationKind::Script(ScriptActivation {
                callee,
                argc,
                completion: returned.completion.expect("a plain frame is a script frame"),
                forwards,
                kept,
                environment,
                record,
            }),
        );
        activation.native.window = window.0.addr();
        (activation, returned.caller_registers)
    }

    /// The return words of the plain activation whose window is `window` (`FrameReturn`), read
    /// from its header and `side`, its side record where its header carries one, and `program`
    /// as `plain_record` takes it: no record is built. A trampoline's header (a native's
    /// callback, an array step's) names the call record, caller registers and base its
    /// trampoline holds, as `plain_record` reads them. The caller registers are the next plain
    /// activation's window where one remains below.
    #[inline(always)]
    pub(super) fn plain_return(&self, window: Registers, side: Option<Side>, program: ProgramId) -> FrameReturn {
        let (call, caller_registers, caller_base) = window.plain_caller();
        let (call, caller_registers, caller_base, completion) = if let Some(side) = side {
            (call, caller_registers, caller_base, side.completion)
        } else if let Some((call, registers)) = self.callback_return.returns(call) {
            (call, registers, caller_base, Completion::ReturnsToNative)
        } else if self.callback_return.steps_return(call) {
            let step = &self.steps[caller_base.addr()];
            let header = step.header.expect("an array step's plain callback has its header");
            (header.call, header.caller_registers, step.caller_base, Completion::ArrayStep)
        } else {
            (call, caller_registers, caller_base, Completion::Plain)
        };
        // SAFETY: the window's header was written at its push (`push_plain`).
        let (packed, callee) = unsafe { (*window.0.sub(CONTEXT_PACKED).cast::<u64>(), *window.0.sub(CONTEXT_CALLEE)) };
        FrameReturn {
            function: packed as u32,
            call,
            caller_registers,
            caller_base,
            caller_program: side.map_or(program, |side| side.caller_program),
            window: window.0.addr(),
            completion: Some(completion),
            callee,
            wasm: None,
            payload: side.map_or(SidePayload::None, |side| side.payload),
        }
    }

    /// The return words of every activation, innermost first, as `frames().rev()` reads them
    /// but building no record: the plain headers with their side records (`plain_return`) and
    /// the records with theirs, where the walk finds them (`walk_innermost`). A reader calls no
    /// `activations` while it walks.
    pub(super) fn frame_returns_innermost(&self) -> impl Iterator<Item = FrameReturn> + '_ {
        self.walk_innermost().map(move |walked| self.walked_return(&walked))
    }

    /// The return words of the innermost activation, read where it is: no spill and no record
    /// built for a plain one.
    #[inline(always)]
    pub(super) fn top_return(&self) -> Option<FrameReturn> {
        if self.plain.depth.get() != 0 {
            return Some(self.plain_return(Registers(self.plain.registers.get()), self.innermost_side(), self.plain.program.get()));
        }
        // SAFETY: the reference ends here.
        let record = unsafe { self.records_unspilled() }.last()?;
        Some(FrameReturn::of_sided_record(record, self.innermost_record_side(record)))
    }

    /// The side record of `record`, the innermost record and frame, where it has one.
    #[inline(always)]
    fn innermost_record_side(&self, record: &Activation) -> Option<Side> {
        // SAFETY: as `walk_innermost`; the copy ends the read.
        Self::record_sided(record).then(|| unsafe { *(*self.sides.get()).last().expect("a sided record has its side record") })
    }

    /// The side record of the innermost activation, where it is a plain one whose header
    /// carries one.
    #[inline(always)]
    pub(super) fn innermost_side(&self) -> Option<Side> {
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
    pub(super) fn plain_program_holds(&self, program: ProgramId) -> bool {
        self.plain.depth.get() == 0 || self.plain.program.get() == program || self.innermost_side().is_some()
    }

    /// Pop the innermost plain activation, header and side record, yielding the words its return
    /// reads (`FrameReturn`, one-frame-one-call.md F4): the header's pop row
    /// (`staged::protocol::pop_plain`), then the side record its flags select
    /// (`staged::call_complete::side_completion`). No record is built. The window stays for the
    /// caller to release (`lane_restore_caller`), as a popped record's does; the records and
    /// their bound are untouched.
    ///
    /// # Safety
    /// A plain activation runs (`plain.depth > 0`).
    pub(super) unsafe fn pop_plain_frame(&mut self) -> FrameReturn {
        debug_assert!(self.plain.depth.get() != 0, "a plain activation runs");
        let window = Registers(self.plain.registers.get());
        let packed = crate::staged::call_complete::frame_packed(
            &mut crate::staged::Concrete::new(std::ptr::null()), window.0.expose_provenance() as u64,
        );
        let side = crate::staged::call_complete::concrete_side_completion(packed);
        let side = side.sided.then(|| *self.sides.get_mut().last().expect("a sided header has its side record"));
        let returned = self.plain_return(window, side, self.plain.program.get());
        crate::staged::protocol::pop_plain(
            &mut crate::staged::Concrete::new(std::ptr::null()), self.plain_frames_word() as u64, window.0.expose_provenance() as u64,
        );
        // The row named the header's caller window; a trampoline's header names its scratch
        // window, its record the real one, which the next plain activation's walk starts from.
        self.plain.registers.set(returned.caller_registers.0);
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
    pub(super) fn recomplete_top(&mut self, completion: Completion) {
        if self.edit_top_side(|side| side.completion = completion) { return; }
        if let Some(ActivationKind::Script(script)) = self.records_mut().last_mut().map(|activation| activation.kind_mut()) {
            script.completion = completion;
        }
    }

    /// Name `record`, an eval record the innermost activation now owns (`lane_eval`), from the
    /// activation where it is: a plain header's side record, which `frame::OWNS_FLAG` tags
    /// (`edit_top_side`), else the innermost record's `ScriptActivation::record`. No spill.
    pub(super) fn name_top_record(&mut self, record: *mut super::super::activation::FrameRecord) {
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
            let returned = self.plain_return(window, None, self.plain.program.get());
            window.set_plain_caller(returned.call, returned.caller_registers, returned.caller_base);
            Side::plain(returned.completion.expect("a plain frame is a script frame"), returned.caller_program)
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
    pub(super) unsafe fn discard_top_frame(&mut self) {
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

    /// The records alone, read without spilling.
    ///
    /// # Safety
    /// No `activations` call (which may spill through a shared reference) happens while the
    /// returned reference lives.
    #[inline(always)]
    pub(super) unsafe fn records_unspilled(&self) -> &LaneActivations {
        // SAFETY: the caller's contract; only `spill_plain` writes through a shared reference.
        unsafe { &*self.activation_records.get() }
    }

    /// The records alone, for writing where they are: no spill. The proved calls' bound
    /// (`PlainFrames::bound`) names neither the records' count nor the values' place any more.
    #[inline(always)]
    pub(super) fn records_mut(&mut self) -> &mut LaneActivations {
        self.plain.bound.set(0);
        self.activation_records.get_mut()
    }

    /// How many records there are (native, Wasm and heap-window frames, and prepared ones
    /// emitted code's entry spilled): a record's index bound.
    #[inline(always)]
    pub(super) fn record_count(&self) -> usize {
        // SAFETY: reads the length alone.
        unsafe { self.records_unspilled() }.len()
    }

    /// Every activation, records and plain headers alike (`activation_count`): whether any is.
    #[inline(always)]
    pub(in crate::vm) fn has_frames(&self) -> bool { self.activation_count() != 0 }

    /// How many activations the records' allocation holds, plain ones counted against it
    /// (`spill_plain` writes them there).
    #[inline(always)]
    pub(in crate::vm) fn frame_capacity(&self) -> usize {
        // SAFETY: reads the capacity alone.
        unsafe { self.records_unspilled() }.capacity()
    }

    /// The innermost activation, read where it is: no spill.
    #[inline]
    pub(in crate::vm) fn top_frame(&self) -> Option<FrameView> {
        if self.plain.depth.get() != 0 {
            let (record, _) = self.plain_record(Registers(self.plain.registers.get()), self.innermost_side(), self.plain.program.get());
            return Some(FrameView { record });
        }
        // SAFETY: the reference ends here.
        unsafe { self.records_unspilled() }.last().map(FrameView::of_record)
    }

    /// The activation `depth` above the outermost, as `activations()[depth]` would read it once
    /// spilled; `None` past the innermost.
    pub(in crate::vm) fn frame(&self, depth: usize) -> Option<FrameView> {
        let above = self.activation_count().checked_sub(depth + 1)?;
        self.walk_innermost().nth(above).map(|walked| self.walked_view(&walked))
    }

    /// Every activation, outermost first, as `activations().iter()` would yield them once
    /// spilled, without spilling: the walk's frames (`walk_innermost`), reversed. The walk reads
    /// no record a spill could write during it: a reader calls no `activations` while it walks.
    pub(in crate::vm) fn frames(&self) -> impl Iterator<Item = FrameView> + '_ {
        let mut frames: Vec<FrameView> = self.walk_innermost().map(|walked| self.walked_view(&walked)).collect();
        frames.reverse();
        frames.into_iter()
    }

    /// A walked frame as a reader sees it (`FrameView`).
    fn walked_view(&self, walked: &super::frame_walk::WalkedFrame) -> FrameView {
        FrameView { record: self.walked_record(walked) }
    }

    /// Whether the innermost activation is a native frame the lane has not linked
    /// (`LaneActivations::top_is_native`); a plain header never is one.
    #[inline(always)]
    pub(in crate::vm) fn top_is_native(&self) -> bool {
        // SAFETY: the reference ends here.
        self.plain.depth.get() == 0 && unsafe { self.records_unspilled() }.top_is_native()
    }

    /// How many frames on top belong to a running native body (`LaneActivations::native_depth`):
    /// none above a plain header, whose prefix names no callee.
    #[inline]
    pub(in crate::vm) fn native_depth(&self) -> usize {
        if self.plain.depth.get() != 0 { return 0; }
        // SAFETY: the reference ends here.
        unsafe { self.records_unspilled() }.native_depth()
    }

    /// The prefix of the innermost frame while a native body runs in it
    /// (`LaneActivations::top_native`): none for a plain header.
    #[inline(always)]
    pub(in crate::vm) fn top_native(&self) -> Option<super::super::compiled::ShadowFrame> {
        if self.plain.depth.get() != 0 { return None; }
        // SAFETY: the reference ends here.
        unsafe { self.records_unspilled() }.top_native()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(function: u32, caller_registers: *mut Value, completion: Completion, kept: u16) -> Activation {
        Activation::of_parts(
            crate::vm::compiled::ShadowFrame::EMPTY, function, std::ptr::null(), std::ptr::null(), Registers(caller_registers), ProgramId::MAIN,
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
        assert!(stack.top_return().unwrap().completion == Some(Completion::Constructs));
        let returns: Vec<Option<Completion>> = stack.frame_returns_innermost().map(|frame| frame.completion).collect();
        assert!(returns == vec![Some(Completion::Constructs), Some(Completion::Cross), Some(Completion::Plain), Some(Completion::Plain)]);
        let popped = stack.pop_activation().unwrap();
        assert!(popped.completion == Some(Completion::Constructs));
        assert_eq!(popped.window_ptr(), windows[3]);
        assert_eq!(popped.caller_registers.0, windows[2]);
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
