//! The planning of a stitched row: the rows that run before it (`preludes`), the leave and skip rows (`plan_row`),
//! the guards they hold (`number_guard`, `live_guard`, `poll_row`), which rows a body interprets (`interprets`),
//! and which rows control reaches (`reachable`).

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

/// The caller records that run as the generic semantic row (`steps::generic_row`), ascending: under
/// `LaneHelpers::Ahead`, every reached row that is a `Step::Generic`, would leave (`leaves`) or guards
/// (`Step::Guard`, whose miss is that row); elsewhere the rows that leave, where the interpreter runs them.
pub(crate) fn generic_records(layout: &Layout, decoded: &[Decoded], reached: &Bits, helpers: LaneHelpers) -> Box<[u32]> {
    let ahead = helpers.generic_rows();
    let generic = |row: &Decoded| match row.step {
        Some(Step::Generic | Step::Guard(_)) => ahead,
        _ if ahead => leaves(layout, row),
        step => step == Some(Step::Leave),
    };
    (0..decoded.len()).filter(|&k| reached.contains(k) && generic(&decoded[k]) && !layout.inlined(k as u32))
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
        Some(Step::CallOut | Step::Generic) => true,
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
        // Under `LaneHelpers::Ahead` nothing leaves: the record is its generic row, which completes the activation itself.
        let step = Decoded { completes: row.completes, ..Decoded::step(unplaced(helpers), None) };
        return plan_step(&step, layout, pc, helpers);
    }
    // The preludes come before the row's own run (a chain's spills): their leaves write the pinned registers back themselves.
    let mut planned = plan(row, layout, pc, (is_general(row, reprs), variant_of(row.kind, kinds)), helpers)?.after(preludes(layout, row, pc)?);
    // A generic row's entry completes the activation where its record returns (`semantic_stop::settle`).
    if row.completes && row.step != Some(Step::Generic) {
        planned = completing(planned, layout, pc, row.dst)?;
    }
    Ok(planned)
}
