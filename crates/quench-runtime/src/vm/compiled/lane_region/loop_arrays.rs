//! The arrays a region's loops index in place (`LoopArray`), whose views each loop's
//! entering edge proves once (`Lowering::hoist_arrays`).

use super::*;

/// An array base a local loop indexes and never redefines: its loop's entry proves the
/// array's view once, writable where the loop stores through it, and only the element
/// store's general path, which may grow the backing, loads the view again.
#[derive(Clone, Copy)]
pub(super) struct LoopArray {
    pub(super) header: usize,
    pub(super) back: usize,
    pub(super) source: ArraySource,
    /// Whether the array's elements are viewed as a dense array's or a typed array's.
    pub(super) views: ElementViews,
    pub(super) writable: bool,
}

/// Where a loop's array comes from: a register the loop never writes, or a capture read the
/// loop's entry hoisted (`hoisted_reads`), whose register the body may reuse.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ArraySource {
    Register(Register),
    Capture(usize),
}

/// The source whose value `register` holds at `pc` by a copy earlier in its block (`Move`,
/// the copy not written again before `pc`) or by a hoisted capture read (`captured`), else
/// `register` itself: an access through a copy of a loop's array indexes the array the loop
/// hoisted.
fn copied_from(instructions: &[WideInstruction], leader: impl Fn(usize) -> bool, captured: impl Fn(usize) -> bool,
    pc: usize, register: Register) -> ArraySource {
    let mut at = pc;
    while at > 0 && !leader(at) {
        at -= 1;
        let instruction = instructions[at];
        if instruction.written_registers().any(|written| written == register) {
            return match instruction.op().class() {
                Class::Move => ArraySource::Register(instruction.register_b()),
                Class::LoadCapture if captured(at) => ArraySource::Capture(at),
                _ => ArraySource::Register(register),
            };
        }
    }
    ArraySource::Register(register)
}

/// A loop's array source at `pc`: a register the loop does not redefine, or a hoisted capture
/// read (`captured`) of a typed array when compiled (`typed_capture`).
fn array_source(unit: &super::super::lane_unit::Unit, leaders: &CompoundBitSet, written: &BTreeSet<Register>,
    captured: impl Fn(usize) -> bool, typed_capture: impl Fn(u16) -> bool, pc: usize, base: Register) -> Option<ArraySource> {
    // The loop's sole entering edge proves the view of any base its body does not
    // redefine, an inlined frame's argument as well as the caller's own register.
    match copied_from(&unit.instructions, |at| leaders.contains(at), captured, pc, base) {
        ArraySource::Register(register) => (!written.contains(&register)).then_some(ArraySource::Register(register)),
        ArraySource::Capture(read) => typed_capture(crate::bytecode::ImmediateLayout::capture_slot(unit.instructions[read].imm()))
            .then_some(ArraySource::Capture(read)),
    }
}

/// Each loop's array bases, by the source (`array_source`) the loop does not redefine. A
/// register's array has the region's `views`; a capture's, a typed array's.
pub(super) fn loop_arrays(unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)], leaders: &CompoundBitSet,
    captured: impl Fn(usize, usize) -> bool, views: ElementViews, typed_captures: &[u16]) -> Vec<LoopArray> {
    let mut arrays = Vec::new();
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let first = arrays.len();
        for (pc, instruction) in (header..).zip(body) {
            let (base, stores) = match instruction.op().class() {
                Class::GetIndex => (instruction.operand_b().register_index(), false),
                Class::SetIndex => (Some(instruction.register_b()), true),
                _ => continue,
            };
            let Some(source) = base.and_then(|base| array_source(unit, leaders, &written, |at| captured(header, at),
                |slot| typed_captures.contains(&slot), pc, base)) else { continue };
            match arrays[first..].iter_mut().find(|array: &&mut LoopArray| array.source == source) {
                Some(array) => array.writable |= stores,
                None => {
                    let views = if matches!(source, ArraySource::Capture(_)) { ElementViews::Typed } else { views };
                    arrays.push(LoopArray { header, back, source, views, writable: stores });
                }
            }
        }
    }
    arrays
}

impl Lowering<'_, '_> {
    /// The hoisted array `register` names at `pc` (`copied_from`), of the innermost loop
    /// around `pc` that proved it, if any.
    pub(super) fn loop_array(&self, pc: usize, register: Option<Register>) -> Option<LoopArray> {
        let source = copied_from(&self.unit.instructions, |at| self.blocks[at].is_some(),
            |at| self.hoisted.get(&at).is_some_and(|read| read.representation == Representation::Tagged) && self.unit.instructions[at].op().class().eq(Class::LoadCapture), pc, register?);
        self.loop_arrays.iter().copied()
            .filter(|array| array.source == source && (array.header..=array.back).contains(&pc))
            .min_by_key(|array| array.back - array.header)
    }
}
