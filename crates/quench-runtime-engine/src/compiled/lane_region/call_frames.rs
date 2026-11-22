//! The lane's call hooks (`staged::CallFrame`) and its return (`Frame::ret`): where a call's
//! words live in a lane body, and where each of its statuses continues. The order of the call
//! (identity, `this`, room, window, entry, completion) is `staged::invoke`; this file keeps the
//! words it reads (the register file, the lane stack's tops, the callee's entry cell) and the
//! blocks the lowering made for its exits. A callee the body owns (`DirectCallee`) has its words
//! as constants; an image's proven callee has them in its `CallRecord` (`ProvenCall`), which the
//! loader binds, so the code names the record by `Symbol::CallRecord` and never a JIT-time address.

use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::machine::{Lane, MachineScalar, Relation};
use super::frame::{Edges, EmitFrame, FRAME};
use super::relocs::address;
use super::super::entry_cell::{EntryCell, RAISED_RUN};
use quench_bjit::call_record::{CallRecord, LaneStatus};
use crate::dispatch_fast::LaneRecoverySpace;
use super::super::lane_body_abi::body_signature_in_lane;
use quench_runtime::staged::call::admit_room;
use quench_runtime::staged::{Access, Region};
use quench_runtime::staged::{plan_entry, Arguments, CallEntry, CallFrame, CalleeKind, DirectPlan, Entered, Identity, Invocation, Resolved, Room, Stage};
use quench_runtime::typed_ir::BindingTime;

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
    /// The callee a body calls by its entry: one the body owns, or an image's proven callee; `None` where the VM calls.
    pub(super) callee: Option<DirectSite<'a>>,
    /// The record the VM's call reads, and the lane context a body enters with.
    pub(super) record: u32,
    pub(super) context: ir::Value,
    /// The base of the caller's registers, written back for the call.
    pub(super) registers: ir::Value,
    /// A completed call continues at `completed`, a miss leaves at `missed`, a callee that ended by
    /// an exception (`Vm::lane_raised`) continues at `raised` (`Lowering::raised_call`), and a
    /// leave in a callee, or a termination, reaches `stop` with its status.
    pub(super) completed: ir::Block,
    /// Where a native return of a callee whose summary types the result continues, with the returned word
    /// as its parameter (`Lowering::typed_completion`); `None` completes at `completed`.
    pub(super) returned: Option<ir::Block>,
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

/// The callee a body calls by its entry: one the body owns (a JIT plan's `DirectCallee`), or a call of an image the
/// closed world proves, whose run-time words its record holds.
#[derive(Clone, Copy)]
pub(super) enum DirectSite<'a> {
    Owned(&'a DirectCallee),
    Proven(ProvenCall),
}

/// An image's call whose callee the closed world proves (`AotFacts::call_target`): the callee's function, which fixes
/// its window and `this` form, and the symbol of the site's record (`Symbol::CallRecord`, as `(namespace, key)`). The
/// call reads the callee's identity, cell, descriptor, captures, global, caller call and caller base from the record,
/// which the loader binds (`BoundCalls::bind`). Until a binding the record's identity is no callee's, so the identity
/// guard sends every call to the VM's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::compiled) struct ProvenCall {
    pub(in crate::compiled) function: u32,
    pub(in crate::compiled) registers: u16,
    pub(in crate::compiled) sloppy: bool,
    pub(in crate::compiled) record: (u32, u32),
}

impl DirectSite<'_> {
    /// The callee's window size: its code's, the same for either form.
    fn registers(self) -> u16 {
        match self {
            Self::Owned(callee) => callee.registers,
            Self::Proven(call) => call.registers,
        }
    }

    /// The function the callee is: the owned callee's target, or the proven callee's function.
    fn function(self) -> u32 {
        match self {
            Self::Owned(callee) => callee.target.function,
            Self::Proven(call) => call.function,
        }
    }
}

/// The alias region of a proven call's record, which no code writes while it runs.
const CALL_RECORD: Access = Region::EntryCell.readonly();

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
        emit(builder).store_u64(Region::Dispatch.mutable(), payload, field(std::mem::offset_of!(JsStepDispatch, head.returned_pc)), record);
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

    /// The word at `offset` of a proven call's record: the record's address is its symbol's.
    fn record_word(&self, stage: &mut Emit<'_, '_>, call: ProvenCall, offset: usize) -> ir::Value {
        let record = crate::symbol_words::symbol_address(stage.builder, call.record.0, call.record.1);
        stage.load_u64(CALL_RECORD, record, field(offset))
    }

    /// A word of the dispatch: the addresses of the lane stack's tops and its ends.
    fn stack_word(&self, stage: &mut Emit<'_, '_>, offset: usize) -> ir::Value {
        stage.load_u64(DISPATCH, self.payload, field(offset))
    }

    /// The lane stack's values' top, read through the word that holds it: the callee's window begins there.
    fn values_top(&self, stage: &mut Emit<'_, '_>) -> ir::Value {
        let values = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.lane_values_top));
        stage.load_u64(DISPATCH, values, 0)
    }

    /// Writes the values' top through its word.
    fn set_values_top(&self, stage: &mut Emit<'_, '_>, top: ir::Value) {
        let values = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.lane_values_top));
        stage.store_u64(DISPATCH, values, 0, top);
    }

    /// Whether the arguments of `site` have the entry types of `callee`'s body (`LaneMethod::entry_param_types`): the
    /// argument window's register `k` holds a value of parameter `k`'s type (`self.state`), so the body's proven entry,
    /// which skips those guards, takes them as the caller proved them. A call that does not pass a register window, that
    /// forwards its arguments, or whose target a forwarder names proves none.
    /// An image's proven callee pins no body, so it proves none and enters its cell's entry.
    fn arguments_proven(&self, site: &Invocation, callee: DirectSite<'_>) -> bool {
        let DirectSite::Owned(callee) = callee else { return false };
        let Some(types) = callee.body.as_deref().and_then(|body| body.entry_param_types()) else { return false };
        let Arguments::Window { first, count } = site.arguments else { return false };
        if site.forwards || callee.forwarder.is_some() || usize::from(count) < types.len() { return false; }
        types.iter().enumerate().all(|(index, ty)| self.state.values.get(usize::from(first) + index)
            .is_some_and(|fact| fact.value.ty.is_subtype(*ty)))
    }

    /// The entry a direct call enters: the callee's proven entry where the call's arguments are proved to have its
    /// parameter types (`arguments_proven`) and the cell publishes the very body the call pinned (`DirectCallee::body`,
    /// `EntryCell::proven_body`), else the cell's entry. A cell that holds no proven entry, or another body's, enters `entry`.
    fn entry_target(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, cell: ir::Value, entry: ir::Value) -> ir::Value {
        let DirectSite::Owned(callee) = callee else { return entry };
        let (true, Some(body)) = (self.proven_arguments.get(), callee.body.as_ref()) else { return entry };
        let proven = stage.load_u64(ENTRY_CELL, cell, field(std::mem::offset_of!(EntryCell, proven_entry)));
        let owner = stage.load_u64(ENTRY_CELL, cell, field(std::mem::offset_of!(EntryCell, proven_body)));
        let pinned = stage.constant(std::rc::Rc::as_ptr(body).addr() as u64);
        let same_body = stage.sub(owner, pinned);
        let same_body = stage.eq_imm(same_body, 0_u64);
        let present = stage.ne_imm(proven, 0_u64);
        let holds = stage.all(same_body, present);
        stage.select(holds, proven, entry)
    }

    /// The total frame count a native header is indexed by (one-frame-one-call.md F5): the frames
    /// below the plain headers (`*activation_count`) plus the plain depth, the count before the push.
    fn frame_index(&self, stage: &mut Emit<'_, '_>) -> ir::Value {
        let count_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.activation_count));
        let count = stage.load_u64(DISPATCH, count_at, 0);
        let plain = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.plain_depth));
        let depth = stage.load_u64(Region::Payload.mutable(), plain, quench_runtime::staged::frame::PLAIN_DEPTH_AT as i32);
        stage.add(count, depth)
    }

    /// Admit the callee's window to the lane stack's values, and its native header to the plain frames
    /// (the index bound: `index + RECOVERY_ACTIVATIONS_AT + 1 <= plain_depth_limit`), each a room row
    /// leaving at `miss`.
    fn admit_stacks(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, cell: ir::Value, miss: ir::Block) -> Option<Room<ir::Value>> {
        let word = std::mem::size_of::<Value>() as u64;
        let top = self.values_top(stage);
        let end = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.lane_values_end));
        // The next window's context words follow the callee's registers (`lane_values::CONTEXT`).
        let next = stage.add_imm(top, (u64::from(callee.registers()) + quench_runtime::vm::register_file::FRAME_CONTEXT as u64) * word);
        let recovery = stage.load_u64(ENTRY_CELL, cell, field(std::mem::offset_of!(EntryCell, recovery)));
        let recovered_values = stage.load_u64(ENTRY_CELL, recovery, field(std::mem::offset_of!(LaneRecoverySpace, values)));
        let bytes = stage.mul_imm(recovered_values, word);
        let reserved = stage.add(next, bytes);
        admit_room(stage, end, reserved, miss)?;
        let index = self.frame_index(stage);
        let activations = stage.load_u64(ENTRY_CELL, recovery, field(std::mem::offset_of!(LaneRecoverySpace, activations)));
        let frames = stage.add_imm(activations, 1_u64);
        let reserved = stage.add(index, frames);
        let limit_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.plain_depth_limit));
        let limit = stage.load_u64(DISPATCH, limit_at, 0);
        admit_room(stage, limit, reserved, miss)?;
        Some(Room { cell, top, next, shadow: index })
    }

    /// The callee's native header (one-frame-one-call.md F5), written below its window with NATIVE_FLAG:
    /// its receiver, the caller's registers, the callee's constant words and the packed word. Its body and
    /// descriptor go in the tables at its index, the plain depth is bumped and the window set as the innermost.
    /// The link (`native_frames::link_header`) fills the call record, caller base, argc and side record.
    fn push_native(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, this: ir::Value, caller_registers: ir::Value, room: Room<ir::Value>) {
        use quench_runtime::staged::frame::{CONTEXT_CALL, CONTEXT_CALLEE, CONTEXT_CALLER_BASE, CONTEXT_CALLER_REGISTERS, CONTEXT_PACKED, CONTEXT_THIS};
        let window = room.top;
        let at = Region::Frame.mutable();
        // The words the callee's identity fixes are constants (its function's packed word); the words the binding
        // makes are the record's (a proven callee's), a constant of the owned callee's own.
        let (descriptor, call, caller_base, callee_word) = match callee {
            DirectSite::Owned(owned) => (
                address(self.helpers, stage.builder, RelocKind::Descriptor, owned.target.function, std::ptr::from_ref(owned).addr() as u64),
                stage.constant(owned.call as u64),
                stage.constant(owned.caller_base as u64),
                stage.constant(owned.callee.to_bits()),
            ),
            DirectSite::Proven(proven) => (
                self.record_word(stage, proven, std::mem::offset_of!(CallRecord, descriptor)),
                self.record_word(stage, proven, std::mem::offset_of!(CallRecord, call)),
                self.record_word(stage, proven, std::mem::offset_of!(CallRecord, caller_base)),
                self.record_word(stage, proven, std::mem::offset_of!(CallRecord, callee)),
            ),
        };
        let body = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, body)));
        let packed = stage.constant(u64::from(callee.function()) | quench_runtime::staged::frame::PLAIN_FLAG | quench_runtime::staged::frame::NATIVE_FLAG);
        // Unlike the b-jit call row, which writes the receiver only where the body's `uses_this` cell word says it
        // reads `this`, this writes it always: loading that word here would add a load and a branch to the
        // emitted header, and an unread receiver word is harmless.
        stage.store_u64(at, window, context_at(CONTEXT_THIS), this);
        stage.store_u64(at, window, context_at(CONTEXT_CALL), call);
        stage.store_u64(at, window, context_at(CONTEXT_CALLER_REGISTERS), caller_registers);
        stage.store_u64(at, window, context_at(CONTEXT_CALLER_BASE), caller_base);
        stage.store_u64(at, window, context_at(CONTEXT_PACKED), packed);
        stage.store_u64(at, window, context_at(CONTEXT_CALLEE), callee_word);
        // The index's entries: the body and the descriptor, one word each.
        let offset = stage.mul_imm(room.shadow, std::mem::size_of::<usize>() as u64);
        let bodies = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.body_table));
        let body_at = stage.add(bodies, offset);
        stage.store_u64(at, body_at, 0, body);
        let descriptors = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.native_table));
        let descriptor_at = stage.add(descriptors, offset);
        stage.store_u64(at, descriptor_at, 0, descriptor);
        // The plain frames: one more header, with the window as the innermost.
        let plain = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.plain_depth));
        let depth = stage.load_u64(Region::Payload.mutable(), plain, quench_runtime::staged::frame::PLAIN_DEPTH_AT as i32);
        let depth = stage.add_imm(depth, 1_u64);
        stage.store_u64(Region::Payload.mutable(), plain, quench_runtime::staged::frame::PLAIN_DEPTH_AT as i32, depth);
        stage.store_u64(Region::Payload.mutable(), plain, quench_runtime::staged::frame::PLAIN_REGISTERS_AT as i32, window);
        let count_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.body_top));
        let index = stage.add_imm(room.shadow, 1_u64);
        stage.store_u64(DISPATCH, count_at, 0, index);
        self.set_values_top(stage, room.next);
    }

    /// Pops the callee's native header, where the call began (`Room`): the plain depth drops, the innermost
    /// window becomes the header's caller registers, the body top returns to the index, and the values' top
    /// returns to the window.
    fn pop_native(&self, stage: &mut Emit<'_, '_>, room: Room<ir::Value>) {
        use quench_runtime::staged::frame::{CONTEXT_CALLER_REGISTERS, PLAIN_DEPTH_AT, PLAIN_REGISTERS_AT};
        let caller = stage.load_u64(Region::Frame.mutable(), room.top, context_at(CONTEXT_CALLER_REGISTERS));
        let plain = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.plain_depth));
        let depth = stage.load_u64(Region::Payload.mutable(), plain, PLAIN_DEPTH_AT as i32);
        // A nested entry to emitted code under the callee (`Vm::stack_words_in_lane`) made the header a
        // record (`LaneStack::spill_plain`): the record pops instead, where Rust keeps it.
        let (spilled, plain_header, popped) = (stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block());
        stage.builder.set_cold_block(spilled);
        stage.builder.ins().brif(depth, plain_header, &[], spilled, &[]);
        stage.builder.switch_to_block(spilled);
        let helper = self.helpers.address(&mut stage.builder, LaneHelper::PopSpilledHeader);
        emit_lane_helper_call(stage.builder, self.payload, helper, &[]);
        stage.builder.ins().jump(popped, &[]);
        stage.builder.switch_to_block(plain_header);
        let depth = stage.add_imm(depth, u64::MAX);
        stage.store_u64(Region::Payload.mutable(), plain, PLAIN_DEPTH_AT as i32, depth);
        stage.store_u64(Region::Payload.mutable(), plain, PLAIN_REGISTERS_AT as i32, caller);
        stage.builder.ins().jump(popped, &[]);
        stage.builder.switch_to_block(popped);
        let count_at = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.body_top));
        stage.store_u64(DISPATCH, count_at, 0, room.shadow);
        self.set_values_top(stage, room.top);
    }
}

/// The context word at `index` counted down from register 0 (`staged::frame`).
const fn context_at(index: usize) -> i32 { -((index * std::mem::size_of::<u64>()) as i32) }

impl CallFrame<Emit<'_, '_>> for EmitFrame<'_> {
    fn call_entry(&self, site: &Invocation) -> CallEntry {
        self.proven_arguments.set(false);
        let Some(CallSite { callee: Some(callee), .. }) = self.call_site() else { return CallEntry::Helper };
        self.proven_arguments.set(self.arguments_proven(site, callee));
        let kind = if site.constructs() { CalleeKind::Class } else { CalleeKind::Script };
        let (identity, sloppy) = match callee {
            DirectSite::Owned(owned) => {
                // A callee register the state already holds as this very function (a Stable binding's
                // read, or an identity an earlier guard proved) needs no check: the identity is folded.
                let word = owned.callee.to_bits();
                let proven = matches!(site.callee, quench_runtime::staged::forms::Callee::Register(register)
                    if self.state.values[usize::from(register)].value.ty.function_identity().map(|identity| identity.to_bits()) == Some(word));
                let identity = if proven { Identity::Folded(word) } else { Identity::Guarded(word) };
                (identity, owned.sloppy.map(Value::to_bits))
            }
            // The identity and the global are the record's words (`call_identity`, `call_global`): the plan's
            // constants stand for them and are never emitted. The record's identity is checked against a register
            // only, so a call of any other form takes the VM's (its callee is not the record's to check).
            DirectSite::Proven(proven) => {
                if !matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_)) { return CallEntry::Helper; }
                (Identity::Guarded(quench_runtime::vm::aot_image::unbound_callee()), proven.sloppy.then(quench_runtime::vm::aot_image::unbound_callee))
            }
        };
        let plan = DirectPlan { kind, identity, sloppy, registers: callee.registers(), returned: LANE_BODY_RETURNED, bound: None };
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
            let arguments = stage.add_imm(registers, quench_runtime::staged::word_bytes(first));
            let argc = stage.constant(u64::from(count));
            let helper = self.helpers.address(&mut stage.builder, LaneHelper::Construct);
            return Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor, arguments, argc]));
        }
        let helper = self.helpers.address(&mut stage.builder, LaneHelper::ConstructReceiver);
        Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor]))
    }

    fn call_room(&mut self, stage: &mut Emit<'_, '_>, miss: ir::Block) -> Option<Room<ir::Value>> {
        let callee = self.call_site()?.callee?;
        let cell = match callee {
            DirectSite::Owned(owned) => address(self.helpers, stage.builder, RelocKind::EntryCell, owned.target.function, owned.cell as u64),
            DirectSite::Proven(proven) => self.record_word(stage, proven, std::mem::offset_of!(CallRecord, cell)),
        };
        self.admit_stacks(stage, callee, cell, miss)
    }

    /// The identity an image's proven call checks its callee register against: the record's word, which the binding
    /// makes the callee's identity (`BoundCalls::bind`) and which no callee holds while the record is unbound.
    /// A body's own callee has its identity as the plan's constant, so the frame reads none.
    fn call_identity(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            DirectSite::Proven(proven) => Some(self.record_word(stage, proven, std::mem::offset_of!(CallRecord, callee))),
            DirectSite::Owned(_) => None,
        }
    }

    /// The global object a proven sloppy callee binds, from the record's `global_this` (`BoundCalls::bind`).
    fn call_global(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            DirectSite::Proven(proven) => Some(self.record_word(stage, proven, std::mem::offset_of!(CallRecord, global_this))),
            DirectSite::Owned(_) => None,
        }
    }

    /// The call of the entry the callee's cell holds, with the lane context, the receiver, the
    /// callee's captures and its window; both tops stay pushed, which a completion pops and a leave keeps
    /// for the lane to link.
    fn call_enter(&mut self, stage: &mut Emit<'_, '_>, this: ir::Value, room: Room<ir::Value>) -> Option<Entered<ir::Value, ir::Block>> {
        let call = self.call_site()?;
        let callee = call.callee?;
        // An initializing construction's receiver is finished (`call_receiver`): the body is not entered,
        // so no frame is pushed and no entry is called. The status is the returned one as a constant, which
        // `complete_call` takes as the construction's receiver; `call_settle` pops no header for it.
        if call.initializing.is_some() {
            let returned = stage.constant(u64::from(LANE_BODY_RETURNED));
            let unfinished = stage.builder.create_block();
            stage.builder.set_cold_block(unfinished);
            return Some(Entered { status: returned, word: this, unfinished });
        }
        self.push_native(stage, callee, this, call.registers, room);
        let entry = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, entry)));
        let entry = self.entry_target(stage, callee, room.cell, entry);
        let signature = body_signature_in_lane(types::I64, stage.builder.func.signature.call_conv);
        let signature = stage.builder.import_signature(signature);
        let captures = match callee {
            DirectSite::Owned(owned) => address(self.helpers, stage.builder, RelocKind::CaptureWords, owned.target.function, owned.capture_word()),
            DirectSite::Proven(proven) => self.record_word(stage, proven, std::mem::offset_of!(CallRecord, captures)),
        };
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
    /// the callee, which stops the caller with it (`dispatch_fast::body_left_in_lane`).
    fn call_settle(&mut self, stage: &mut Emit<'_, '_>, site: &Invocation, entered: Entered<ir::Value, ir::Block>, room: Room<ir::Value>,
        value: ir::Value) -> Option<()> {
        let call = self.call_site()?;
        let helper = self.helper_block()?;
        // An initializing construction pushed no header (`call_enter`): there is none to pop.
        if call.initializing.is_none() { self.pop_native(stage, room); }
        stage.store_u64(FRAME, call.registers, register_offset(site.result), value);
        match call.returned {
            Some(returned) => stage.jump(returned, &[value]),
            None => stage.jump(call.completed, &[]),
        }
        let (left, missed, threw, raised) = (stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block(), stage.builder.create_block());
        for block in [left, missed, threw, raised] {
            stage.builder.set_cold_block(block);
        }
        stage.builder.switch_to_block(entered.unfinished);
        let not_entered = stage.raw_compare_imm(Relation::Equal, entered.status, i64::from(LaneStatus::NotEntered as u32));
        stage.branch(not_entered, (missed, &[]), (threw, &[]));
        stage.builder.switch_to_block(missed);
        self.pop_native(stage, room);
        stage.jump(helper, &[]);
        stage.builder.switch_to_block(threw);
        let did_raise = stage.raw_compare_imm(Relation::Equal, entered.status, i64::from(LaneStatus::Threw as u32));
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
        let context = emit_js_payload_word(stage.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
        let record = stage.raw_const(Lane::I64, i64::from(call.record));
        let address = self.helpers.address(&mut stage.builder, which);
        let status = emit_lane_helper_call(stage.builder, self.payload, address, &[context, record]);
        // The status dispatch, generated from the shared outcome table: each test but the first
        // is cold where a direct entry was emitted; the last miss leaves with the status.
        let outcomes = quench_runtime::staged::protocol::HELPER_OUTCOMES;
        for (index, (code, outcome)) in outcomes.into_iter().enumerate() {
            let target = match outcome {
                quench_runtime::staged::protocol::HelperOutcome::Completed => call.completed,
                quench_runtime::staged::protocol::HelperOutcome::Missed => call.missed,
                quench_runtime::staged::protocol::HelperOutcome::Raised => call.raised,
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
