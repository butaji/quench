//! The run of a stencil region that needs no activation: `Crossing::Bare`.

#[cfg(test)]
use super::LANE_OSR_RUNS;
use super::{enter_native, CallContext, Entry, LaneExit, LoopEntry, LoopOutcome, LoopRegion, NativeEntryMode, Raised};


/// A bare run's payload is the entry record's address alone, where the dispatch holds it.
const _: () = assert!(std::mem::offset_of!(super::JsStepDispatch, loop_entry) == 0);

/// What a run of a region builds around its code.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Crossing {
    /// The general tier's activation, dispatch and views.
    General,
    /// A stencil region that reads the heap: the activation and dispatch, without the views.
    Lean,
    /// A stencil region that reads no heap row: a payload of the entry record's address only (`run_bare`).
    Bare,
}

/// Runs a `Crossing::Bare` region on the register file in place: no activation, dispatch or
/// view is built, the payload is the entry record's address.
///
/// # Safety
/// As `run`.
pub(super) unsafe fn run_bare(
    entry_code: Entry,
    registers: *mut crate::value::Value,
    entry: &mut LoopEntry,
    region: &LoopRegion,
    interrupt: *const crate::execution_interrupt::ExecutionInterrupt,
) -> LoopOutcome {
    #[cfg(test)]
    LANE_OSR_RUNS.with(|count| count.set(count.get() + 1));
    // The code reads the interrupt flag and the entry record's address (word 0 of its payload, `JsStepDispatch::loop_entry`).
    let mut payload = entry.words_mut() as usize;
    let mut context = CallContext::interrupt_only(unsafe { &*interrupt });
    context.adapter_payload = (&mut payload as *mut usize).cast();
    let mut word = 0u64;
    // The code takes the frame, whose window is the register file it runs on.
    let mut frame = super::ShadowFrame { window: registers as usize, ..super::ShadowFrame::EMPTY };
    // SAFETY: as the caller promises; the code reads the register file through the frame and writes the result word.
    let status = unsafe { enter_native(&mut context, 0, 0, (&raw mut frame).cast::<u64>(), &mut word, NativeEntryMode::Resumable, entry_code) };
    if let Some(Raised::Panic(panic)) = context.raised.take() {
        std::panic::resume_unwind(panic);
    }
    LoopOutcome::of(LaneExit::read_region(status, word, || unreachable!("a bare region reads no heap row, so it completes no activation")), region)
}

