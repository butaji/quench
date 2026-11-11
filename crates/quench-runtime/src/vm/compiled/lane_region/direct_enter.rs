//! The native call itself: the window and shadow frame it pushes, the words the callee runs
//! with, and the entry its cell holds.

use super::*;
use super::super::entry_cell::EntryCell;
use super::direct_admission::dispatch_offset;

impl Lowering<'_, '_> {
    /// Pushes the callee's window and shadow frame, installs the per-entry words it runs with
    /// and calls the entry its cell holds; the status, with the caller's words back.
    pub(super) fn direct_enter(&mut self, instruction: WideInstruction, callee: &DirectCallee, cell: ir::Value, (this, top, next, shadow): (ir::Value, ir::Value, ir::Value, ir::Value)) -> ir::Value {
        let (payload, result) = (self.payload, self.result);
        self.direct_window(instruction, callee, top);
        let descriptor = self.word_const(callee as *const DirectCallee as i64);
        let entry = self.word_load(cell, dispatch_offset(std::mem::offset_of!(EntryCell, entry)));
        let pin = self.word_load(cell, dispatch_offset(std::mem::offset_of!(EntryCell, pin)));
        for (field, value) in [(std::mem::offset_of!(ShadowFrame, callee), descriptor), (std::mem::offset_of!(ShadowFrame, this), this), (std::mem::offset_of!(ShadowFrame, pin), pin)] {
            self.word_store(value, shadow, dispatch_offset(field));
        }
        let pushed = self.word_add_imm(shadow, std::mem::size_of::<ShadowFrame>() as i64);
        let words = [
            std::mem::offset_of!(JsStepDispatch, receiver),
            std::mem::offset_of!(JsStepDispatch, capture_values),
            std::mem::offset_of!(JsStepDispatch, capture_len),
        ];
        let capture_values = self.word_const(callee.capture_values as i64);
        let capture_len = self.word_const(callee.capture_len as i64);
        let saved: Vec<ir::Value> = words.iter()
            .map(|field| self.word_load(payload, dispatch_offset(*field)))
            .collect();
        for (field, value) in words.iter().zip([this, capture_values, capture_len]) {
            self.word_store(value, payload, dispatch_offset(*field));
        }
        self.word_store(pushed, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        self.word_store(next, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        let signature = native_entry_signature(types::I64, self.builder.func.signature.call_conv);
        let signature = self.builder.import_signature(signature);
        let zero = self.word_const(0);
        let context = self.context;
        let status = self.word_call(signature, entry, &[context, zero, zero, top, result]);
        for (field, value) in words.iter().zip(saved) {
            self.word_store(value, payload, dispatch_offset(*field));
        }
        self.word_store(top, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        status
    }
}
