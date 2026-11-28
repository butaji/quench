//! The lane's call hooks (`staged::CallFrame`) and its return (`Frame::ret`): where a call's
//! words live in a lane body, and where each of its statuses continues. The order of the call
//! (identity, `this`, room, window, entry, completion) is `staged::invoke`; this file keeps the
//! words it reads (the register file, the lane stack's tops, the callee's entry cell) and the
//! blocks the lowering made for its exits. A callee the body owns (`DirectCallee`) has its words
//! as constants; an image's proven callee has them in its `CalleeRecord` (`ProvenCall`), which the
//! loader binds, so the code names the record by `Symbol::CallRecord` and never a JIT-time address.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::object_slots::emit;
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::machine::{Lane, MachineScalar, Relation};
use super::frame::{Edges, EmitFrame, FRAME};
use super::relocs::address;
use super::super::entry_cell::EntryCell;
use quench_bjit::call_record::{CalleeRecord, CallRecord, LaneStatus};
use quench_runtime::vm::engine_types::BoundClosure;
use crate::dispatch_fast::LaneRecoverySpace;
use quench_runtime::staged::call::admit_room;
use quench_runtime::staged::{Access, Region};
use quench_runtime::staged::{plan_entry, Arguments, CallEntry, CallFrame, CalleeKind, DirectPlan, Entered, Identity, Invocation, Resolved, Room, Stage};
use quench_runtime::typed_ir::BindingTime;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// Where a run's completion goes: the block a run leaves at with its status and result word.
#[derive(Clone, Copy)]
pub(crate) struct Leave {
    pub(crate) block: ir::Block,
}

impl Leave {
    /// Leaves with `status`, and with `word` as the result word where a run publishes one.
    pub(crate) fn with(self, builder: &mut FunctionBuilder<'_>, status: ir::Value, word: Option<ir::Value>) {
        let word = word.unwrap_or_else(|| emit(builder).raw_const(Lane::I64, 0));
        emit(builder).jump(self.block, &[status, word]);
    }
}

/// What the lowering knows of the call at one record: the callee its site is bound to, the
/// words the call reads, and the blocks its statuses continue at.
#[derive(Clone, Copy)]
pub(crate) struct CallSite<'a> {
    /// The callee a body calls by its entry: one the body owns, or an image's proven callee; `None` where the VM calls.
    pub(crate) callee: Option<DirectSite<'a>>,
    /// The record the VM's call reads, and the lane context a body enters with.
    pub(crate) record: u32,
    pub(crate) context: ir::Value,
    /// The base of the caller's registers, written back for the call.
    pub(crate) registers: ir::Value,
    /// A completed call continues at `completed`, a miss leaves at `missed`, a callee that ended by
    /// an exception (`Vm::lane_raised`) continues at `raised` (`Lowering::raised_call`), and a
    /// leave in a callee, or a termination, reaches `stop` with its status.
    pub(crate) completed: ir::Block,
    /// Where a native return of a callee whose summary types the result continues, with the returned word
    /// as its parameter (`Lowering::typed_completion`); `None` completes at `completed`.
    pub(crate) returned: Option<ir::Block>,
    pub(crate) missed: ir::Block,
    pub(crate) raised: ir::Block,
    pub(crate) stop: ir::Block,
    pub(crate) leave: Leave,
    /// A construction whose target's body only initializes `this` (`Function::only_initializes_this_fields`
    /// of an `inline_constructible` function, the facts `dispatch_fast` makes `TARGET_INITIALIZES`), and
    /// the window of arguments it passes in the caller's registers. Its receiver is allocated finished by
    /// `LaneHelper::Construct` (`lane_body_construction`), which is the construction's result; its body is
    /// not entered (`call_enter`), as the VM's own construction does not run it (`initializes()`).
    pub(crate) initializing: Option<(Register, u16)>,
    /// The construct site of a construction (`Lowering::construct_site`), which its receiver's class is named by; `None`
    /// for a call that is no construction.
    pub(crate) construct: Option<u64>,
    /// A sibling tail call (DESIGN.md 4.4) of an owned body: the window of the call's arguments (`first`, `count`). The
    /// running frame becomes the callee's, and the body returns through the callee's entry (`tail_call`); the call's
    /// completion is the callee's. `None` where the call pushes its own frame.
    pub(crate) tail: Option<(Register, u16)>,
}

/// The callee a body calls by its entry: one the body owns (a JIT plan's `DirectCallee`), or a call of an image the
/// closed world proves, whose run-time words its record holds.
#[derive(Clone, Copy)]
pub(crate) enum DirectSite<'a> {
    Owned(&'a DirectCallee),
    Proven(ProvenCall),
    /// A closed-set call through its receiver class's method vector (`closed_set_indirect`): `callee` is the callee record
    /// the vector holds at run time, whose words the call reads; `site` is the arm the call's site is, whose record holds
    /// the site's own words (the caller's call record and base), which every arm of the call shares.
    Indirect { site: ProvenCall, callee: ir::Value },
}

/// An image's call whose callee the closed world proves (`AotFacts::call_target`): the callee's function, which fixes
/// its window and `this` form, and the symbol of the site's record (`Symbol::CallRecord`, as `(namespace, key)`). The
/// call reads the callee's identity, cell, descriptor, captures, global from the callee record the site names, and the
/// caller call and caller base from the site's record,
/// which the loader binds (`BoundCalls::bind`). Until a binding the record's identity is no callee's, so the identity
/// guard sends every call to the VM's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProvenCall {
    pub(crate) function: u32,
    pub(crate) registers: u16,
    pub(crate) sloppy: bool,
    pub(crate) record: (u32, u32),
    /// The `Symbol::ClassLayout` key of the arm's receiver class, where the closed world proves its layout (`None` else).
    pub(crate) layout: Option<u32>,
    /// Whether the call's plan has no miss (`Miss::None`): the arms name every receiver the call can have.
    pub(crate) closed: bool,
    /// The selector of the call's method (`SymbolTable::selector`): the index of its record in each class's method vector
    /// (`Symbol::ClassMethods`), which the indirect form reads. `None` where the call names no method.
    pub(crate) selector: Option<u32>,
    /// Whether the call's plan inlines its arms (`tier_policy::arms_inline`), which keeps the class-layout compare form.
    pub(crate) inline: bool,
}

impl DirectSite<'_> {
    /// The callee's window size: its code's, where the callee's identity fixes it. An indirect callee's record holds it
    /// at run time, which the generic call protocol does not take yet (`DirectPlan::registers`), so it has none here.
    fn registers(self) -> Option<u16> {
        match self {
            Self::Owned(callee) => Some(callee.registers),
            Self::Proven(call) => Some(call.registers),
            Self::Indirect { .. } => None,
        }
    }

    /// The function the callee is: the owned callee's target, or the proven callee's function. An indirect callee's
    /// function is its record's packed word's, which the same protocol gap leaves unread (`registers`).
    fn function(self) -> Option<u32> {
        match self {
            Self::Owned(callee) => Some(callee.target.function),
            Self::Proven(call) => Some(call.function),
            Self::Indirect { .. } => None,
        }
    }
}

/// The alias region of a proven call's record, which no code writes while it runs.
const CALL_RECORD: Access = Region::EntryCell.readonly();

/// The record that completes the activation a return ends (`LaneExit::Returned`), and where it
/// leaves.
#[derive(Clone, Copy)]
pub(crate) struct ReturnSite {
    pub(crate) record: u32,
    pub(crate) leave: Leave,
}

impl ReturnSite {
    /// Stops with the activation's completion: `value` in the result word, the record in the
    /// dispatch (`LANE_BODY_RETURNED`).
    pub(crate) fn complete(self, builder: &mut FunctionBuilder<'_>, payload: ir::Value, value: ir::Value) {
        let record = emit(builder).raw_const(Lane::I64, i64::from(self.record));
        emit(builder).store_u64(Region::Dispatch.mutable(), payload, field(std::mem::offset_of!(JsStepDispatch, head.returned_pc)), record);
        let returned = emit(builder).raw_const(Lane::I32, i64::from(LANE_BODY_RETURNED));
        self.leave.with(builder, returned, Some(value));
    }
}

/// The site a step of the call family runs for.
#[derive(Clone, Copy)]
pub(crate) enum Site<'a> {
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

    /// The address of the callee record a call names: a proven call's site record names it (`CallRecord::callee_record`),
    /// an indirect call's is the one its class's method vector holds. `None` for an owned callee, whose words are its own
    /// constants.
    fn callee_address(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>) -> Option<ir::Value> {
        match callee {
            DirectSite::Owned(_) => None,
            DirectSite::Proven(call) => Some(self.record_word(stage, call, std::mem::offset_of!(CallRecord, callee_record))),
            DirectSite::Indirect { callee, .. } => Some(callee),
        }
    }

    /// The word at `offset` of the callee record of a proven or indirect callee (`CalleeRecord`).
    fn callee_word(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, offset: usize) -> Option<ir::Value> {
        let record = self.callee_address(stage, callee)?;
        Some(stage.load_u64(CALL_RECORD, record, field(offset)))
    }

    /// The word at `offset` of the site's own record (`CallRecord`) of a proven or indirect call: the proven call's symbol,
    /// or for an indirect call the symbol of the arm it names (every arm of the call shares the site's words). `None` for
    /// an owned callee, whose words are its own constants.
    fn site_word(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, offset: usize) -> Option<ir::Value> {
        match callee {
            DirectSite::Owned(_) => None,
            DirectSite::Proven(call) | DirectSite::Indirect { site: call, .. } => Some(self.record_word(stage, call, offset)),
        }
    }

    /// The word at `offset` of an owned callee's bound closure (`DirectCallee::bound`), which the site's helper call
    /// rebinds: a mutable word, read again after each call out.
    fn bound_word(&self, stage: &mut Emit<'_, '_>, owned: &DirectCallee, offset: usize) -> ir::Value {
        let at = stage.constant(std::ptr::from_ref(&owned.bound).addr() as u64);
        stage.load_u64(ENTRY_CELL, at, field(offset))
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
    fn proven_entry(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, cell: ir::Value, entry: quench_runtime::staged::machine::CallTarget<ir::Value>) -> quench_runtime::staged::machine::CallTarget<ir::Value> {
        let DirectSite::Owned(callee) = callee else { return entry };
        let (true, Some(body)) = (self.proven_arguments.get(), callee.body.as_ref()) else { return entry };
        let proven = stage.entry_target(crate::compiled::slow_paths::EngineEntry::CompiledEntry, crate::compiled::regions::flags(ENTRY_CELL), cell, field(std::mem::offset_of!(EntryCell, proven_entry)));
        let owner = stage.load_u64(ENTRY_CELL, cell, field(std::mem::offset_of!(EntryCell, proven_body)));
        let pinned = stage.constant(std::rc::Rc::as_ptr(body).addr() as u64);
        let same_body = stage.sub(owner, pinned);
        let same_body = stage.eq_imm(same_body, 0_u64);
        let present = stage.ne_imm(proven.word(), 0_u64);
        let holds = stage.all(same_body, present);
        stage.select_target(holds, proven, entry)
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
        // The next window's context words follow the callee's registers (`lane_values::CONTEXT`), which an indirect callee's
        // record holds at run time.
        let context = quench_runtime::vm::register_file::FRAME_CONTEXT as u64;
        let next = match callee.registers() {
            Some(registers) => stage.add_imm(top, (u64::from(registers) + context) * word),
            None => {
                let registers = self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, registers))?;
                let bytes = stage.mul_imm(registers, word);
                let at = stage.add(top, bytes);
                stage.add_imm(at, context * word)
            }
        };
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

    /// The callee's descriptor word: the owned callee's constant, or its record's (`CalleeRecord::descriptor`).
    fn callee_descriptor(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>) -> Option<ir::Value> {
        match callee {
            DirectSite::Owned(owned) => Some(address(self.helpers, stage.builder, RelocKind::Descriptor, owned.target.function, std::ptr::from_ref(owned).addr() as u64)),
            DirectSite::Proven(_) | DirectSite::Indirect { .. } => self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, descriptor)),
        }
    }

    /// The callee identity word of a native header: the owned callee's constant, or its bound closure's where the call
    /// rebinds, or its record's (`CalleeRecord::callee`).
    fn callee_identity(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>) -> Option<ir::Value> {
        match callee {
            DirectSite::Owned(owned) if self.rebinds.get() => Some(self.bound_word(stage, owned, std::mem::offset_of!(BoundClosure, callee))),
            DirectSite::Owned(owned) => Some(stage.constant(owned.callee.to_bits())),
            DirectSite::Proven(_) | DirectSite::Indirect { .. } => self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, callee)),
        }
    }

    /// The packed word of a native header: the callee's function with the plain and native flags. An indirect callee's
    /// record holds its function, so the header takes those bits and the flags.
    fn callee_packed(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>) -> Option<ir::Value> {
        let flags = quench_runtime::staged::frame::PLAIN_FLAG | quench_runtime::staged::frame::NATIVE_FLAG;
        match callee.function() {
            Some(function) => Some(stage.constant(u64::from(function) | flags)),
            None => {
                let word = self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, function))?;
                let function = stage.and_imm(word, u64::from(u32::MAX));
                Some(stage.or_imm(function, flags))
            }
        }
    }

    /// The callee's native header (one-frame-one-call.md F5), written below its window with NATIVE_FLAG:
    /// its receiver, the caller's registers, the callee's constant words and the packed word. Its body and
    /// descriptor go in the tables at its index, the plain depth is bumped and the window set as the innermost.
    /// The link (`native_frames::link_header`) fills the call record, caller base, argc and side record.
    fn push_native(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, this: ir::Value, caller_registers: ir::Value, room: Room<ir::Value>) -> Option<()> {
        use quench_runtime::staged::{frame::CONTEXT_THIS, protocol};
        let window = room.top;
        let at = Region::Frame.mutable();
        // The words the callee's identity fixes are constants; the words the binding makes are the records' (a
        // proven callee's, an indirect callee's).
        let (call, caller_base) = match callee {
            DirectSite::Owned(owned) => (stage.constant(owned.call as u64), stage.constant(owned.caller_base as u64)),
            DirectSite::Proven(_) | DirectSite::Indirect { .. } => (
                self.site_word(stage, callee, std::mem::offset_of!(CallRecord, call))?,
                self.site_word(stage, callee, std::mem::offset_of!(CallRecord, caller_base))?,
            ),
        };
        let descriptor = self.callee_descriptor(stage, callee)?;
        let callee_word = self.callee_identity(stage, callee)?;
        let packed = self.callee_packed(stage, callee)?;
        let body = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, body)));
        // Unlike the b-jit call row, which writes the receiver only where the body's `uses_this` cell word says it
        // reads `this`, this writes it always: loading that word here would add a load and a branch to the
        // emitted header, and an unread receiver word is harmless.
        stage.store_u64(at, window, context_at(CONTEXT_THIS), this);
        // The header, the index's entries and the plain frames: the native frame every tier pushes. Its
        // places load after the header's stores, so their addresses are not live across them.
        let header = protocol::BodyHeader { call, caller_registers, caller_base, packed, callee: callee_word };
        let tops = protocol::PayloadTops { payload: self.payload };
        protocol::push_native_frame(stage, &tops, protocol::NativeFrame { window, header, body, descriptor, index: room.shadow, next: room.next }, |stage| {
            let bodies = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.body_table));
            let descriptors = self.stack_word(stage, std::mem::offset_of!(JsStepDispatch, head.native_table));
            let plain_at = protocol::LaneWords::plain_at(&tops, stage);
            let depth = stage.load_u64(Region::Payload.mutable(), plain_at, quench_runtime::staged::frame::PLAIN_DEPTH_AT as i32);
            protocol::NativePlaces { bodies, descriptors, plain_at, depth }
        });
        Some(())
    }

    /// Pops the callee's native header, where the call began (`Room`): the plain depth drops, the innermost
    /// window becomes the header's caller registers, the body top returns to the index, and the values' top
    /// returns to the window.
    fn pop_native(&self, stage: &mut Emit<'_, '_>, room: Room<ir::Value>) {
        use quench_runtime::staged::protocol;
        let tops = protocol::PayloadTops { payload: self.payload };
        protocol::pop_native_frame(stage, &tops, room.top, room.shadow, None, |stage| {
            let helper = self.helpers.target(&mut stage.builder, LaneHelper::PopObservedHeader);
            emit_lane_helper_call(stage.builder, self.payload, helper, &[]);
        });
    }
}

/// The context word at `index` counted down from register 0 (`staged::frame`).
const fn context_at(index: usize) -> i32 { -((index * std::mem::size_of::<u64>()) as i32) }


/// Completes the activation with `value` (`Frame::ret`): only a return the lowering made a site
/// for can.
pub(crate) fn complete_activation(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, value: ir::Value) -> Option<()> {
    let Some(Site::Return(site)) = frame.site else { return None };
    site.complete(stage.builder, frame.payload, value);
    Some(())
}

#[path = "call_frames/frame_trait.rs"]
mod frame_trait;

#[path = "call_frames/self_tail.rs"]
mod self_tail;

#[path = "call_frames/tail_call.rs"]
mod tail_call;
