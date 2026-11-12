//! The native call itself: the window and native frame it pushes, the words the callee runs
//! with, and the entry its cell holds.

use super::*;
use super::super::entry_cell::EntryCell;
use super::direct_admission::{dispatch_offset, DirectStacks};
use super::super::super::dispatch_fast::{KIND_NATIVE, KIND_OFFSET};

impl Lowering<'_, '_> {
    /// Pushes the callee's window and its native frame (`ActivationKind::Native`: the prefix
    /// words and the kind tag), installs the per-entry words it runs with and calls the entry
    /// its cell holds; its status and result word. Both tops stay pushed, which a completion
    /// pops (`DirectStacks::pop`) and a leave keeps for the lane to link.
    pub(super) fn direct_enter(&mut self, instruction: WideInstruction, callee: &DirectCallee, cell: ir::Value, (this, stacks): (ir::Value, DirectStacks)) -> (ir::Value, ir::Value) {
        self.direct_window(instruction, callee, stacks.top);
        let descriptor = self.reloc(RelocKind::Descriptor, callee.target.function, callee as *const DirectCallee as u64);
        let entry = self.word_load(cell, dispatch_offset(std::mem::offset_of!(EntryCell, entry)));
        let body = self.word_load(cell, dispatch_offset(std::mem::offset_of!(EntryCell, body)));
        for (field, value) in [(std::mem::offset_of!(ShadowFrame, callee), descriptor), (std::mem::offset_of!(ShadowFrame, this), this), (std::mem::offset_of!(ShadowFrame, body), body), (std::mem::offset_of!(ShadowFrame, window), stacks.top)] {
            self.word_store(value, stacks.frame, dispatch_offset(field));
        }
        let tag = self.word_const(i64::from(KIND_NATIVE));
        self.word_store(tag, stacks.frame, dispatch_offset(KIND_OFFSET));
        let pushed = self.word_add_imm(stacks.frame, super::super::FRAME_BYTES as i64);
        self.word_store(pushed, stacks.activations, 0);
        self.word_store(stacks.next, stacks.values, 0);
        let signature = lane_body_signature(types::I64, self.builder.func.signature.call_conv);
        let signature = self.builder.import_signature(signature);
        let captures = self.reloc(RelocKind::CaptureWords, callee.target.function, super::super::entry_cell::capture_word(callee.capture_values, callee.capture_len) as u64);
        let context = self.context;
        self.word_call_all(signature, entry, &[context, this, captures, stacks.top])
    }
}
