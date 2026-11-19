//! The lane's call hooks (`staged::CallFrame`) and its return (`Frame::ret`): where a call's
//! words live in a lane body, and where each of its statuses continues. The order of the call
//! (identity, `this`, room, window, entry, completion) is `staged::invoke`; this file keeps the
//! words it reads (the register file, the lane stack's tops, the callee's entry cell) and the
//! blocks the lowering made for its exits.

use super::*;
use super::super::object_slots::emit;
use crate::staged::flow::FlowStage;
use crate::staged::machine::{Lane, MachineScalar, Relation};
use super::frame::{Edges, EmitFrame, FRAME};
use super::relocs::address;
use super::super::entry_cell::{EntryCell, RAISED_RUN};
use super::super::exit_protocol::{status_is, StatusClass};
use super::super::super::dispatch_fast::{LaneRecoverySpace, KIND_NATIVE, KIND_OFFSET};
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
    /// A construction whose target's body only initializes `this` (`Function::only_initializes_this_fields`
    /// of an `inline_constructible` function, the facts `dispatch_fast` makes `TARGET_INITIALIZES`), and
    /// the window of arguments it passes in the caller's registers. Its receiver is allocated finished by
    /// `LaneHelper::Construct` (`lane_body_construction`), which is the construction's result; its body is
    /// not entered (`call_enter`), as the VM's own construction does not run it (`initializes()`).
    pub(super) initializing: Option<(Register, u16)>,
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

    /// The lane stack's two tops, each read through the word that holds it (the values' and the
    /// activations'): the callee's window and its frame begin there.
    fn stack_tops(&self, stage: &mut Emit<'_, '_>) -> (ir::Value, ir::Value) {
        let values = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_values_top));
        let activations = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_activations_top));
        (stage.load_u64(DISPATCH, values, 0), stage.load_u64(DISPATCH, activations, 0))
    }

    /// Writes `top` and `frame` back through the tops' words: the call's window and its frame pop.
    fn set_stack_tops(&self, stage: &mut Emit<'_, '_>, (top, frame): (ir::Value, ir::Value)) {
        let values = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_values_top));
        let activations = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_activations_top));
        stage.store_u64(DISPATCH, values, 0, top);
        stage.store_u64(DISPATCH, activations, 0, frame);
    }

    /// Admit the callee's window to the lane stack's values, and its frame and recovery space to
    /// its activations, each a room row leaving at `miss`.
    fn admit_stacks(&self, stage: &mut Emit<'_, '_>, callee: &DirectCallee, cell: ir::Value, miss: ir::Block) -> Option<Room<ir::Value>> {
        let word = std::mem::size_of::<Value>() as u64;
        let (top, frame) = self.stack_tops(stage);
        let end = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_values_end));
        // The next window's context words follow the callee's registers (`lane_values::CONTEXT`).
        let next = stage.add_imm(top, (u64::from(callee.registers) + crate::vm::register_file::FRAME_CONTEXT as u64) * word);
        let recovery = stage.load_u64(ENTRY_CELL, cell, field(std::mem::offset_of!(EntryCell, recovery)));
        let recovered_values = stage.load_u64(ENTRY_CELL, recovery, field(std::mem::offset_of!(LaneRecoverySpace, values)));
        let bytes = stage.mul_imm(recovered_values, word);
        let reserved = stage.add(next, bytes);
        admit_room(stage, end, reserved, miss)?;
        let frames_end = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, lane_frames_end));
        let activations = stage.load_u64(ENTRY_CELL, recovery, field(std::mem::offset_of!(LaneRecoverySpace, activations)));
        let frames = stage.add_imm(activations, 1_u64);
        let frame_bytes = stage.mul_imm(frames, super::super::FRAME_BYTES as u64);
        let recovered = stage.add(frame, frame_bytes);
        admit_room(stage, frames_end, recovered, miss)?;
        Some(Room { cell, top, next, shadow: frame })
    }

    /// The callee's native frame pushed (`ShadowFrame` and `ActivationKind::Native`), which the lane
    /// links only should a helper observe it or the callee leave, and the stacks' tops moved past
    /// its window and its frame.
    fn push_native(&self, stage: &mut Emit<'_, '_>, callee: &DirectCallee, this: ir::Value, room: Room<ir::Value>) {
        let descriptor = address(self.helpers, stage.builder, RelocKind::Descriptor, callee.target.function, callee as *const DirectCallee as u64);
        let body = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, body)));
        for (offset, value) in [(std::mem::offset_of!(ShadowFrame, callee), descriptor), (std::mem::offset_of!(ShadowFrame, this), this),
            (std::mem::offset_of!(ShadowFrame, body), body), (std::mem::offset_of!(ShadowFrame, window), room.top)] {
            stage.store_u64(DISPATCH, room.shadow, field(offset), value);
        }
        let tag = stage.constant(u64::from(KIND_NATIVE));
        stage.store_u64(DISPATCH, room.shadow, field(KIND_OFFSET), tag);
        let pushed = stage.add_imm(room.shadow, super::super::FRAME_BYTES as u64);
        self.set_stack_tops(stage, (room.next, pushed));
    }

    /// Pops the callee's window and frame, where the call began (`Room`).
    fn pop_native(&self, stage: &mut Emit<'_, '_>, room: Room<ir::Value>) {
        self.set_stack_tops(stage, (room.top, room.shadow));
    }
}

impl CallFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn call_entry(&self, site: &Invocation) -> CallEntry {
        let Some(CallSite { callee: Some(callee), .. }) = self.call_site() else { return CallEntry::Helper };
        let kind = if site.constructs() { CalleeKind::Class } else { CalleeKind::Script };
        // A callee register the state already holds as this very function (a Stable binding's
        // read, or an identity an earlier guard proved) needs no check: the identity is folded.
        let word = callee.callee.to_bits();
        let proven = matches!(site.callee, crate::staged::forms::Callee::Register(register)
            if self.state.values[usize::from(register)].value.ty.function_identity().map(|identity| identity.to_bits()) == Some(word));
        let identity = if proven { Identity::Folded(word) } else { Identity::Guarded(word) };
        let plan = DirectPlan { kind, identity, sloppy: callee.sloppy.map(Value::to_bits),
            registers: callee.registers, returned: LANE_BODY_RETURNED, bound: None };
        // The callee is the one the site linked, which the call guards: a Site fact, never inlined by the plan.
        plan_entry(Some(Resolved { plan, binding: BindingTime::Site, body: None }), self.tiers.lane_body.max_inlined_instructions)
    }

    fn call_registers(&mut self, _stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        Some(self.call_site()?.registers)
    }

    /// The receiver the constructor's plan allocates (`LaneHelper::ConstructReceiver`); a due
    /// collection answers the deleted marker. An initializing construction (`CallSite::initializing`)
    /// is allocated finished from its arguments in the caller's window (`LaneHelper::Construct`), as
    /// `lane_body_construction` makes it: the plan's fields, or the initializer's cached adds. Its
    /// body is not entered (`call_enter`), so the helper is the whole construction.
    fn call_receiver(&mut self, stage: &mut Emit<'_, '_>, constructor: ir::Value) -> Option<ir::Value> {
        if let Some(CallSite { initializing: Some((first, count)), registers, .. }) = self.call_site() {
            let arguments = stage.add_imm(registers, crate::staged::word_bytes(first));
            let argc = stage.constant(u64::from(count));
            let helper = self.helpers.address(&mut stage.builder, LaneHelper::Construct);
            return Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor, arguments, argc]));
        }
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
        // An initializing construction's receiver is finished (`call_receiver`): the body is not entered,
        // so no frame is pushed and no entry is called. The status is the returned one as a constant, which
        // `complete_call` takes as the construction's receiver; `call_settle` then restores the tops it
        // admitted (unchanged, as nothing was pushed).
        if call.initializing.is_some() {
            let returned = stage.constant(u64::from(LANE_BODY_RETURNED));
            let unfinished = stage.builder.create_block();
            stage.builder.set_cold_block(unfinished);
            return Some(Entered { status: returned, word: this, unfinished });
        }
        self.push_native(stage, callee, this, room);
        let entry = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, entry)));
        let signature = lane_body_signature(types::I64, stage.builder.func.signature.call_conv);
        let signature = stage.builder.import_signature(signature);
        let captures = address(self.helpers, stage.builder, RelocKind::CaptureWords, callee.target.function,
            callee.capture_word());
        let (status, word) = stage.call_entry(signature, entry, &[call.context, this, captures, room.top]);
        // The status test's fall-through is the returned path (`complete_call`); every other status is cold.
        let unfinished = stage.builder.create_block();
        stage.builder.set_cold_block(unfinished);
        Some(Entered { status, word, unfinished })
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
        let (left, missed, threw, raised) = (stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block());
        for block in [left, missed, threw, raised] {
            stage.builder.set_cold_block(block);
        }
        stage.builder.switch_to_block(entered.unfinished);
        let not_entered = status_is(stage, entered.status, StatusClass::NotEntered);
        stage.branch(not_entered, (missed, &[]), (threw, &[]));
        stage.builder.switch_to_block(missed);
        self.pop_native(stage, room);
        stage.jump(helper, &[]);
        stage.builder.switch_to_block(threw);
        let did_raise = status_is(stage, entered.status, StatusClass::Threw);
        stage.branch(did_raise, (raised, &[]), (left, &[]));
        stage.builder.switch_to_block(raised);
        let counters = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, counters)));
        let counted = stage.add_imm(counters, RAISED_RUN);
        stage.store_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, counters)), counted);
        self.pop_native(stage, room);
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
        // Where a direct entry was emitted, the VM's call is reached only from a guard's miss, so its
        // blocks are cold and the completed path stays the fall-through of the direct call.
        let direct_entry = direct.is_some();
        if !direct_entry {
            stage.jump(helper, &[]);
        }
        stage.builder.switch_to_block(helper);
        if direct_entry { stage.builder.set_cold_block(helper); }
        let which = if site.constructs() { LaneHelper::ConstructCall } else { LaneHelper::Call };
        let context = emit_js_payload_word(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, lane_context));
        let record = stage.raw_const(Lane::I64, i64::from(call.record));
        let address = self.helpers.address(&mut stage.builder, which);
        let status = emit_lane_helper_call(stage.builder, self.payload, address, &[context, record]);
        // The status dispatch, generated from the shared outcome table: each test but the first
        // is cold where a direct entry was emitted; the last miss leaves with the status.
        let outcomes = crate::vm::compiled::meaning_compiled::HELPER_OUTCOMES;
        for (index, (code, outcome)) in outcomes.into_iter().enumerate() {
            let target = match outcome {
                crate::vm::compiled::meaning_compiled::HelperOutcome::Completed => call.completed,
                crate::vm::compiled::meaning_compiled::HelperOutcome::Missed => call.missed,
                crate::vm::compiled::meaning_compiled::HelperOutcome::Raised => call.raised,
            };
            let hit = stage.raw_compare_imm(Relation::Equal, status, code as i64);
            if index + 1 == outcomes.len() {
                stage.branch(hit, (target, &[]), (call.stop, &[status]));
            } else {
                let other = stage.builder.create_block();
                stage.branch(hit, (target, &[]), (other, &[]));
                stage.builder.switch_to_block(other);
                if direct_entry { stage.builder.set_cold_block(other); }
            }
        }
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
