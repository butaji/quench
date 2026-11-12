//! The lane's call hooks (`staged::CallFrame`) and its return (`Frame::ret`): where a call's
//! words live in a lane body, and where each of its statuses continues. The order of the call
//! (identity, `this`, room, window, entry, completion) is `staged::invoke`; this file keeps the
//! words it reads (the register file, the lane and shadow stacks, the callee's entry cell) and the
//! blocks the lowering made for its exits.

use super::*;
use super::frame::{Edges, EmitFrame};
use super::relocs::address;
use super::super::entry_cell::{capture_word, EntryCell, LANE_BODY_NOT_ENTERED};
use super::super::super::dispatch_fast::LaneRecoverySpace;
use crate::staged::call::admit_room;
use crate::staged::{CallEntry, CallFrame, CalleeKind, DirectPlan, Entered, Identity, Invocation, Room, Stage};

/// Where a run's completion goes: the block a run leaves at, and where it publishes its result
/// word.
#[derive(Clone, Copy)]
pub(super) struct Leave {
    pub(super) completion: Completion,
    pub(super) block: ir::Block,
    pub(super) result: ir::Value,
}

impl Leave {
    /// Leaves with `status`, and with `word` as the result word where a run publishes one: a
    /// body returns it beside the status, a region stores it through its result pointer.
    pub(super) fn with(self, builder: &mut FunctionBuilder<'_>, status: ir::Value, word: Option<ir::Value>) {
        if self.completion == Completion::Body {
            let word = word.unwrap_or_else(|| builder.ins().iconst(types::I64, 0));
            builder.ins().jump(self.block, &[status.into(), word.into()]);
        } else {
            if let Some(word) = word {
                builder.ins().store(MemFlagsData::trusted(), word, self.result, 0);
            }
            builder.ins().jump(self.block, &[status.into()]);
        }
    }
}

/// What the lowering knows of the call at one record: the callee its site is bound to, the
/// words the call reads, and the blocks its statuses continue at.
#[derive(Clone, Copy)]
pub(super) struct CallSite<'a> {
    /// The callee a body calls by its entry, which the body owns; `None` where the VM calls.
    pub(super) callee: Option<&'a DirectCallee>,
    /// The record the VM's call reads, and the lane context a body enters with.
    pub(super) record: u32,
    pub(super) context: ir::Value,
    /// The base of the caller's registers, written back for the call.
    pub(super) registers: ir::Value,
    /// A completed call continues at `completed`, a miss leaves at `missed`, and a leave in a
    /// callee, or a termination, reaches `stop` with its status.
    pub(super) completed: ir::Block,
    pub(super) missed: ir::Block,
    pub(super) stop: ir::Block,
    pub(super) leave: Leave,
}

/// The record that completes the activation a return ends (`LaneExit::Returned`), and where it
/// leaves.
#[derive(Clone, Copy)]
pub(super) struct ReturnSite {
    pub(super) record: u32,
    pub(super) leave: Leave,
}

impl ReturnSite {
    /// Stops with the activation's completion: `value` in the result word, the record in the
    /// dispatch (`LANE_BODY_RETURNED`).
    pub(super) fn complete(self, builder: &mut FunctionBuilder<'_>, payload: ir::Value, value: ir::Value) {
        let record = builder.ins().iconst(types::I64, i64::from(self.record));
        builder.ins().store(MemFlagsData::trusted(), record, payload, field(std::mem::offset_of!(JsStepDispatch, returned_pc)));
        let returned = builder.ins().iconst(types::I32, i64::from(LANE_BODY_RETURNED));
        self.leave.with(builder, returned, Some(value));
    }
}

/// The site a step of the call family runs for.
#[derive(Clone, Copy)]
pub(super) enum Site<'a> {
    Call(CallSite<'a>),
    Return(ReturnSite),
}

/// A dispatch or cell field's offset as the loads take it.
fn field(offset: usize) -> i32 {
    i32::try_from(offset).expect("dispatch offset fits i32")
}

impl EmitFrame<'_> {
    fn call_site(&self) -> Option<CallSite<'_>> {
        match self.site {
            Some(Site::Call(call)) => Some(call),
            _ => None,
        }
    }

    /// The block the call's guards, and a callee that never entered, leave at: the VM's call.
    fn helper_block(&self) -> Option<ir::Block> {
        match self.edges {
            Edges::ValueGuarded { miss } => Some(miss),
            _ => None,
        }
    }

    /// The lane's stack top and end, and the shadow stack's top and end, in the dispatch.
    fn stack_word(&self, stage: &mut Emit<'_, '_>, offset: usize) -> ir::Value {
        stage.load_u64(self.payload, field(offset))
    }

    /// Admit the callee's window to the lane stack, and its recovery space to the lane and
    /// shadow stacks, each a room row leaving at `miss`.
    fn admit_stacks(&self, stage: &mut Emit<'_, '_>, callee: &DirectCallee, cell: ir::Value, miss: ir::Block) -> Option<Room<ir::Value>> {
        let word = std::mem::size_of::<Value>() as u64;
        let top = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, native_top));
        let end = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, native_end));
        let next = stage.add_imm(top, u64::from(callee.registers) * word);
        let recovery = stage.load_u64(cell, field(std::mem::offset_of!(EntryCell, recovery)));
        let values = stage.load_u64(recovery, field(std::mem::offset_of!(LaneRecoverySpace, values)));
        let bytes = stage.mul_imm(values, word);
        let reserved = stage.add(next, bytes);
        admit_room(stage, end, reserved, miss)?;
        let shadow = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, shadow_top));
        let shadow_end = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, shadow_end));
        let activations = stage.load_u64(recovery, field(std::mem::offset_of!(LaneRecoverySpace, activations)));
        let frames = stage.add_imm(activations, 1_u64);
        let frame_bytes = stage.mul_imm(frames, super::super::FRAME_BYTES as u64);
        let recovered = stage.add(shadow, frame_bytes);
        admit_room(stage, shadow_end, recovered, miss)?;
        Some(Room { cell, top, next, shadow })
    }

    /// The callee's shadow frame pushed (`ShadowFrame`), which the lane makes an activation of
    /// only should the callee leave, and the stacks' tops moved past its window.
    fn push_shadow(&self, stage: &mut Emit<'_, '_>, callee: &DirectCallee, this: ir::Value, room: Room<ir::Value>) {
        let descriptor = address(self.helpers, stage.builder, RelocKind::Descriptor, callee.target.function, callee as *const DirectCallee as u64);
        let body = stage.load_u64(room.cell, field(std::mem::offset_of!(EntryCell, body)));
        for (offset, value) in [(std::mem::offset_of!(ShadowFrame, callee), descriptor), (std::mem::offset_of!(ShadowFrame, this), this),
            (std::mem::offset_of!(ShadowFrame, body), body), (std::mem::offset_of!(ShadowFrame, window), room.top)] {
            stage.store_u64(room.shadow, field(offset), value);
        }
        let pushed = stage.add_imm(room.shadow, super::super::FRAME_BYTES as u64);
        stage.store_u64(self.payload, field(std::mem::offset_of!(JsStepDispatch, shadow_top)), pushed);
        stage.store_u64(self.payload, field(std::mem::offset_of!(JsStepDispatch, native_top)), room.next);
    }
}

impl CallFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn call_entry(&self, site: &Invocation) -> CallEntry {
        let Some(CallSite { callee: Some(callee), .. }) = self.call_site() else { return CallEntry::Helper };
        let kind = if site.constructs() { CalleeKind::Class } else { CalleeKind::Script };
        CallEntry::Direct(DirectPlan { kind, identity: Identity::Guarded(callee.callee.to_bits()), sloppy: callee.sloppy.map(Value::to_bits),
            registers: callee.registers, returned: LANE_BODY_RETURNED })
    }

    fn call_registers(&mut self, _stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        Some(self.call_site()?.registers)
    }

    /// The receiver the constructor's plan allocates (`LaneHelper::ConstructReceiver`); a due
    /// collection answers the deleted marker.
    fn call_receiver(&mut self, stage: &mut Emit<'_, '_>, constructor: ir::Value) -> Option<ir::Value> {
        let helper = self.helpers.address(&mut stage.builder, LaneHelper::ConstructReceiver);
        Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor]))
    }

    fn call_room(&mut self, stage: &mut Emit<'_, '_>, miss: ir::Block) -> Option<Room<ir::Value>> {
        let callee = self.call_site()?.callee?;
        let cell = address(self.helpers, stage.builder, RelocKind::EntryCell, callee.target.function, callee.cell as u64);
        self.admit_stacks(stage, callee, cell, miss)
    }

    /// The call of the entry the callee's cell holds, with the lane context, the receiver, the
    /// callee's captures and its window; the lane's top returns to the window's start after it.
    fn call_enter(&mut self, stage: &mut Emit<'_, '_>, this: ir::Value, room: Room<ir::Value>) -> Option<Entered<ir::Value, ir::Block>> {
        let call = self.call_site()?;
        let callee = call.callee?;
        self.push_shadow(stage, callee, this, room);
        let entry = stage.load_u64(room.cell, field(std::mem::offset_of!(EntryCell, entry)));
        let signature = lane_body_signature(types::I64, stage.builder.func.signature.call_conv);
        let signature = stage.builder.import_signature(signature);
        let captures = address(self.helpers, stage.builder, RelocKind::CaptureWords, callee.target.function,
            capture_word(callee.capture_values, callee.capture_len) as u64);
        let entered = stage.builder.ins().call_indirect(signature, entry, &[call.context, this, captures, room.top]);
        let (status, word) = (stage.builder.inst_results(entered)[0], stage.builder.inst_results(entered)[1]);
        stage.store_u64(self.payload, field(std::mem::offset_of!(JsStepDispatch, native_top)), room.top);
        Some(Entered { status, word, unfinished: stage.builder.create_block() })
    }

    /// A returned callee's value is the call's result, its shadow frame popped, and the call
    /// continues at `completed`. A callee that never entered (the cell held the stub) takes the
    /// VM's call; any other status is a leave in the callee, which stops the caller with it
    /// (`dispatch_fast::lane_body_left`).
    fn call_settle(&mut self, stage: &mut Emit<'_, '_>, site: &Invocation, entered: Entered<ir::Value, ir::Block>, room: Room<ir::Value>,
        value: ir::Value) -> Option<()> {
        let call = self.call_site()?;
        let helper = self.helper_block()?;
        let shadow_top = field(std::mem::offset_of!(JsStepDispatch, shadow_top));
        stage.store_u64(self.payload, shadow_top, room.shadow);
        stage.store_u64(call.registers, register_offset(site.result), value);
        stage.builder.ins().jump(call.completed, &[]);
        let (left, missed) = (stage.builder.create_block(), stage.builder.create_block());
        stage.builder.set_cold_block(left);
        stage.builder.set_cold_block(missed);
        stage.builder.switch_to_block(entered.unfinished);
        let not_entered = stage.builder.ins().icmp_imm_u(IntCC::Equal, entered.status, i64::from(LANE_BODY_NOT_ENTERED));
        stage.builder.ins().brif(not_entered, missed, &[], left, &[]);
        stage.builder.switch_to_block(missed);
        stage.store_u64(self.payload, shadow_top, room.shadow);
        stage.builder.ins().jump(helper, &[]);
        stage.builder.switch_to_block(left);
        call.leave.with(stage.builder, entered.status, Some(entered.word));
        Some(())
    }

    /// The call by the lane's helper of the record: its completion continues at `completed`, a
    /// miss leaves at `missed`, and a leave in a callee, or a termination, reaches `stop`. A call
    /// that emitted no direct entry jumps here; one that did has ended its blocks. The result
    /// is in the call's register, which the lowering reloads.
    fn call_helper(&mut self, stage: &mut Emit<'_, '_>, site: &Invocation, direct: Option<ir::Value>) -> Option<ir::Value> {
        let call = self.call_site()?;
        let helper = self.helper_block()?;
        if direct.is_none() {
            stage.builder.ins().jump(helper, &[]);
        }
        stage.builder.switch_to_block(helper);
        let which = if site.constructs() { LaneHelper::ConstructCall } else { LaneHelper::Call };
        let context = emit_js_payload_word(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let record = stage.builder.ins().iconst(types::I64, i64::from(call.record));
        let address = self.helpers.address(&mut stage.builder, which);
        let status = emit_lane_helper_call(stage.builder, self.payload, address, &[context, record]);
        let (done, other) = (stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_COMPLETED as i64), stage.builder.create_block());
        stage.builder.ins().brif(done, call.completed, &[], other, &[]);
        stage.builder.switch_to_block(other);
        let missed = stage.builder.ins().icmp_imm_u(IntCC::Equal, status, LANE_CALL_MISSED as i64);
        stage.builder.ins().brif(missed, call.missed, &[], call.stop, &[ir::BlockArg::Value(status)]);
        stage.builder.switch_to_block(call.completed);
        Some(stage.constant(0))
    }
}

/// Completes the activation with `value` (`Frame::ret`): only a return the lowering made a site
/// for can.
pub(super) fn complete_activation(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, value: ir::Value) -> Option<()> {
    let Some(Site::Return(site)) = frame.site else { return None };
    site.complete(stage.builder, frame.payload, value);
    Some(())
}
