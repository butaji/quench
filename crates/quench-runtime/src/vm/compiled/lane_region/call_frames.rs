//! The lane's call hooks (`staged::CallFrame`) and its return (`Frame::ret`): where a call's
//! words live in a lane body, and where each of its statuses continues. The order of the call
//! (identity, `this`, room, window, entry, completion) is `staged::invoke`; this file keeps the
//! words it reads (the register file, the lane stack's tops, the callee's entry cell) and the
//! blocks the lowering made for its exits.

use super::*;
use super::super::object_slots::emit;
use crate::staged::flow::FlowStage;
use crate::staged::machine::{Binary, Lane, MachineScalar, Relation};
use super::frame::{Edges, EmitFrame, FRAME};
use super::relocs::address;
use super::super::entry_cell::{EntryCell, LANE_BODY_NOT_ENTERED, RAISED_RUN};
use super::super::super::dispatch_fast::LaneRecoverySpace;
use crate::staged::call::admit_room;
use crate::staged::{Access, Region};
use crate::staged::{plan_entry, CallEntry, CallFrame, CalleeKind, DirectPlan, Entered, Identity, Invocation, Resolved, Room, Stage};
use crate::typed_ir::BindingTime;

/// Where a run's completion goes: the block a run leaves at with its status and result word.
#[derive(Clone, Copy)]
pub(super) struct Leave {
    pub(super) block: ir::Block,
}

impl Leave {
    /// Leaves with `status`, and with `word` as the result word where a run publishes one.
    pub(super) fn with(self, builder: &mut FunctionBuilder<'_>, status: ir::Value, word: Option<ir::Value>) {
        let word = word.unwrap_or_else(|| emit(builder).raw_const(Lane::I64, 0));
        emit(builder).jump(self.block, &[status, word]);
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
    /// A completed call continues at `completed`, a miss leaves at `missed`, a callee that ended by
    /// an exception (`Vm::lane_raised`) continues at `raised` (`Lowering::raised_call`), and a
    /// leave in a callee, or a termination, reaches `stop` with its status.
    pub(super) completed: ir::Block,
    pub(super) missed: ir::Block,
    pub(super) raised: ir::Block,
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
        let record = emit(builder).raw_const(Lane::I64, i64::from(self.record));
        emit(builder).store_u64(Region::Dispatch.mutable(), payload, field(std::mem::offset_of!(JsStepDispatch, returned_pc)), record);
        let returned = emit(builder).raw_const(Lane::I32, i64::from(LANE_BODY_RETURNED));
        self.leave.with(builder, returned, Some(value));
    }
}

/// The site a step of the call family runs for.
#[derive(Clone, Copy)]
pub(super) enum Site<'a> {
    Call(CallSite<'a>),
    Return(ReturnSite),
}

/// The alias region of the callee's entry cell, which a call's words live in beside the dispatch's
/// stacks and the caller's register file.
const ENTRY_CELL: Access = Region::EntryCell.mutable();

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

    /// A word of the dispatch: the addresses of the lane stack's tops and its ends.
    fn stack_word(&self, stage: &mut Emit<'_, '_>, offset: usize) -> ir::Value {
        stage.load_u64(DISPATCH, self.payload, field(offset))
    }

    /// The lane stack's values' top, read through the word that holds it: the callee's window begins there.
    fn values_top(&self, stage: &mut Emit<'_, '_>) -> ir::Value {
        let values = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_values_top));
        stage.load_u64(DISPATCH, values, 0)
    }

    /// Writes the values' top through its word.
    fn set_values_top(&self, stage: &mut Emit<'_, '_>, top: ir::Value) {
        let values = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_values_top));
        stage.store_u64(DISPATCH, values, 0, top);
    }

    /// The total frame count a native header is indexed by (one-frame-one-call.md F5): the frames
    /// below the plain headers (`*activation_count`) plus the plain depth, the count before the push.
    fn frame_index(&self, stage: &mut Emit<'_, '_>) -> ir::Value {
        let count_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, activation_count));
        let count = stage.load_u64(DISPATCH, count_at, 0);
        let plain = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, plain_depth));
        let depth = stage.load_u64(Region::Payload.mutable(), plain, crate::staged::frame::PLAIN_DEPTH_AT as i32);
        stage.add(count, depth)
    }

    /// Admit the callee's window to the lane stack's values, and its native header to the plain frames
    /// (the index bound: `index + RECOVERY_ACTIVATIONS_AT + 1 <= plain_depth_limit`), each a room row
    /// leaving at `miss`.
    fn admit_stacks(&self, stage: &mut Emit<'_, '_>, callee: &DirectCallee, cell: ir::Value, miss: ir::Block) -> Option<Room<ir::Value>> {
        let word = std::mem::size_of::<Value>() as u64;
        let top = self.values_top(stage);
        let end = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_values_end));
        // The next window's context words follow the callee's registers (`lane_values::CONTEXT`).
        let next = stage.add_imm(top, (u64::from(callee.registers) + crate::vm::register_file::FRAME_CONTEXT as u64) * word);
        let recovery = stage.load_u64(ENTRY_CELL, cell, field(std::mem::offset_of!(EntryCell, recovery)));
        let recovered_values = stage.load_u64(ENTRY_CELL, recovery, field(std::mem::offset_of!(LaneRecoverySpace, values)));
        let bytes = stage.mul_imm(recovered_values, word);
        let reserved = stage.add(next, bytes);
        admit_room(stage, end, reserved, miss)?;
        let index = self.frame_index(stage);
        let activations = stage.load_u64(ENTRY_CELL, recovery, field(std::mem::offset_of!(LaneRecoverySpace, activations)));
        let frames = stage.add_imm(activations, 1_u64);
        let reserved = stage.add(index, frames);
        let limit_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, plain_depth_limit));
        let limit = stage.load_u64(DISPATCH, limit_at, 0);
        admit_room(stage, limit, reserved, miss)?;
        Some(Room { cell, top, next, shadow: index })
    }

    /// The callee's native header (one-frame-one-call.md F5), written below its window with NATIVE_FLAG:
    /// its receiver, the caller's registers, the callee's constant words and the packed word. Its body and
    /// descriptor go in the tables at its index, the plain depth is bumped and the window set as the innermost.
    /// The link (`native_frames::link_header`) fills the call record, caller base, argc and side record.
    fn push_native(&self, stage: &mut Emit<'_, '_>, callee: &DirectCallee, this: ir::Value, caller_registers: ir::Value, room: Room<ir::Value>) {
        use crate::staged::frame::{CONTEXT_CALL, CONTEXT_CALLEE, CONTEXT_CALLER_BASE, CONTEXT_CALLER_REGISTERS, CONTEXT_PACKED, CONTEXT_THIS};
        let window = room.top;
        let at = Region::Frame.mutable();
        let descriptor = address(self.helpers, stage.builder, RelocKind::Descriptor, callee.target.function, callee as *const DirectCallee as u64);
        let body = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, body)));
        let call = stage.constant(callee.call as u64);
        let caller_base = stage.constant(callee.caller_base as u64);
        let packed = stage.constant(u64::from(callee.target.function) | crate::staged::frame::PLAIN_FLAG | crate::staged::frame::NATIVE_FLAG);
        let callee_word = stage.constant(callee.callee.to_bits());
        stage.store_u64(at, window, context_at(CONTEXT_THIS), this);
        stage.store_u64(at, window, context_at(CONTEXT_CALL), call);
        stage.store_u64(at, window, context_at(CONTEXT_CALLER_REGISTERS), caller_registers);
        stage.store_u64(at, window, context_at(CONTEXT_CALLER_BASE), caller_base);
        stage.store_u64(at, window, context_at(CONTEXT_PACKED), packed);
        stage.store_u64(at, window, context_at(CONTEXT_CALLEE), callee_word);
        // The index's entries: the body and the descriptor, one word each.
        let offset = stage.mul_imm(room.shadow, std::mem::size_of::<usize>() as u64);
        let bodies = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, body_table));
        let body_at = stage.add(bodies, offset);
        stage.store_u64(at, body_at, 0, body);
        let descriptors = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, native_table));
        let descriptor_at = stage.add(descriptors, offset);
        stage.store_u64(at, descriptor_at, 0, descriptor);
        // The plain frames: one more header, with the window as the innermost.
        let plain = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, plain_depth));
        let depth = stage.load_u64(Region::Payload.mutable(), plain, crate::staged::frame::PLAIN_DEPTH_AT as i32);
        let depth = stage.add_imm(depth, 1_u64);
        stage.store_u64(Region::Payload.mutable(), plain, crate::staged::frame::PLAIN_DEPTH_AT as i32, depth);
        stage.store_u64(Region::Payload.mutable(), plain, crate::staged::frame::PLAIN_REGISTERS_AT as i32, window);
        let count_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, body_top));
        let index = stage.add_imm(room.shadow, 1_u64);
        stage.store_u64(DISPATCH, count_at, 0, index);
        self.set_values_top(stage, room.next);
    }

    /// Pops the callee's native header, where the call began (`Room`): the plain depth drops, the innermost
    /// window becomes the header's caller registers, the body top returns to the index, and the values' top
    /// returns to the window.
    fn pop_native(&self, stage: &mut Emit<'_, '_>, room: Room<ir::Value>) {
        use crate::staged::frame::{CONTEXT_CALLER_REGISTERS, PLAIN_DEPTH_AT, PLAIN_REGISTERS_AT};
        let caller = stage.load_u64(Region::Frame.mutable(), room.top, context_at(CONTEXT_CALLER_REGISTERS));
        let plain = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, plain_depth));
        let depth = stage.load_u64(Region::Payload.mutable(), plain, PLAIN_DEPTH_AT as i32);
        let depth = stage.add_imm(depth, u64::MAX);
        stage.store_u64(Region::Payload.mutable(), plain, PLAIN_DEPTH_AT as i32, depth);
        stage.store_u64(Region::Payload.mutable(), plain, PLAIN_REGISTERS_AT as i32, caller);
        let count_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, body_top));
        stage.store_u64(DISPATCH, count_at, 0, room.shadow);
        self.set_values_top(stage, room.top);
    }
}

/// The context word at `index` counted down from register 0 (`staged::frame`).
const fn context_at(index: usize) -> i32 { -((index * std::mem::size_of::<u64>()) as i32) }

impl CallFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn call_entry(&self, site: &Invocation) -> CallEntry {
        let Some(CallSite { callee: Some(callee), .. }) = self.call_site() else { return CallEntry::Helper };
        if callee.lexical { return CallEntry::Helper; }
        let kind = if site.constructs() { CalleeKind::Class } else { CalleeKind::Script };
        let plan = DirectPlan { kind, identity: Identity::Guarded(callee.callee.to_bits()), sloppy: callee.sloppy.map(Value::to_bits),
            registers: callee.registers, returned: LANE_BODY_RETURNED, bound: None };
        // The callee is the one the site linked, which the call guards: a Site fact, never inlined by the plan.
        plan_entry(Some(Resolved { plan, binding: BindingTime::Site, body: None }), self.tiers.lane_body.max_inlined_instructions)
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
    /// callee's captures and its window; both tops stay pushed, which a completion pops and a leave keeps
    /// for the lane to link.
    fn call_enter(&mut self, stage: &mut Emit<'_, '_>, this: ir::Value, room: Room<ir::Value>) -> Option<Entered<ir::Value, ir::Block>> {
        let call = self.call_site()?;
        let callee = call.callee?;
        self.push_native(stage, callee, this, call.registers, room);
        let entry = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, entry)));
        let signature = lane_body_signature(types::I64, stage.builder.func.signature.call_conv);
        let signature = crate::vm::jit_backend::signature_ref(stage.builder.func, signature);
        let captures = address(self.helpers, stage.builder, RelocKind::CaptureWords, callee.target.function,
            callee.capture_word());
        let (status, word) = stage.call_entry(signature, entry, &[call.context, this, captures, room.top]);
        Some(Entered { status, word, unfinished: stage.builder.create_block() })
    }

    /// A returned callee's value is the call's result, its frame popped, and the call continues at
    /// `completed`. A callee that never entered (the cell held the stub) takes the VM's call; one
    /// that ended by an exception (`LANE_BODY_THREW`, in `Vm::lane_raised`) has its frame popped and
    /// is counted in its cell, and the call continues at `raised`; any other status is a leave in
    /// the callee, which stops the caller with it (`dispatch_fast::lane_body_left`).
    fn call_settle(&mut self, stage: &mut Emit<'_, '_>, site: &Invocation, entered: Entered<ir::Value, ir::Block>, room: Room<ir::Value>,
        value: ir::Value) -> Option<()> {
        let call = self.call_site()?;
        let helper = self.helper_block()?;
        self.pop_native(stage, room);
        stage.store_u64(FRAME, call.registers, register_offset(site.result), value);
        stage.jump(call.completed, &[]);
        let (left, missed, raised) = (stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block());
        for block in [left, missed, raised] {
            stage.builder.set_cold_block(block);
        }
        // A callee that never entered and one that threw both pop the frame, once, in one
        // block; a leave keeps it pushed for the lane to link.
        let popped = stage.builder.create_block();
        stage.builder.set_cold_block(popped);
        stage.builder.switch_to_block(entered.unfinished);
        let not_entered = stage.raw_compare_imm(Relation::Equal, entered.status, i64::from(LANE_BODY_NOT_ENTERED));
        let did_raise = stage.raw_compare_imm(Relation::Equal, entered.status, i64::from(LANE_BODY_THREW));
        let pops = stage.raw_binary(Binary::Or, not_entered, did_raise);
        stage.branch(pops, (popped, &[]), (left, &[]));
        stage.builder.switch_to_block(popped);
        self.pop_native(stage, room);
        stage.branch(not_entered, (missed, &[]), (raised, &[]));
        stage.builder.switch_to_block(missed);
        stage.jump(helper, &[]);
        stage.builder.switch_to_block(raised);
        let counters = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, counters)));
        let counted = stage.add_imm(counters, RAISED_RUN);
        stage.store_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, counters)), counted);
        stage.jump(call.raised, &[]);
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
            stage.jump(helper, &[]);
        }
        stage.builder.switch_to_block(helper);
        let which = if site.constructs() { LaneHelper::ConstructCall } else { LaneHelper::Call };
        let context = emit_js_payload_word(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let record = stage.raw_const(Lane::I64, i64::from(call.record));
        let address = self.helpers.address(&mut stage.builder, which);
        let status = emit_lane_helper_call(stage.builder, self.payload, address, &[context, record]);
        let (done, other) = (stage.raw_compare_imm(Relation::Equal, status, LANE_CALL_COMPLETED as i64), stage.builder.create_block());
        stage.branch(done, (call.completed, &[]), (other, &[]));
        stage.builder.switch_to_block(other);
        let (missed, unmissed) = (stage.raw_compare_imm(Relation::Equal, status, LANE_CALL_MISSED as i64), stage.builder.create_block());
        stage.branch(missed, (call.missed, &[]), (unmissed, &[]));
        stage.builder.switch_to_block(unmissed);
        let raised = stage.raw_compare_imm(Relation::Equal, status, LANE_CALL_RAISED as i64);
        stage.branch(raised, (call.raised, &[]), (call.stop, &[status]));
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
