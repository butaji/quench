//! The arrays a region's loops index in place (`LoopArray`), whose views each loop's
//! entering edge proves once (`Lowering::hoist_arrays`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::facts::TypedAccess;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// An array base a local loop indexes and never redefines: its loop's entry proves the
/// array's view once, writable where the loop stores through it, and only the element
/// store's general path, which may grow the backing, loads the view again.
#[derive(Clone, Copy)]
pub(crate) struct LoopArray {
    pub(crate) header: usize,
    pub(crate) back: usize,
    /// The register holding the array, which the loop never writes.
    pub(crate) source: Register,
    /// Whether the array's elements are viewed as a dense array's or a typed array's.
    pub(crate) views: ElementViews,
    pub(crate) writable: bool,
}

/// The register whose value `register` holds at `pc` by a copy earlier in its block (`Move`,
/// the copy not written again before `pc`), else `register` itself: an access through a copy
/// of a loop's array indexes the array the loop hoisted.
fn copied_from(instructions: &[WideInstruction], leader: impl Fn(usize) -> bool, pc: usize, register: Register) -> Register {
    let mut at = pc;
    while at > 0 && !leader(at) {
        at -= 1;
        let instruction = instructions[at];
        if instruction.written_registers().any(|written| written == register) {
            return match crate::compiled::decoded(instruction) {
                Some(quench_runtime::staged::Operation::Move { .. }) => instruction.register_b(),
                _ => register,
            };
        }
    }
    register
}

/// A loop's array register at `pc`: the register the loop does not redefine, through its copies.
fn array_source(unit: &super::super::lane_unit::Unit, leaders: &quench_runtime::dataflow::BitSet, written: &BTreeSet<Register>,
    pc: usize, base: Register) -> Option<Register> {
    // The loop's sole entering edge proves the view of any base its body does not
    // redefine, an inlined frame's argument as well as the caller's own register.
    let register = copied_from(&unit.instructions, |at| leaders.contains(at), pc, base);
    (!written.contains(&register)).then_some(register)
}

/// Each loop's array bases, by the register (`array_source`) the loop does not redefine. An
/// array's view is the region's `views`. A source a lowered loop around another proves too is
/// proved there alone: its view is proved once for the whole nest, where the base is invariant
/// (`loops` hold only native operations, which cannot change a view).
/// The receiver of an append (`push`) is a writable view too, where the region's arrays are dense:
/// the append stores within the view's room (`Lowering::hoisted_push`). A search's receiver
/// (`includes`, `indexOf`) is a read-only view, scanned in place (`Lowering::element_search`).
pub(crate) fn loop_arrays(unit: &super::super::lane_unit::Unit, loops: &[(usize, usize)], leaders: &quench_runtime::dataflow::BitSet,
    views: ElementViews, kind_at: impl Fn(usize) -> Option<quench_runtime::tier_surface::TypedArrayKind>,
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
            let Some(source) = base.and_then(|base| array_source(unit, leaders, &written, pc, base)) else { continue };
            // An append's array is a dense register's: the view of any other kind leaves the append to its helper.
            if super::native_calls::appends(intrinsics, pc) && views != ElementViews::Dense { continue; }
            // A search's array is a dense register's too: its scan reads the words a dense view holds.
            if super::native_calls::searches(intrinsics, pc) && views != ElementViews::Dense { continue; }
            match arrays[first..].iter_mut().find(|array: &&mut LoopArray| array.source == source) {
                Some(array) => array.writable |= stores,
                None => {
                    // The kind the solve proves of the access's typed array is the arm its accesses keep.
                    let views = kind_at(pc).map_or(views, |kind| ElementViews::Typed(Some(kind)));
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
    pub(crate) fn loop_array(&self, pc: usize, register: Option<Register>) -> Option<LoopArray> {
        let source = copied_from(&self.unit.instructions, |at| self.blocks[at].is_some(), pc, register?);
        self.loop_arrays.iter().copied()
            .filter(|array| array.source == source && (array.header..=array.back).contains(&pc))
            .min_by_key(|array| array.back - array.header)
    }
}

impl Lowering<'_, '_> {
    /// Whether the element access at `pc` goes through a hoisted view (`element_read`,
    /// `dense_element_write`), whose miss leaves the body, so nothing runs between the access
    /// and its next instruction.
    pub(crate) fn hoisted_access(&self, pc: usize) -> bool {
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
    pub(crate) fn typed_access(&self, pc: usize) -> Option<TypedAccess> {
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
            TypedAccess::Length => matches!(array.views, ElementViews::Typed(_)),
            TypedAccess::Store => matches!(array.views, ElementViews::Typed(_) | ElementViews::Double) && array.writable,
        };
        typed.then_some(access)
    }
}

impl Lowering<'_, '_> {
    /// The payload's typed view records (`JsStepHead::typed_view_directory`), which a view proof reads.
    pub(crate) fn typed_records(&mut self) -> quench_runtime::staged::typed_view::TypedRecords<ir::Value> {
        let mut word = |offset| emit_js_payload_word(&mut self.builder, self.payload, offset);
        quench_runtime::staged::typed_view::TypedRecords {
            directory: word(std::mem::offset_of!(JsStepDispatch, head.typed_view_directory)),
            pages: word(std::mem::offset_of!(JsStepDispatch, head.typed_view_pages)),
            relocations: word(std::mem::offset_of!(JsStepDispatch, head.buffer_relocations)),
        }
    }
}
