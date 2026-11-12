//! The native call itself: the window and shadow frame it pushes, the words the callee runs
//! with, and the entry its cell holds.

use super::*;
use super::super::entry_cell::EntryCell;
use super::direct_admission::dispatch_offset;

impl Lowering<'_, '_> {
    /// Pushes the callee's window and shadow frame, installs the per-entry words it runs with
    /// and calls the entry its cell holds; its status and result word, with the caller's words back.
    pub(super) fn direct_enter(&mut self, instruction: WideInstruction, callee: &DirectCallee, cell: ir::Value, (this, top, next, shadow): (ir::Value, ir::Value, ir::Value, ir::Value)) -> (ir::Value, ir::Value) {
        let payload = self.payload;
        self.direct_window(instruction, callee, top);
        let descriptor = self.reloc(RelocKind::Descriptor, callee.target.function, callee as *const DirectCallee as u64);
        let entry = self.word_load(cell, dispatch_offset(std::mem::offset_of!(EntryCell, entry)));
        let body = self.word_load(cell, dispatch_offset(std::mem::offset_of!(EntryCell, body)));
        for (field, value) in [(std::mem::offset_of!(ShadowFrame, callee), descriptor), (std::mem::offset_of!(ShadowFrame, this), this), (std::mem::offset_of!(ShadowFrame, body), body), (std::mem::offset_of!(ShadowFrame, window), top)] {
            self.word_store(value, shadow, dispatch_offset(field));
        }
        let pushed = self.word_add_imm(shadow, std::mem::size_of::<ShadowFrame>() as i64);
        self.word_store(pushed, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        self.word_store(next, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        let signature = lane_body_signature(types::I64, self.builder.func.signature.call_conv);
        let signature = self.builder.import_signature(signature);
        let captures = self.reloc(RelocKind::CaptureWords, callee.target.function, super::super::entry_cell::capture_word(callee.capture_values, callee.capture_len) as u64);
        let context = self.context;
        let call = self.word_call_all(signature, entry, &[context, this, captures, top]);
        self.word_store(top, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, native_top)));
        call
    }
}
