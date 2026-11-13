//! A lane body's native call of a callee through its entry cell: the callee and receiver it
//! admits, the room both stacks keep, the window and frame it pushes, and where each status
//! goes. The admission, `this` binding, window and completion are the staged call protocol
//! (`staged::protocol`), stepped as rows (`call_row`); the lowering owns where the words live.

use super::*;
use super::super::entry_cell::LANE_BODY_NOT_ENTERED;
use super::direct_admission::dispatch_offset;
use crate::staged::{CallOperation, Invocation};

impl Lowering<'_, '_> {
    /// The call of `callee` by its compiled entry: the callee's registers in the lane stack's
    /// free values, a native frame the lane makes an activation of only should the callee
    /// leave (`compiled::ShadowFrame`), and the per-entry words the callee runs with. A
    /// completion stores its value to the call's register and continues at `completed`; a
    /// leave stops the caller too, its frames kept; any other callee, receiver, or a full
    /// stack takes `helper`. The caller wrote back its registers.
    pub(super) fn direct_call(&mut self, state: &mut State, site: Invocation, callee: &DirectCallee, helper: ir::Block, completed: ir::Block) {
        let base = self.registers_base;
        let held = self.word_load(base, register_offset(site.callee));
        let expected = self.stable.roots.embed_heap_identity(callee.callee);
        self.call_row(state, helper, CallOperation::Callee { held, expected });
        let receiver = self.word_load(base, register_offset(site.receiver.expect("a call has a receiver register")));
        let sloppy = callee.sloppy.map(|global| self.stable.roots.embed_heap_identity(global));
        let this = self.call_row(state, helper, CallOperation::BindThis { receiver, sloppy });
        let cell = self.reloc(RelocKind::EntryCell, callee.target.function, callee.cell as u64);
        let (top, next, shadow) = self.direct_room(state, callee, cell, helper);
        let status = self.direct_enter(state, site, callee, cell, (this, top, next, shadow), helper);
        self.direct_finish(state, site, status, shadow, (helper, completed), None);
    }

    /// The construction through forwarding constructor `callee.callee` whose forwarded
    /// initializer is `callee.target`: the constructor guarded, the receiver its plan
    /// allocates (`LaneHelper::ConstructReceiver`) passed as the initializer's `this`, which
    /// runs through its cell; the construction yields the receiver whatever it returns, as
    /// the constructor's forwarding body does. Any other constructor, a plan that does not
    /// hold, or no room takes `helper`.
    pub(super) fn direct_construct(&mut self, state: &mut State, site: Invocation, callee: &DirectCallee, helper: ir::Block, completed: ir::Block) {
        let (base, payload) = (self.registers_base, self.payload);
        let constructor = self.word_load(base, register_offset(site.callee));
        let expected = self.stable.roots.embed_heap_identity(callee.callee);
        self.call_row(state, helper, CallOperation::Callee { held: constructor, expected });
        let allocated = self.helper_call(LaneHelper::ConstructReceiver, &[payload, constructor]);
        let receiver = self.call_row(state, helper, CallOperation::Receiver { receiver: allocated });
        let cell = self.reloc(RelocKind::EntryCell, callee.target.function, callee.cell as u64);
        let (top, next, shadow) = self.direct_room(state, callee, cell, helper);
        let status = self.direct_enter(state, site, callee, cell, (receiver, top, next, shadow), helper);
        self.direct_finish(state, site, status, shadow, (helper, completed), Some(receiver));
    }

    /// Where each status of a native call goes: a completion stores its value, or the
    /// construction's `receiver`, to the call's register and continues at `completed`. The
    /// completion row leaves at `check` for any other status.
    fn direct_finish(&mut self, state: &mut State, site: Invocation, (status, word): (ir::Value, ir::Value), shadow: ir::Value, (helper, completed): (ir::Block, ir::Block), receiver: Option<ir::Value>) {
        let (base, payload) = (self.registers_base, self.payload);
        let (check, left, missed) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        self.builder.set_cold_block(left);
        self.builder.set_cold_block(missed);
        let value = super::super::unforced(|| self.call_row(state, check, CallOperation::Complete { status, value: word, receiver, returned: LANE_BODY_RETURNED }));
        self.word_store(shadow, payload, dispatch_offset(std::mem::offset_of!(JsStepDispatch, shadow_top)));
        self.word_store(value, base, register_offset(site.result));
        self.word_jump(completed, &[]);
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
    }

    /// Copies the call's arguments into the callee's window at `top`; parameter `k` lives in
    /// register `k` of a plain activation, and the rest start undefined (`protocol::fill_window`).
    pub(super) fn direct_window(&mut self, state: &mut State, site: Invocation, callee: &DirectCallee, top: ir::Value, miss: ir::Block) {
        let fill = crate::staged::WindowFill { first: site.window, argc: site.argc, registers: callee.registers };
        let source = self.registers_base;
        self.call_row(state, miss, CallOperation::Window { top, source, fill });
    }
}
