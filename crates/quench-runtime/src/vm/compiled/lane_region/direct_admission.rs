//! What admits a native call: the room both stacks keep for its window and recovery, and the
//! call-protocol rows (`staged::protocol`) a lowering steps.

use super::*;
use super::super::entry_cell::EntryCell;
use super::super::super::dispatch_fast::LaneRecoverySpace;

impl Lowering<'_, '_> {
    /// The word of call-protocol `row` stepped on the emit stage (`staged::protocol`): a failed
    /// guard branches to `miss`, one compare and branch each.
    pub(super) fn call_row(&mut self, state: &mut State, miss: ir::Block, row: crate::staged::CallOperation<ir::Value>) -> ir::Value {
        match self.step(state, super::frame::Edges::ValueGuarded { miss }, crate::staged::Operation::Call(row)) {
            crate::staged::Step::Value(word) => word,
            _ => unreachable!("a call-protocol row with its operands yields its word"),
        }
    }

    /// The lane stack's top and the callee window's end, and the shadow stack's top, admitting
    /// that both stacks hold the window and the space the callee's published body needs to
    /// recover (`EntryCell::recovery`), each a room row leaving at `helper`.
    pub(super) fn direct_room(&mut self, state: &mut State, callee: &DirectCallee, cell: ir::Value, helper: ir::Block) -> (ir::Value, ir::Value, ir::Value) {
        let word = std::mem::size_of::<Value>() as i64;
        let payload = self.payload;
        let load = |this: &mut Self, base: ir::Value, field: usize| this.word_load(base, dispatch_offset(field));
        let top = load(self, payload, std::mem::offset_of!(JsStepDispatch, native_top));
        let end = load(self, payload, std::mem::offset_of!(JsStepDispatch, native_end));
        let next = self.word_add_imm(top, i64::from(callee.registers) * word);
        let recovery = load(self, cell, std::mem::offset_of!(EntryCell, recovery));
        let values = load(self, recovery, std::mem::offset_of!(LaneRecoverySpace, values));
        let bytes = self.word_mul_imm(values, word);
        let reserved = self.word_add(next, bytes);
        self.call_row(state, helper, crate::staged::CallOperation::Room { limit: end, needed: reserved });
        let shadow = load(self, payload, std::mem::offset_of!(JsStepDispatch, shadow_top));
        let shadow_end = load(self, payload, std::mem::offset_of!(JsStepDispatch, shadow_end));
        let activations = load(self, recovery, std::mem::offset_of!(LaneRecoverySpace, activations));
        let frames = self.word_add_imm(activations, 1);
        let frame_bytes = self.word_mul_imm(frames, super::super::FRAME_BYTES as i64);
        let recovered = self.word_add(shadow, frame_bytes);
        self.call_row(state, helper, crate::staged::CallOperation::Room { limit: shadow_end, needed: recovered });
        (top, next, shadow)
    }
}

/// A dispatch or cell field's offset as the loads take it.
pub(super) fn dispatch_offset(field: usize) -> i32 {
    i32::try_from(field).expect("dispatch offset fits i32")
}
