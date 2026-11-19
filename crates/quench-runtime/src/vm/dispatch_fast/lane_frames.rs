//! The one frame view (`/mnt/project-files/calls-frames/one-frame-one-call.md`, step F1): every
//! reader of the lane's activations, whether the activation is a record (`Activation`) or a plain
//! activation's header in the value stack (call protocol v2 stage C), reads it through
//! `FrameView`. No read through it spills the plain activations (`LaneStack::spill_plain`), and
//! the readers stop naming the record, so a later step can take the record away from the plain
//! frames' readers without touching them again.
//!
//! Writers still go through `LaneStack::activations_mut`, which spills: a view is a copy.
use super::*;

/// One activation as a reader sees it: a record, or a plain header read as the record its spill
/// would write (`LaneStack::plain_record`), so a reader answers the same either way.
#[derive(Clone, Copy)]
pub(in crate::vm) struct FrameView {
    record: Activation,
    /// Whether the activation is a plain header in the value stack, not a record.
    plain: bool,
}

#[allow(dead_code)] // the accessors the remaining readers (F2/F3) take over from `Activation`
impl FrameView {
    #[inline(always)]
    fn of_record(record: &Activation) -> Self { Self { record: *record, plain: false } }

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
    /// plainly or returns to a native through the callback trampoline).
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
    /// Whether it is a plain header in the value stack.
    #[inline(always)] pub(in crate::vm) fn is_plain(&self) -> bool { self.plain }
    /// The record it is, or would be once spilled, for the readers that still take an
    /// `&Activation` (`lane_restore_caller`). Transitional: step F4 removes it.
    #[inline(always)] pub(super) fn record(&self) -> &Activation { &self.record }
}

impl LaneStack {
    /// The record the plain activation whose window is `window` becomes when it spills, and its
    /// caller's window, the next plain activation's where one remains below it. A native's
    /// callback (`CallbackTrampoline`), always the outermost plain activation, becomes
    /// `PreparedShape::of`'s record. The one definition `spill_plain` and the frame view share.
    pub(super) fn plain_record(&self, window: Registers) -> (Activation, Registers) {
        let (call, caller_registers, caller_base) = window.plain_caller();
        let (call, record_caller_registers, completion) = self.callback_return.returns(call)
            .map_or((call, caller_registers, Completion::Plain), |(call, registers)| (call, registers, Completion::ReturnsToNative));
        let (function, argc, callee) = window.plain_callee();
        let mut activation = Activation::of_parts(
            crate::vm::compiled::ShadowFrame::EMPTY, function, call, caller_base, record_caller_registers, self.plain.program.get(),
            ActivationKind::Script(ScriptActivation {
                callee,
                argc,
                completion,
                forwards: false,
                kept: 0,
                environment: false,
                record: std::ptr::null_mut(),
            }),
        );
        activation.native.window = window.0.addr();
        (activation, record_caller_registers)
    }

    /// The records alone, read without spilling.
    ///
    /// # Safety
    /// No `activations` call (which may spill through a shared reference) happens while the
    /// returned reference lives.
    #[inline(always)]
    unsafe fn records_unspilled(&self) -> &LaneActivations {
        // SAFETY: the caller's contract; only `spill_plain` writes through a shared reference.
        unsafe { &*self.activation_records.get() }
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
            let (record, _) = self.plain_record(Registers(self.plain.registers.get()));
            return Some(FrameView { record, plain: true });
        }
        // SAFETY: the reference ends here.
        unsafe { self.records_unspilled() }.last().map(FrameView::of_record)
    }

    /// The activation `depth` above the outermost, as `activations()[depth]` would read it once
    /// spilled; `None` past the innermost.
    pub(in crate::vm) fn frame(&self, depth: usize) -> Option<FrameView> {
        // SAFETY: the reference ends within this call.
        let records = unsafe { self.records_unspilled() };
        if let Some(record) = records.get(depth) {
            return Some(FrameView::of_record(record));
        }
        let above = depth - records.len();
        let plain = self.plain.depth.get();
        if above >= plain { return None; }
        let mut window = Registers(self.plain.registers.get());
        for _ in 0..plain - 1 - above {
            window = window.plain_caller().1;
        }
        Some(FrameView { record: self.plain_record(window).0, plain: true })
    }

    /// Every activation, outermost first, as `activations().iter()` would yield them once
    /// spilled, without spilling: the records, then the plain headers. The walk reads no
    /// record a spill could write during it: a reader calls no `activations` while it walks.
    pub(in crate::vm) fn frames(&self) -> impl Iterator<Item = FrameView> + '_ {
        // SAFETY: the walk's contract above.
        let records = unsafe { self.records_unspilled() };
        let depth = self.plain.depth.get();
        let mut plain = Vec::with_capacity(depth);
        let mut window = Registers(self.plain.registers.get());
        for _ in 0..depth {
            let (record, caller) = self.plain_record(window);
            plain.push(FrameView { record, plain: true });
            window = caller;
        }
        records.iter().map(FrameView::of_record).chain(plain.into_iter().rev())
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
