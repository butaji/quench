//! What admits a native call: the callee and receiver it names, and the room both stacks
//! keep for its window and recovery.

use super::*;
use super::super::entry_cell::EntryCell;

impl Lowering<'_, '_> {
    /// Whether the callee and receiver the call names are the descriptor's, and the `this`
    /// it binds. A sloppy callee binds an object receiver itself and an `undefined` or `null`
    /// one to the global object (`lane_receiver_this`); any other receiver takes the helper.
    pub(super) fn direct_receiver(&mut self, instruction: WideInstruction, callee: &DirectCallee) -> (ir::Value, ir::Value) {
        let base = self.registers_base;
        let callee_bits = self.word_load(base, register_offset(instruction.register_b()));
        let same = self.word_cmp_imm(IntCC::Equal, callee_bits, callee.callee.to_bits() as i64);
        let this = self.word_load(base, register_offset(instruction.register_c()));
        let Some(global) = callee.sloppy else { return (same, this) };
        let object = crate::staged::value::is_direct_object(&mut super::object_slots::emit(&mut self.builder), this);
        let undefined = self.word_cmp_imm(IntCC::Equal, this, Value::UNDEFINED.to_bits() as i64);
        let null = self.word_cmp_imm(IntCC::Equal, this, Value::NULL.to_bits() as i64);
        let nullish = self.word_or(undefined, null);
        let bound = self.word_or(object, nullish);
        let global = self.word_const(global.to_bits() as i64);
        (self.word_and(same, bound), self.word_select(object, this, global))
    }

    /// The lane stack's top and the callee window's end, the shadow stack's top, and whether
    /// both stacks hold the window and the space the callee's published body needs to
    /// recover (`EntryCell::values`, `activations`), each as its own condition so each is one
    /// compare and branch (`direct_admit`).
    pub(super) fn direct_room(&mut self, callee: &DirectCallee, cell: ir::Value) -> (ir::Value, ir::Value, ir::Value, [ir::Value; 2]) {
        let word = std::mem::size_of::<Value>() as i64;
        let payload = self.payload;
        let load = |this: &mut Self, base: ir::Value, field: usize| this.word_load(base, dispatch_offset(field));
        let top = load(self, payload, std::mem::offset_of!(JsStepDispatch, native_top));
        let end = load(self, payload, std::mem::offset_of!(JsStepDispatch, native_end));
        let next = self.word_add_imm(top, i64::from(callee.registers) * word);
        let values = load(self, cell, std::mem::offset_of!(EntryCell, values));
        let bytes = self.word_mul_imm(values, word);
        let reserved = self.word_add(next, bytes);
        let fits = self.word_cmp(IntCC::UnsignedLessThanOrEqual, reserved, end);
        let shadow = load(self, payload, std::mem::offset_of!(JsStepDispatch, shadow_top));
        let shadow_end = load(self, payload, std::mem::offset_of!(JsStepDispatch, shadow_end));
        let activations = load(self, cell, std::mem::offset_of!(EntryCell, activations));
        let frame_bytes = self.word_mul_imm(activations, std::mem::size_of::<ShadowFrame>() as i64);
        let recovered = self.word_add(shadow, frame_bytes);
        let room = self.word_cmp(IntCC::UnsignedLessThanOrEqual, recovered, shadow_end);
        (top, next, shadow, [fits, room])
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
