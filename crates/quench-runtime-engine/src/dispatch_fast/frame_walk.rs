//! The lane's frames as one walk (one-frame-one-call.md F4 (3)): plain headers with their side
//! records, and the records left (native, Wasm and heap-window frames) between them. A record
//! pushed over plain headers seals them where they are (`PlainSegment`) instead of spilling them
//! into records, and its pop unseals them; every reader pairs the frames with the side records
//! innermost first, so no reader and no writer but emitted code's entry spills.
use super::*;
use super::lane_frames::{header_sided, ActivationView, Side};

pub(crate) use quench_runtime::vm::engine_types::PlainSegment;

pub(crate) use quench_runtime::vm::engine_types::Walked;

pub(crate) use quench_runtime::vm::engine_types::Walk;







#[quench_macros::ext(LaneStackOps5)]
impl LaneStack {
    /// Give the innermost activation, just pushed or recompleted, what its completion carries
    /// (`SidePayload`): a header's side record takes it, which its completion flag made; a
    /// record's is pushed.
    pub(crate) fn attach_payload(&mut self, payload: SidePayload) {
        if self.plain.depth.get() != 0 {
            debug_assert!(header_sided(Registers(self.plain.registers.get())), "a payload's completion flags its header");
            self.sides.get_mut().last_mut().expect("a sided header has its side record").payload = payload;
        } else {
            let index = self.record_count() - 1;
            self.push_record_side(index, payload);
        }
    }

    /// Push the side record of the record at `index` carrying `payload`: the innermost sided
    /// frame's, the frames above holding none.
    pub(crate) fn push_record_side(&mut self, index: usize, payload: SidePayload) {
        // SAFETY: the copy ends the read.
        let record = unsafe { self.records_unspilled() }[index];
        debug_assert!(Self::record_sided(&record), "a record's payload rides with its completion");
        self.sides.get_mut().push(Self::record_side(&record, payload));
    }

    /// The side record of `record`, a sided one, carrying `payload`: a record's own words say
    /// the rest, so its side record is read for its payload alone (`record_sided`).
    pub(crate) fn record_side(record: &Activation, payload: SidePayload) -> Side {
        let completion = record.script().map_or(Completion::Plain, |script| script.completion);
        Side { payload, ..Side::plain(completion, record.caller_program()) }
    }

    /// Pop the side record of `record`, the innermost frame just popped, where it owns one: what
    /// its completion carries.
    pub(crate) fn pop_record_side(&mut self, record: &Activation) -> SidePayload {
        if !Self::record_sided(record) { return SidePayload::None; }
        self.sides.get_mut().pop().expect("a sided record has its side record").payload
    }

    /// Store `side` as the side record of the header below `window`, which `had` one at `at` or
    /// takes one there (`Walked::Header`'s `side_at`): the header's flags rewritten
    /// (`staged::call_complete::write_side_flags`), then the side record replaced, inserted or
    /// dropped where the flags come to none.
    pub(crate) fn store_side(&mut self, window: Registers, had: bool, at: usize, side: Side) {
        let flags = side.flags();
        quench_runtime::staged::call_complete::concrete_write_side_flags(window.0.expose_provenance() as u64, flags);
        let sides = self.sides.get_mut();
        match (had, flags != 0) {
            (true, true) => sides[at] = side,
            (true, false) => { sides.remove(at); }
            (false, true) => sides.insert(at, side),
            (false, false) => {}
        }
    }

    /// Change the completion of the frame at `depth` among every frame `from` one `to` another,
    /// where it is: the innermost as `recomplete_top` does, a header below it through its side
    /// record (`store_side`), a record in place. False, changing nothing, where it completes
    /// otherwise or is no script frame.
    pub(crate) fn recomplete_at(&mut self, depth: usize, from: Completion, to: Completion) -> bool {
        let Some(above) = self.activation_count().checked_sub(depth + 1) else { return false };
        if above == 0 {
            if self.top_frame().and_then(|frame| frame.completion()) != Some(from) { return false; }
            self.recomplete_top(to);
            return true;
        }
        let Some((frame, walked)) = self.walk_innermost().nth(above) else { return false };
        match walked {
            Walked::Record { index } => match self.records_mut()[index].kind_mut() {
                ActivationKind::Script(script) if script.completion == from => {
                    script.completion = to;
                    true
                }
                _ => false,
            },
            Walked::Header { window, program, side, side_at } => {
                // A trampoline's header completes through its trampoline, never `from`.
                if frame.completion() != Some(from) { return false; }
                let edited = Side { completion: to, ..side.unwrap_or_else(|| Side::plain(from, program)) };
                self.store_side(window, side.is_some(), side_at, edited);
                true
            }
        }
    }

    /// Seal the innermost plain headers below a record about to be pushed (`PlainSegment`): no
    /// spill. Nothing where none is.
    pub(crate) fn seal_plain(&mut self) {
        let depth = self.plain.depth.get();
        if depth == 0 { return; }
        let records = self.record_count();
        self.segments.get_mut().push(PlainSegment { records, depth, registers: self.plain.registers.get(), program: self.plain.program.get() });
        self.sealed.set(self.sealed.get() + depth);
        self.frames_changed();
        self.plain.depth.set(0);
        // The records change: the proved calls' bound no longer holds.
        self.plain.bound.set(0);
    }

    /// Unseal the headers the record just popped sealed, where it sealed any: they are the
    /// innermost frames again.
    pub(crate) fn unseal_plain(&mut self) {
        debug_assert_eq!(self.plain.depth.get(), 0, "a record was the innermost");
        let records = self.record_count();
        let segments = self.segments.get_mut();
        let Some(&segment) = segments.last().filter(|segment| segment.records == records) else { return };
        segments.pop();
        self.sealed.set(self.sealed.get() - segment.depth);
        self.frames_changed();
        self.plain.depth.set(segment.depth);
        self.plain.registers.set(segment.registers);
        self.plain.program.set(segment.program);
        // The bound counts from depth 0 only (`refresh_plain_bound`): the exact test decides.
        self.plain.bound.set(0);
    }

    /// The frames a spill makes records (`LaneStack::spill_plain`), innermost first: every frame
    /// above the records floor, the outermost seal's records or, where nothing is sealed, every
    /// record. The records below the floor are records already, and the side records left under
    /// the walked frames (the second count) are theirs, so the spill keeps both where they are
    /// and its cost follows the headers pushed since the last spill, not the stack's depth.
    pub(crate) fn frames_to_spill(&self) -> (usize, usize, smallvec::SmallVec<[ActivationView; 16]>) {
        // SAFETY: reads the lengths alone.
        let records = unsafe { self.records_unspilled() }.len();
        // Seals are pushed innermost last over a records count that never falls below an outer one.
        let floor = unsafe { &*self.segments.get() }.first().map_or(records, |segment| segment.records);
        let count = records - floor + self.plain.depth.get() + self.sealed.get();
        let mut walk = self.walk_innermost();
        let frames = walk.by_ref().take(count).map(|(frame, _)| frame).collect();
        (floor, walk.left, frames)
    }

    /// Forget every seal, their headers made records or taken (`spill_plain`, `take_frames`).
    ///
    /// # Safety
    /// No walk runs.
    pub(crate) unsafe fn clear_segments(&self) {
        // SAFETY: the caller's contract.
        unsafe { (*self.segments.get()).clear() };
        self.sealed.set(0);
    }

    /// Take every frame off the stack, outermost first, each as the record it becomes
    /// (`ActivationView::record`) with what its side record carries: the materialization's frames,
    /// without a spill. The windows stay in the values for the frames to take.
    pub(crate) fn take_frames(&mut self) -> Vec<(Activation, SidePayload)> {
        let mut frames: Vec<(Activation, SidePayload)> = self.frames_innermost().map(|frame| (frame.record(), frame.payload())).collect();
        frames.reverse();
        self.records_mut().truncate(0);
        self.frames_changed();
        self.plain.depth.set(0);
        self.sides.get_mut().clear();
        // SAFETY: the walk above ended.
        unsafe { self.clear_segments() };
        frames
    }

    /// Re-point the plain headers' windows where the values moved (`reserve_values`): `moved`
    /// maps an old address to its new one, and keeps any other. The header words moved with the
    /// values; the words naming a window are each header's caller registers, the trampolines'
    /// caller registers, the innermost window and each seal's.
    pub(crate) fn relocate_plain(&mut self, moved: impl Fn(usize) -> usize) {
        let relocate = |registers: Registers| Registers(std::ptr::with_exposed_provenance_mut(moved(registers.0.expose_provenance())));
        // The trampolines' words first: a trampoline's header names its caller through them.
        self.callback_return.caller_registers.set(relocate(self.callback_return.caller_registers.get()));
        for step in &mut self.steps {
            if let Some(header) = &mut step.header { header.caller_registers = relocate(header.caller_registers); }
        }
        let top = relocate(Registers(self.plain.registers.get()));
        self.plain.registers.set(top.0);
        let mut segments: Vec<(usize, Registers)> = vec![(self.plain.depth.get(), top)];
        for segment in self.segments.get_mut() {
            segment.registers = relocate(Registers(segment.registers)).0;
            segments.push((segment.depth, Registers(segment.registers)));
        }
        for (depth, mut window) in segments {
            for _ in 0..depth {
                let (call, caller, base) = window.plain_caller();
                window.set_plain_caller(call, relocate(caller), base);
                // The next header down, as the walk finds it: a trampoline's caller where this
                // header names its trampoline (a side record changes no caller window).
                window = self.plain_frame(window, None, ProgramId::MAIN).caller_registers();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn script(function: u32, caller_registers: *mut Value, completion: Completion) -> Activation {
        Activation::of_parts(
            function, std::ptr::null(), std::ptr::null(), Registers(caller_registers), ProgramId::MAIN,
            ActivationKind::Script(ScriptActivation {
                callee: Value::UNDEFINED,
                argc: 0,
                completion,
                forwards: false,
                kept: 0,
                environment: false,
                record: std::ptr::null_mut(),
            }),
        )
    }

    /// One-frame-one-call.md F4 (3): a record pushed over plain headers seals them where they
    /// are; the walk reads them below it, a header above it carries its payload in its side
    /// record, the record's pop unseals them, and only a spill makes them records.
    #[test]
    fn a_record_seals_the_headers_below_it_and_its_pop_unseals_them() {
        let mut stack = LaneStack::default();
        let base = stack.values.as_mut_ptr();
        let windows = [16, 48, 80].map(|at| base.wrapping_add(at));
        let mut heap = [Value::UNDEFINED; 8];
        // SAFETY: the windows lie in the values' reservation, each past the context words; the
        // heap window outlives the stack's use of it.
        unsafe {
            stack.push_activation(script(0, std::ptr::null_mut(), Completion::Plain), windows[0]);
            stack.push_activation(script(1, windows[0], Completion::Constructs), windows[1]);
            let mut generator = script(2, windows[1], Completion::Generates);
            generator.window = heap.as_mut_ptr().addr();
            stack.push_generator_activation(generator);
        }
        assert_eq!((stack.plain.depth.get(), stack.record_count(), stack.activation_count()), (0, 1, 3));
        // SAFETY: as above.
        unsafe { stack.push_activation(script(3, heap.as_mut_ptr(), Completion::SettlesPromise), windows[2]) };
        stack.attach_payload(SidePayload::Settles(None));
        let functions: Vec<u32> = stack.frames().map(|frame| frame.function()).collect();
        assert_eq!(functions, [0, 1, 2, 3]);
        let completions: Vec<Option<Completion>> = stack.frames_innermost().map(|frame| frame.completion()).collect();
        assert!(completions == [Some(Completion::SettlesPromise), Some(Completion::Generates), Some(Completion::Constructs), Some(Completion::Plain)]);
        let popped = stack.pop_activation().unwrap();
        assert!(matches!(popped.payload(), SidePayload::Settles(None)));
        let popped = stack.pop_activation().unwrap();
        assert!(popped.completion() == Some(Completion::Generates));
        assert_eq!((stack.plain.depth.get(), stack.plain.registers.get(), stack.record_count()), (2, windows[1], 0));
        assert!(stack.top_frame().unwrap().completion() == Some(Completion::Constructs));
        let records = stack.activations();
        assert_eq!(records.len(), 2);
        assert!(records[1].script().unwrap().completion == Completion::Constructs);
    }
}
