//! The run of a stencil region that needs no activation: `Crossing::Bare`.

use super::entry_protocol::{self, EntryDescriptor, EntryKind};
use super::{BodyEntry, CallContext, LoopEntry, RegionRun, Raised};


/// A bare run's payload is the entry record's address alone, where the dispatch holds it.
const _: () = assert!(std::mem::offset_of!(super::JsStepDispatch, head.loop_entry) == 0);

pub(crate) use quench_runtime::vm::engine_types::Crossing;

/// Runs a `Crossing::Bare` region on the register file in place: no activation, dispatch or
/// view is built, the payload is the entry record's address.
///
/// # Safety
/// As `run`.
pub(crate) unsafe fn run_bare(
    (entry_code, resume): (BodyEntry, u64),
    registers: *mut quench_runtime::value::Value,
    entry: &mut LoopEntry,
    interrupt: *const quench_runtime::execution_interrupt::ExecutionInterrupt,
) -> RegionRun {
    // The code reads the interrupt flag and the entry record's address (word 0 of its payload, `JsStepDispatch::loop_entry`).
    let mut payload = entry.words_mut() as usize;
    let mut context = CallContext::interrupt_only(unsafe { &*interrupt });
    context.adapter_payload = (&mut payload as *mut usize).cast();
    // SAFETY: as the caller promises; the code runs on the register file and returns its status and result
    // word. A resume entry takes its stub's address where a body takes its captures.
    let descriptor = EntryDescriptor { kind: EntryKind::Stencil, receiver: 0, captures: resume, registers: registers.cast::<u64>().cast_const(), entry: entry_code };
    let returned = unsafe { entry_protocol::enter(&mut context, &descriptor) };
    if let Some(Raised::Panic(panic)) = context.raised.take() {
        std::panic::resume_unwind(panic);
    }
    RegionRun::own(entry_protocol::exit(&descriptor, returned, || unreachable!("a bare region reads no heap row, so it completes no activation")))
}

