//! A lane body's native call of a callee through its entry cell: the receiver and room
//! it admits, the window and frame it pushes, and where each status goes.

use super::*;
use super::super::entry_cell::{EntryCell, LANE_BODY_NOT_ENTERED};
use super::direct_admission::dispatch_offset;

impl Lowering<'_, '_> {
    /// The call of `callee` by its compiled entry: the callee's registers in the lane stack's
    /// free values, a native frame the lane makes an activation of only should the callee
    /// leave (`compiled::ShadowFrame`), and the per-entry words the callee runs with. A
    /// completion stores its value to the call's register and continues at `completed`; a
    /// leave stops the caller too, its frames kept; any other callee, receiver, or a full
    /// stack takes `helper`. The caller wrote back its registers.
    pub(super) fn direct_call(&mut self, instruction: WideInstruction, callee: &DirectCallee, helper: ir::Block, completed: ir::Block) {
        let (mut admitted, this) = self.direct_receiver(instruction, callee);
        let cell = self.reloc(RelocKind::EntryCell, callee.target.function, callee.cell as u64);
        let (top, next, shadow, [fits, room]) = self.direct_room(callee, cell);
        admitted.extend([fits, room]);
        self.direct_admit(&admitted, helper);
        let status = self.direct_enter(instruction, callee, cell, (this, top, next, shadow));
        self.direct_finish(instruction, status, (shadow, cell), helper, completed, None);
    }

    /// The construction through forwarding constructor `callee.callee` whose forwarded
    /// initializer is `callee.target`: the constructor guarded, the receiver its plan
    /// allocates (`LaneHelper::ConstructReceiver`) passed as the initializer's `this`, which
    /// runs through its cell; the construction yields the receiver whatever it returns, as
    /// the constructor's forwarding body does. Any other constructor, a plan that does not
    /// hold, or no room takes `helper`.
    pub(super) fn direct_construct(&mut self, instruction: WideInstruction, callee: &DirectCallee, helper: ir::Block, completed: ir::Block) {
        let (base, payload) = (self.registers_base, self.payload);
        let constructor = self.word_load(base, register_offset(instruction.register_b()));
        let same = self.word_cmp_imm(IntCC::Equal, constructor, callee.callee.to_bits() as i64);
        self.direct_admit(&[same], helper);
        let receiver = self.helper_call(LaneHelper::ConstructReceiver, &[payload, constructor]);
        let built = self.word_cmp_imm(IntCC::NotEqual, receiver, Value::DELETED.to_bits() as i64);
        let cell = self.reloc(RelocKind::EntryCell, callee.target.function, callee.cell as u64);
        let (top, next, shadow, [fits, room]) = self.direct_room(callee, cell);
        self.direct_admit(&[built, fits, room], helper);
        let status = self.direct_enter(instruction, callee, cell, (receiver, top, next, shadow));
        self.direct_finish(instruction, status, (shadow, cell), helper, completed, Some(receiver));
    }

    /// Where each status of a native call goes: a completion stores its value, or the
    /// construction's `receiver`, to the call's register and continues at `completed`.
    fn direct_finish(&mut self, instruction: WideInstruction, (status, word): (ir::Value, ir::Value), (shadow, cell): (ir::Value, ir::Value), helper: ir::Block, completed: ir::Block, receiver: Option<ir::Value>) {
        let (base, payload) = (self.registers_base, self.payload);
        let returned = self.word_cmp_imm(IntCC::Equal, status, i64::from(LANE_BODY_RETURNED));
        let (back, check, left, missed) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(left);
        self.builder.set_cold_block(missed);
        self.word_brif(returned, back, &[], check, &[]);
        // The cell held the stub: nothing ran, so the call takes the helper path.
        self.builder.switch_to_block(check);
        let not_entered = self.word_cmp_imm(IntCC::Equal, status, i64::from(LANE_BODY_NOT_ENTERED));
        self.word_brif(not_entered, missed, &[], left, &[]);
        self.builder.switch_to_block(missed);
        self.word_store(shadow, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        self.word_jump(helper, &[]);
        // A leave inside the callee stops the caller with it: the lane makes the frames'
        // activations and continues in the innermost (`dispatch_fast::lane_body_left`).
        self.builder.switch_to_block(left);
        self.leave_with(status, Some(word));
        self.builder.switch_to_block(back);
        let offset = dispatch_offset(std::mem::offset_of!(EntryCell, completions));
        let done = self.word_load(cell, offset);
        let done = self.word_add_imm(done, 1);
        self.word_store(done, cell, offset);
        self.word_store(shadow, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        let value = match receiver { Some(receiver) => receiver, None => word };
        self.word_store(value, base, register_offset(instruction.result_register()));
        self.word_jump(completed, &[]);
    }

    /// Copies the call's arguments into the callee's window at `top`; parameter `k` lives in
    /// register `k` of a plain activation, and the rest start undefined.
    pub(super) fn direct_window(&mut self, instruction: WideInstruction, callee: &DirectCallee, top: ir::Value) {
        let base = self.registers_base;
        let undefined = self.word_const(Value::UNDEFINED.to_bits() as i64);
        let (window, argc) = match instruction.construct_arguments() {
            crate::bytecode::ConstructArguments::Registers(window) if instruction.op().class().eq(Class::Construct) => (window.base, window.count),
            _ => (crate::bytecode::ImmediateLayout::call_window_base(instruction.imm()), crate::bytecode::ImmediateLayout::argument_count(instruction.imm())),
        };
        for register in 0..callee.registers {
            let value = if register < argc {
                self.word_load(base, register_offset(window + register))
            } else { undefined };
            self.word_store(value, top, register_offset(register));
        }
    }
}
