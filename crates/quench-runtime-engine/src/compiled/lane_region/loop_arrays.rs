//! The arrays a region's loops index in place (`LoopArray`), whose views each loop's
//! entering edge proves once (`Lowering::hoist_arrays`).

use super::*;
use super::super::facts::TypedAccess;

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
            return match instruction.op() {
                _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Move { .. })) => ArraySource::Register(instruction.register_b()),
                _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) && captured(at) => ArraySource::Capture(at),
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
        ArraySource::Capture(read) => typed_capture(quench_runtime::bytecode::ImmediateLayout::capture_slot(unit.instructions[read].imm()))
            .then_some(ArraySource::Capture(read)),
    }
}

/// Each loop's array bases, by the source (`array_source`) the loop does not redefine. A
/// register's array has the region's `views`; a capture's, a typed array's. A source a lowered
/// loop around another proves too is proved there alone: its view is proved once for the whole nest,
/// where the base is invariant (`loops` hold only native operations, which cannot change a view).
/// The receiver of an append (`push`) is a writable view too, where the region's arrays are dense:
/// the append stores within the view's room (`Lowering::hoisted_push`). A search's receiver
/// (`includes`, `indexOf`) is a read-only view, scanned in place (`Lowering::element_search`).
pub(super) fn loop_arrays(unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)], leaders: &CompoundBitSet,
    captured: impl Fn(usize, usize) -> bool, views: ElementViews, typed_captures: &[u16],
    intrinsics: &super::native_calls::Intrinsics, lowered: impl Fn(usize) -> bool) -> Vec<LoopArray> {
    let mut arrays = Vec::new();
    for &(header, back) in loops {
        let body = &unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|candidate| writes(*candidate)).collect();
        let first = arrays.len();
        for (pc, instruction) in (header..).zip(body) {
            let (base, stores) = match instruction.op() {
                _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) => (instruction.operand_b().register_index(), false),
                _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) => (Some(instruction.register_b()), true),
                _ if super::native_calls::appends(intrinsics, pc) => (Some(instruction.register_c()), true),
                _ if super::native_calls::searches(intrinsics, pc) => (Some(instruction.register_c()), false),
                _ => continue,
            };
            let Some(source) = base.and_then(|base| array_source(unit, leaders, &written, |at| captured(header, at),
                |slot| typed_captures.contains(&slot), pc, base)) else { continue };
            // A double region stores raw doubles: a typed capture's raw NaN could carry a hole's
            // bits (`heap::DOUBLE_LEN`), so its elements are read boxed, NaN canonical.
            if views == ElementViews::Double && matches!(source, ArraySource::Capture(_)) { continue; }
            // An append's array is a dense register's: the view of any other kind leaves the append to its helper.
            if super::native_calls::appends(intrinsics, pc) && (views != ElementViews::Dense || matches!(source, ArraySource::Capture(_))) { continue; }
            // A search's array is a dense register's too: its scan reads the words a dense view holds.
            if super::native_calls::searches(intrinsics, pc) && (views != ElementViews::Dense || matches!(source, ArraySource::Capture(_))) { continue; }
            match arrays[first..].iter_mut().find(|array: &&mut LoopArray| array.source == source) {
                Some(array) => array.writable |= stores,
                None => {
                    let views = if matches!(source, ArraySource::Capture(_)) { ElementViews::Typed(None) } else { views };
                    arrays.push(LoopArray { header, back, source, views, writable: stores });
                }
            }
        }
    }
    arrays.iter().copied().filter(|inner| !enclosed(&arrays, inner, &lowered)).collect()
}

/// Whether a loop of `arrays` that is `lowered` and encloses `inner`'s proves its source too.
fn enclosed(arrays: &[LoopArray], inner: &LoopArray, lowered: &impl Fn(usize) -> bool) -> bool {
    arrays.iter().any(|outer| outer.source == inner.source && outer.header < inner.header
        && inner.back <= outer.back && lowered(outer.header) && lowered(outer.back))
}

impl Lowering<'_, '_> {
    /// The hoisted array `register` names at `pc` (`copied_from`, `resolved_base`), of the loop
    /// around `pc` that proved it (the outermost lowered one: `loop_arrays`), if any.
    pub(super) fn loop_array(&self, pc: usize, register: Option<Register>) -> Option<LoopArray> {
        let source = copied_from(&self.unit.instructions, |at| self.blocks[at].is_some(),
            |at| self.hoisted.get(&at).is_some_and(|read| read.representation == Representation::Tagged) && matches!(crate::compiled::performed(self.unit.instructions[at]), Some(quench_runtime::staged::Operation::LoadCapture { .. })), pc, register?);
        self.loop_arrays.iter().copied()
            .filter(|array| array.source == source && (array.header..=array.back).contains(&pc))
            .min_by_key(|array| array.back - array.header)
    }
}

impl Lowering<'_, '_> {
    /// Whether the element access at `pc` goes through a hoisted view (`element_read`,
    /// `dense_element_write`), whose miss leaves the body, so nothing runs between the access
    /// and its next instruction.
    pub(super) fn hoisted_access(&self, pc: usize) -> bool {
        let instruction = self.unit.instructions[pc];
        let base = match instruction.op().class() {
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) => instruction.operand_b().register_index(),
            _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementWrite { .. })) => Some(instruction.register_b()),
            _ => None,
        };
        self.loop_array(pc, base).is_some()
    }
}

impl Lowering<'_, '_> {
    /// What the access at `pc` does through a hoisted typed view (`Model::typed`): a `length` read of a
    /// register's typed array, which `length_read` answers from the view, or an element store through a
    /// typed or double view, which the held view writes or leaves at its miss (`element_write`). Any
    /// other access, or one through a view that is not hoisted, is `None`.
    pub(super) fn typed_access(&self, pc: usize) -> Option<TypedAccess> {
        let instruction = self.unit.instructions[pc];
        let (base, access) = match crate::compiled::performed(instruction) {
            Some(quench_runtime::staged::Operation::ElementWrite { .. }) if !instruction.returns_from_frame() => (Some(instruction.register_b()), TypedAccess::Store),
            _ if crate::compiled::reads_length(self.program, instruction) => match instruction.field_lookup() {
                Some(FieldLookup::Atom { base, .. }) => (base.register_index(), TypedAccess::Length),
                _ => return None,
            },
            _ => return None,
        };
        let array = self.loop_array(pc, base)?;
        let typed = match access {
            TypedAccess::Length => matches!(array.views, ElementViews::Typed(_)) && matches!(array.source, ArraySource::Register(_)),
            TypedAccess::Store => matches!(array.views, ElementViews::Typed(_) | ElementViews::Double) && array.writable,
        };
        typed.then_some(access)
    }
}
