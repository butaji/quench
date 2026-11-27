//! The planning of a stitched row: the rows that run before it (`preludes`), the leave and skip rows (`plan_row`),
//! the guards they hold (`number_guard`, `live_guard`, `poll_row`), which rows a body interprets (`interprets`),
//! and which rows control reaches (`reachable`).

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Which rows control reaches from row 0; an error where one falls off the program's end.
pub(crate) fn reachable(span: &Span, decoded: &[Decoded]) -> Result<Bits, String> {
    let count = decoded.len();
    let (mut reached, mut pending) = (Bits::default(), vec![0]);
    while let Some(k) = pending.pop() {
        if reached.contains(k) {
            continue;
        }
        reached.insert(k);
        for next in decoded[k].successors(k, count, span) {
            match next {
                Next::Row(row) => pending.push(row),
                Next::Leave(_) => return Err("a row falls off the function".to_string()),
            }
        }
    }
    Ok(reached)
}

/// A row that leaves at `pc` unless the frame slot at byte offset `slot` holds a Number.
pub(crate) fn number_guard(pc: u32, slot: u64) -> Result<Planned, String> {
    Ok(Planned { stop: Some(Stop { pc, returns: None }), ..Planned::single(control(Kind::Guard)?, padded_holes([0, slot]), None, None) })
}

/// A row that leaves at `pc` once the invalidation word at `address` is set (`Heap::Live`): a Stable
/// fact the code holds changed under a call, so the interpreter makes the instruction.
pub(crate) fn live_guard(pc: u32, address: usize) -> Result<Planned, String> {
    let row = HeapRow { heap: Heap::Live, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    Ok(Planned::single(base_rows::stencil(row)?, padded_holes([0, address as u64]), None, Some(pc)))
}

/// A row that leaves at `pc` when an interrupt is requested, and continues.
pub(crate) fn poll_row(pc: u32) -> Result<Planned, String> {
    let interrupt = std::mem::offset_of!(super::super::super::CallContext, head.interrupt) as u64;
    let attention = quench_runtime::execution_interrupt::attention_address();
    Ok(Planned::single(control(Kind::Poll)?, padded_holes([interrupt, 0, 0, attention]), None, Some(pc)))
}

/// The rows that run before the row `pc`, at the same row: the check that a Stable fact the row
/// holds still holds, the proof a `ToNumeric` operand held in a frame slot is a Number (a loop's
/// entry guards prove the registers it names itself, a function's registers and those of an inlined
/// callee are not proved), and the poll a conditional branch back takes (an unconditional one is its
/// own `JumpWatch`), so no loop a function runs goes unwatched; a region's own jumps count and poll
/// (`JumpPoll`).
pub(crate) fn preludes(layout: &Layout, row: &Decoded, pc: u32) -> Result<Vec<Planned>, String> {
    let (place, slot) = operand(&layout.homes, row.a);
    let body = layout.ending == Ending::Body;
    let mut before = Vec::new();
    if let Some(address) = layout.invalidation.filter(|_| row.holds_stable()) {
        before.push(live_guard(pc, address)?);
    }
    // A declared Number parameter of this body (not an inlined callee's register) is proved by the entry's guard.
    let proven = !layout.inlined(pc) && row.a.and_then(register_of).is_some_and(|register| layout.numbers.contains(&register));
    // A checked operand that is a Number on every path into the row needs no guard (`numbers::proven_operands`).
    let held = layout.number_held.get(pc as usize).copied().unwrap_or(false);
    if row.checked && place == Place::Mem && (body || layout.inlined(pc)) && !proven && !held {
        before.push(number_guard(pc, slot)?);
    }
    if body && row.kind != Kind::Jump && row.target.is_some_and(|target| target <= pc) {
        before.push(poll_row(pc)?);
    }
    Ok(before)
}

/// Whether the register a heap row reads its array, object or callee through is held in its frame
/// slot, where the row reads it: a pinned register holds a Number, never one of those, and its
/// slot is not kept current.
pub(crate) fn framed(homes: &Homes, row: &Decoded) -> bool {
    let through = match row.heap {
        Some(Heap::Store | Heap::TypedStore(_) | Heap::SiteStore) => row.base,
        Some(Heap::Index | Heap::TypedIndex(_) | Heap::TypedLength | Heap::Site | Heap::SiteChain | Heap::Length | Heap::Held | Heap::Identity | Heap::CharCodeAt | Heap::ParseInt) => row.a.and_then(register_of),
        _ => None,
    };
    through.into_iter().all(|register| homes.place(register) == Place::Mem)
}

/// Whether the row leaves the body: a leave step, or a heap row whose array is not in its frame slot.
pub(crate) fn leaves(layout: &Layout, row: &Decoded) -> bool {
    row.step == Some(Step::Leave) || !framed(&layout.homes, row)
}

/// The caller records whose rows leave, ascending, where the interpreter runs them: an image body's as a JIT body's.
pub(crate) fn generic_records(layout: &Layout, decoded: &[Decoded], reached: &Bits) -> Box<[u32]> {
    (0..decoded.len()).filter(|&k| reached.contains(k) && decoded[k].step == Some(Step::Leave) && !layout.inlined(k as u32))
        .map(|k| layout.record(k as u32)).collect()
}

/// Whether `row` interprets its instruction rather than running its handler's residue: the general
/// step a lane helper makes from the instruction's PC (re-decoding it and dispatching on its form,
/// hundreds of instructions where the interpreter's quickened handler spends tens). The design is a
/// ladder, b-jit at most the interpreter's instructions on every operation (b-jit is the interpreter
/// with dispatch and decode folded), so a loop region holding one on its loop is not built and the
/// interpreter runs the loop. A helper step with a packed row (`Decoded::packed`: a family runs the
/// instruction without the PC re-decode, `family_helpers`) is built; the others, and a call-out, are
/// not. A call keeps its call row in every build: a loop that calls is built,
/// its calls direct rows into the callee's cell or the helper row, and the tier-ladder gate
/// (`quench-gate --only ir-budget`: call_direct, call_closure, k03_call_add) holds the call protocol
/// at most the interpreter's. A fast arm's slow arm (`Step::Fast`), a throw and a leave are off the
/// loop's steady path.
pub(crate) fn interprets(row: &Decoded) -> bool {
    match row.step {
        Some(Step::Helper) => row.packed.is_none(),
        Some(Step::CallOut) => true,
        _ => false,
    }
}

/// The plan of row `pc`: nothing where control never reaches it, a leave row, or the row with the
/// rows before it its kind needs.
pub(crate) fn plan_row(layout: &Layout, (reprs, kinds): (&Reprs, Option<&OperandKinds>), rows: &[Decoded], reached: bool, pc: u32, row: &Decoded, helpers: LaneHelpers) -> Result<Planned, String> {
    if !reached {
        return Ok(skipped());
    }
    if row.step == Some(Step::Switch) {
        return super::switch::switch_run(layout, (reprs, kinds), rows, pc, row, helpers);
    }
    if leaves(layout, row) {
        return plan_step(&Decoded::step(Step::Leave, None), layout, pc, helpers);
    }
    // The preludes come before the row's own run (a chain's spills): their leaves write the pinned registers back themselves.
    let mut planned = plan(row, layout, pc, (is_general(row, reprs), variant_of(row.kind, kinds)), helpers)?.after(preludes(layout, row, pc)?);
    if row.completes {
        planned = completing(planned, layout, pc, row.dst)?;
    }
    Ok(planned)
}

/// The loop heads of a body's `function` whose interpreter kernel (installed on the head record by
/// `loop_kernel::install_loop_kernels`) runs the loop in fewer instructions than the rows (`LoopKernel::beats_rows`),
/// dropped from the body's `entries`: a body is not entered at a kernel head, where the interpreter's head runs the
/// kernel (`kernel_entries`). A region keeps them all.
pub(crate) fn kernel_heads(ending: Ending, function: &Function, constants: &[Value], live: Option<&quench_runtime::compile::liveness::RegisterMasks>, entries: &mut Vec<(u32, u32)>) -> Vec<u32> {
    if ending != Ending::Body {
        return Vec::new();
    }
    let heads = body_kernel_heads(function, constants, live);
    entries.retain(|(pc, _)| *pc == 0 || !heads.contains(pc));
    heads
}

fn body_kernel_heads(function: &Function, constants: &[Value], live: Option<&quench_runtime::compile::liveness::RegisterMasks>) -> Vec<u32> {
    let code: Vec<_> = function.instructions().collect();
    let jump = |word: quench_runtime::bytecode::WideInstruction| matches!(quench_runtime::staged::Operation::<(), ()>::decode(word, quench_runtime::staged::DecodeContext::EMPTY), Some(quench_runtime::staged::Operation::Jump));
    code.iter().enumerate().filter(|&(backedge, &word)| jump(word) && (word.jump_target() as usize) < backedge)
        .filter(|&(backedge, &word)| crate::dispatch_fast::loop_kernel::LoopKernel::beats_rows(&code, constants, function, (word.jump_target() as usize, backedge), live))
        .map(|(_, word)| word.jump_target()).collect()
}

/// The interpreter's loop kernels on their loops' entry edges: a body loop whose head is a kernel head (`kernel_heads`)
/// runs the kernel as the interpreter's head does, through the kernel helper (`LaneHelper::KernelStep`) after the row
/// that falls into the head, so the entry pays the kernel's entry once and the back edge nothing. The body is not
/// entered at such a head (`assemble` drops it from the entries), where the interpreter's head runs the kernel. The
/// helper reads the head record, so a head without a kernel (stood down, or not installed) runs the rows; where the
/// kernel ran, the body leaves at the head and the interpreter goes on after the loop. The kernel is the head's own
/// semantics, the one definition every tier derives (S1.11).
pub(crate) fn kernel_entries(layout: &Layout, heads: &[u32], decoded: &[Decoded], reached: &Bits, planned: &mut [Planned], helpers: LaneHelpers) -> Result<(), String> {
    if layout.ending != Ending::Body {
        return Ok(());
    }
    for &header in heads {
        let Some(row) = (1..decoded.len() as u32).find(|&row| !layout.inlined(row) && layout.record(row) == header) else { continue };
        let before = row as usize - 1;
        // The row falls into the head: no jump, and a conditional branch only on its fall-through (`if (n > 0) do ...`),
        // where its run's parts after it (`Planned::then_run`) are laid out.
        let falls = reached.contains(before) && !layout.inlined(row - 1) && decoded[before].kind != Kind::Jump && !leaves(layout, &decoded[before]);
        if !falls || planned[before].stop.is_some() || matches!(planned[before].branch, Some(Branch::Part(_))) {
            continue;
        }
        let chain = super::steps::helper_chain(layout, row, &layout.spills(row), (super::super::super::LaneHelper::KernelStep, helpers), None)?;
        planned[before].then_run(chain);
    }
    Ok(())
}
