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
use super::super::entry_cell::{EntryCell, RAISED_RUN};
use quench_bjit::call_record::{CalleeRecord, CallRecord, LaneStatus};
use quench_runtime::vm::engine_types::BoundClosure;
use crate::dispatch_fast::LaneRecoverySpace;
use quench_runtime::staged::call::admit_room;
use quench_runtime::staged::{Access, Region};
use quench_runtime::staged::{plan_entry, Arguments, CallEntry, CallFrame, CalleeKind, DirectPlan, Entered, Identity, Invocation, Resolved, Room, Stage};
use quench_runtime::typed_ir::BindingTime;

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

    /// The callee's native header (one-frame-one-call.md F5), written below its window with NATIVE_FLAG:
    /// its receiver, the caller's registers, the callee's constant words and the packed word. Its body and
    /// descriptor go in the tables at its index, the plain depth is bumped and the window set as the innermost.
    /// The link (`native_frames::link_header`) fills the call record, caller base, argc and side record.
    fn push_native(&self, stage: &mut Emit<'_, '_>, callee: DirectSite<'_>, this: ir::Value, caller_registers: ir::Value, room: Room<ir::Value>) -> Option<()> {
        use quench_runtime::staged::{frame::CONTEXT_THIS, protocol};
        let window = room.top;
        let at = Region::Frame.mutable();
        // The words the callee's identity fixes are constants (its function's packed word); the words the binding
        // makes are the records' (a proven callee's, an indirect callee's), a constant of the owned callee's own.
        let (descriptor, call, caller_base, callee_word) = match callee {
            DirectSite::Owned(owned) => (
                address(self.helpers, stage.builder, RelocKind::Descriptor, owned.target.function, std::ptr::from_ref(owned).addr() as u64),
                stage.constant(owned.call as u64),
                stage.constant(owned.caller_base as u64),
                if self.rebinds.get() { self.bound_word(stage, owned, std::mem::offset_of!(BoundClosure, callee)) } else { stage.constant(owned.callee.to_bits()) },
            ),
            DirectSite::Proven(_) | DirectSite::Indirect { .. } => (
                self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, descriptor))?,
                self.site_word(stage, callee, std::mem::offset_of!(CallRecord, call))?,
                self.site_word(stage, callee, std::mem::offset_of!(CallRecord, caller_base))?,
                self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, callee))?,
            ),
        };
        let body = stage.load_u64(ENTRY_CELL, room.cell, field(std::mem::offset_of!(EntryCell, body)));
        let flags = quench_runtime::staged::frame::PLAIN_FLAG | quench_runtime::staged::frame::NATIVE_FLAG;
        // A proven callee's packed word is its function's constant; an indirect callee's record holds its function, so the
        // header takes those bits and the flags.
        let packed = match callee.function() {
            Some(function) => stage.constant(u64::from(function) | flags),
            None => {
                let word = self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, function))?;
                let function = stage.and_imm(word, u64::from(u32::MAX));
                stage.or_imm(function, flags)
            }
        };
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

impl CallFrame<Emit<'_, '_>> for EmitFrame<'_> {
    /// A callee's bound `this` is the callee's own fact (`dispatch_fast::receiver_trusted`): the call guards nothing.
    fn call_this_bound(&mut self, _stage: &mut Emit<'_, '_>, _this: ir::Value, _miss: ir::Block) -> Option<()> {
        Some(())
    }

    fn call_entry(&self, site: &Invocation) -> CallEntry {
        self.proven_arguments.set(false);
        self.rebinds.set(false);
        let Some(CallSite { callee: Some(callee), .. }) = self.call_site() else { return CallEntry::Helper };
        // An indirect callee's window is read at run time (`call_registers_dynamic`), so the plan's is unused for it.
        let registers = callee.registers().unwrap_or(0);
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
                // A guarded call of a JIT plan reads the closure it calls and its captures from the bound words, which
                // its helper call rebinds to another closure of the function (`DirectCallee::bound`).
                self.rebinds.set(!proven && !self.helpers.links() && !site.constructs() && owned.forwarder.is_none());
                if self.rebinds.get() { owned.bound.read.set(true); }
                (identity, owned.sloppy.map(Value::to_bits))
            }
            // The identity and the global are the record's words (`call_identity`, `call_global`): the plan's
            // constants stand for them and are never emitted. The record's identity is checked against a register
            // only, so a call of any other form takes the VM's (its callee is not the record's to check).
            DirectSite::Proven(proven) => {
                if !matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_)) { return CallEntry::Helper; }
                (Identity::Guarded(crate::aot_image::unbound_callee()), proven.sloppy.then(crate::aot_image::unbound_callee))
            }
            // An indirect callee's identity is its record's, which the closed-set guard checked: the register's identity is
            // guarded again here as a proven callee's is, and its sloppiness and window are its record's at run time.
            DirectSite::Indirect { .. } => {
                if !matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_)) { return CallEntry::Helper; }
                (Identity::Guarded(crate::aot_image::unbound_callee()), None)
            }
        };
        let plan = DirectPlan { kind, identity, sloppy, registers, returned: LANE_BODY_RETURNED, bound: None };
        // The callee is the one the site linked, which the call guards: a Site fact, never inlined by the plan.
        plan_entry(Some(Resolved { plan, binding: BindingTime::Site }), false)
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
            let helper = self.helpers.target(&mut stage.builder, LaneHelper::Construct);
            return Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor, arguments, argc]));
        }
        let helper = self.helpers.target(&mut stage.builder, LaneHelper::ConstructReceiver);
        Some(emit_lane_helper_call(stage.builder, self.payload, helper, &[self.payload, constructor]))
    }

    /// An indirect callee's window is its record's words, read at run time.
    fn call_registers_dynamic(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            callee @ DirectSite::Indirect { .. } => self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, registers)),
            DirectSite::Owned(_) | DirectSite::Proven(_) => None,
        }
    }

    /// An indirect callee's sloppiness is its record's word, read at run time.
    fn call_sloppy_dynamic(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            callee @ DirectSite::Indirect { .. } => self.callee_word(stage, callee, std::mem::offset_of!(CalleeRecord, sloppy)),
            DirectSite::Owned(_) | DirectSite::Proven(_) => None,
        }
    }

    fn call_room(&mut self, stage: &mut Emit<'_, '_>, miss: ir::Block) -> Option<Room<ir::Value>> {
        let callee = self.call_site()?.callee?;
        let cell = match callee {
            DirectSite::Owned(owned) => address(self.helpers, stage.builder, RelocKind::EntryCell, owned.target.function, owned.cell as u64),
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, cell))?,
        };
        self.admit_stacks(stage, callee, cell, miss)
    }

    /// The identity an image's proven call checks its callee register against: the record's word, which the binding
    /// makes the callee's identity (`BoundCalls::bind`) and which no callee holds while the record is unbound.
    /// A body's own callee has its identity as the plan's constant, so the frame reads none.
    fn call_identity(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            DirectSite::Owned(owned) => self.rebinds.get().then(|| self.bound_word(stage, owned, std::mem::offset_of!(BoundClosure, callee))),
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, callee)),
        }
    }

    /// The global object a proven sloppy callee binds, from the record's `global_this` (`BoundCalls::bind`).
    fn call_global(&mut self, stage: &mut Emit<'_, '_>) -> Option<ir::Value> {
        match self.call_site()?.callee? {
            DirectSite::Owned(_) => None,
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, global_this)),
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
        self.push_native(stage, callee, this, call.registers, room)?;
        let entry = stage.entry_target(crate::compiled::slow_paths::EngineEntry::CompiledEntry, crate::compiled::regions::flags(ENTRY_CELL), room.cell, field(std::mem::offset_of!(EntryCell, entry)));
        let entry = self.proven_entry(stage, callee, room.cell, entry);
        let signature = crate::compiled::lane_body_abi::body_shim_signature(types::I64, stage.builder.func.signature.call_conv);
        let signature = stage.builder.import_signature(signature);
        let shim = stage.entry_target(crate::compiled::slow_paths::EngineEntry::CompiledEntry, crate::compiled::regions::flags(ENTRY_CELL), call.context,
            field(std::mem::offset_of!(CallContext, head.body_shim)));
        let captures = match callee {
            DirectSite::Owned(owned) if self.rebinds.get() => self.bound_word(stage, owned, std::mem::offset_of!(BoundClosure, captures)),
            DirectSite::Owned(owned) => address(self.helpers, stage.builder, RelocKind::CaptureWords, owned.target.function, owned.capture_word()),
            other => self.callee_word(stage, other, std::mem::offset_of!(CalleeRecord, captures))?,
        };
        let (status, word) = stage.call_entry(signature, shim, &[call.context, this, captures, room.top, entry.word()]);
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
        let address = self.helpers.target(&mut stage.builder, which);
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
pub(crate) fn complete_activation(frame: &mut EmitFrame<'_>, stage: &mut Emit<'_, '_>, value: ir::Value) -> Option<()> {
    let Some(Site::Return(site)) = frame.site else { return None };
    site.complete(stage.builder, frame.payload, value);
    Some(())
}

impl<'a, 'f> Lowering<'a, 'f> {
    /// The callee of a self tail call (DESIGN.md 4.4): the returning call at `pc` of the function's own body, whose one owned
    /// callee is the function itself, is a tail site of the body (`Function::tail_site_permitted`), and whose callee plan is an
    /// ordinary body's (`CallPlan::tail_callee`). It passes a register-held closure, no more arguments than the parameters,
    /// and its receiver is the body's `this` or `undefined`. Anything else keeps the call's own lowering.
    pub(crate) fn self_tail_callee(&self, pc: usize, record: u32, site: &Invocation) -> Option<&'a DirectCallee> {
        let function = self.program.function_get(self.function_id as usize)?;
        let [callee] = self.direct.get(&pc)?.as_slice() else { return None };
        // SAFETY: the body owns its callees for as long as its code runs (`Lowering::call`).
        let callee: &'a DirectCallee = unsafe { &**callee };
        let admitted = self.completion == Completion::Body
            && matches!(self.coverage, Coverage::Prefix | Coverage::Full)
            && site.returns && !site.constructs() && !site.forwards
            && matches!(site.callee, quench_runtime::staged::forms::Callee::Register(_))
            && matches!(site.receiver, quench_runtime::staged::forms::Receiver::Undefined
                | quench_runtime::staged::forms::Receiver::This { path: None })
            && matches!(site.arguments, Arguments::Window { count, .. } if usize::from(count) <= self.params)
            && callee.target.function == self.function_id && callee.forwarder.is_none()
            && function.tail_site_permitted(record)
            && self.program.function_get(callee.target.function as usize).is_some_and(|body| body.call_plan(self.program).tail_callee)
            && self.vm.ahead.and_then(|ahead| ahead.calls.get(&pc)).is_none_or(Vec::is_empty);
        admitted.then_some(callee)
    }

    /// The self tail call `self_tail_callee` admits, lowered as a jump to the function's entry: the parameters take the
    /// arguments (read before any parameter is written, as the window overlaps them), the registers the entry holds live
    /// beyond the parameters are undefined as a fresh call's are, and the entry is entered as a back edge of the body, which
    /// polls as one does (`back_edge_check`). The body's own closure, captures and receiver are the entry's words, so the
    /// jump is taken only where the callee register holds the closure the owned callee names, the captures are the body's,
    /// and the receiver is the one the call passes; otherwise the guards leave at the call (`missed`), which makes the call.
    pub(crate) fn self_tail_call(&mut self, state: &mut State, pc: usize, missed: ir::Block, site: &Invocation,
        callee: &DirectCallee) -> bool {
        let (quench_runtime::staged::forms::Callee::Register(register), Arguments::Window { first, count }) = (site.callee, site.arguments) else {
            return false;
        };
        let entry = self.coverage.entry();
        let identity = self.tagged(state, register);
        let same_closure = self.raw_compare_imm(Relation::Equal, identity, callee.callee.to_bits() as i64);
        crate::compiled::emit_js_guard(&mut self.builder, same_closure, missed);
        let captures = self.reloc(RelocKind::CaptureWords, callee.target.function, callee.capture_word());
        let entry_captures = self.entry_words.1;
        let same_captures = self.raw_compare(Relation::Equal, entry_captures, captures);
        crate::compiled::emit_js_guard(&mut self.builder, same_captures, missed);
        if matches!(site.receiver, quench_runtime::staged::forms::Receiver::Undefined) {
            let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
            let entry_this = self.entry_words.0;
            let same_this = self.raw_compare(Relation::Equal, entry_this, undefined);
            crate::compiled::emit_js_guard(&mut self.builder, same_this, missed);
        }
        // The back edge polls before the parameters are written, so an interrupt leaves at the call with its state.
        self.backedge = true;
        self.count_trip();
        self.back_edge_check(state, pc, entry);
        let arguments: Vec<ir::Value> = (0..count).map(|index| self.tagged(state, first + index)).collect();
        let undefined = self.raw_const(Lane::I64, Value::UNDEFINED.to_bits() as i64);
        for index in 0..self.params {
            let value = arguments.get(index).copied().unwrap_or(undefined);
            self.set_tagged(state, index as Register, value);
        }
        for index in self.params..usize::from(self.unit.caller_registers) {
            if self.is_live(entry, index as Register) { self.set_tagged(state, index as Register, undefined); }
        }
        self.continue_at(state, entry);
        false
    }
}
