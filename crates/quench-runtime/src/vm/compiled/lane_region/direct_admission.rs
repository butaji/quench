//! What admits a native call: the callee and receiver it names, and the room both stacks
//! keep for its window and recovery.

use super::*;
use super::super::entry_cell::EntryCell;
use super::super::super::dispatch_fast::LaneRecoverySpace;

/// The lane stack's two tops a native call pushes onto: the words that hold them (the values'
/// and the activations', which the VM owns and the dispatch points at), and what they held
/// before the call, where its window and its frame begin; `next` is the values' top once the
/// callee's window is pushed.
#[derive(Clone, Copy)]
pub(super) struct DirectStacks {
    pub(super) values: ir::Value,
    pub(super) activations: ir::Value,
    pub(super) top: ir::Value,
    pub(super) next: ir::Value,
    pub(super) frame: ir::Value,
}

impl DirectStacks {
    /// Pops both stacks to where the call began: its window and its frame go.
    pub(super) fn pop(self, lowering: &mut Lowering<'_, '_>) {
        lowering.word_store(self.top, self.values, 0);
        lowering.word_store(self.frame, self.activations, 0);
    }
}

impl Lowering<'_, '_> {
    /// What admits the callee and receiver the call names (the descriptor's), and the `this`
    /// it binds. A sloppy callee binds an object receiver itself and an `undefined` or `null`
    /// one to the global object (`lane_receiver_this`); any other receiver takes the helper.
    pub(super) fn direct_receiver(&mut self, instruction: WideInstruction, callee: &DirectCallee) -> (Vec<ir::Value>, ir::Value) {
        let base = self.registers_base;
        let callee_bits = self.word_load(base, register_offset(instruction.register_b()));
        let same = self.word_cmp_imm(IntCC::Equal, callee_bits, callee.callee.to_bits() as i64);
        let this = self.word_load(base, register_offset(instruction.register_c()));
        let Some(global) = callee.sloppy else { return (vec![same], this) };
        let object = crate::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), this);
        let undefined = self.word_cmp_imm(IntCC::Equal, this, Value::UNDEFINED.to_bits() as i64);
        let null = self.word_cmp_imm(IntCC::Equal, this, Value::NULL.to_bits() as i64);
        let nullish = self.word_or(undefined, null);
        let bound = self.word_or(object, nullish);
        let global = self.word_const(global.to_bits() as i64);
        (vec![same, bound], self.word_select(object, this, global))
    }

    /// The lane stack's two tops, each read through the word that holds it (the values' and
    /// the activations'), and whether both stacks hold the window, the frame and the space the
    /// callee's published body needs to recover (`EntryCell::recovery`), each as its own
    /// condition so each is one compare and branch (`direct_admit`).
    pub(super) fn direct_room(&mut self, callee: &DirectCallee, cell: ir::Value) -> (DirectStacks, [ir::Value; 2]) {
        let word = std::mem::size_of::<Value>() as i64;
        let payload = self.payload;
        let load = |this: &mut Self, base: ir::Value, field: usize| this.word_load(base, dispatch_offset(field));
        let values = load(self, payload, std::mem::offset_of!(JsStepDispatch, lane_values_top));
        let top = self.word_load(values, 0);
        let end = load(self, payload, std::mem::offset_of!(JsStepDispatch, lane_values_end));
        let next = self.word_add_imm(top, i64::from(callee.registers) * word);
        let recovery = load(self, cell, std::mem::offset_of!(EntryCell, recovery));
        let recovered_values = load(self, recovery, std::mem::offset_of!(LaneRecoverySpace, values));
        let bytes = self.word_mul_imm(recovered_values, word);
        let reserved = self.word_add(next, bytes);
        let fits = self.word_cmp(IntCC::UnsignedLessThanOrEqual, reserved, end);
        let activations = load(self, payload, std::mem::offset_of!(JsStepDispatch, lane_activations_top));
        let frame = self.word_load(activations, 0);
        let frames_end = load(self, payload, std::mem::offset_of!(JsStepDispatch, lane_frames_end));
        let recovered_frames = load(self, recovery, std::mem::offset_of!(LaneRecoverySpace, activations));
        let frames = self.word_add_imm(recovered_frames, 1);
        let frame_bytes = self.word_mul_imm(frames, super::super::FRAME_BYTES as i64);
        let recovered = self.word_add(frame, frame_bytes);
        let room = self.word_cmp(IntCC::UnsignedLessThanOrEqual, recovered, frames_end);
        (DirectStacks { values, activations, top, next, frame }, [fits, room])
    }

    /// Branches to `helper` unless every condition holds: one compare and branch each, not
    /// their conjunction as a boolean word.
    pub(super) fn direct_admit(&mut self, conditions: &[ir::Value], helper: ir::Block) {
        for condition in conditions {
            let next = self.builder.create_block();
            self.word_brif(*condition, next, &[], helper, &[]);
            self.builder.switch_to_block(next);
        }
    }
}

/// A dispatch or cell field's offset as the loads take it.
pub(super) fn dispatch_offset(field: usize) -> i32 {
    i32::try_from(field).expect("dispatch offset fits i32")
}
