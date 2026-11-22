//! Capture reads and stores, the lane's lowering of the staged capture rows
//! (`Frame::capture_slot`, `Frame::store_capture`): dense slot loads and the VM's own lookups
//! where they miss. The lowering decides, per read, what its slow case reaches (`CaptureSite`);
//! the frame emits the access.
use super::*;

/// Whether a capture read may take the lane's own lookup for a root-held binding, and where it runs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RootedRead {
    No,
    /// In a loop body, where the helper call spills every live value: only once the code has left here.
    InBody,
    /// On a loop's entry edge, where nothing is live across the call.
    OnEntry,
}

/// Where the activation that reads keeps its dense capture slots, as one load reaches them.
#[derive(Clone, Copy)]
pub(crate) enum Dense {
    /// The slots (values, count) the running activation holds (`Entry::capture_values`), or a
    /// published callee's, fixed by the identity its call guards (`Lowering::frame_captures`), or
    /// the bound callee's frameless admission, as its frameless activation would read them
    /// (`Lowering::admitted_captures`).
    Held(ir::Value, ir::Value),
    /// No dense slots: the read takes its slow case.
    Absent,
}

/// What a capture row's emission needs of the lowering: the slots its fast case loads, the
/// closure its slow case projects, and whether the lane's own lookup may serve it.
#[derive(Clone, Copy)]
pub(crate) struct CaptureSite {
    pub(crate) dense: Dense,
    /// The closure an inlined frame's call bound, whose captures the VM projects
    /// (`LaneHelper::FunctionCapture`).
    pub(crate) callee: Option<ir::Value>,
    /// Whether a read past the dense slots may take the lane's own lookup (`LaneHelper::CaptureGet`):
    /// for a root-held binding, and for a capture past the activation's own scope.
    pub(crate) walks: bool,
}

impl CaptureSite {
    /// A store's site, or a read past the activation's scope: the lane's own lookup, nothing else.
    pub(crate) const OUTER: Self = Self { dense: Dense::Absent, callee: None, walks: true };
}

/// The emit stage's reach into the running body: its payload and helper table.
type Reach = (ir::Value, LaneHelpers);

/// The capture `slot`, `depth` scopes out, as the site reaches it: one load from its dense
/// slots where the slot holds an ordinary value, else the slow case. Anything the access cannot
/// serve leaves at `exit`.
pub(crate) fn read_capture(builder: &mut FunctionBuilder<'_>, reach: Reach, site: &CaptureSite,
    (depth, slot): (u16, u16), exit: ir::Block) -> ir::Value {
    let (slow, join) = (builder.create_block(), builder.create_block());
    builder.set_cold_block(slow);
    let dense = dense_slots(site);
    let value = builder.append_block_param(join, types::I64);
    match dense {
        Some((values, len)) => {
            let mut stage = Emit { builder: &mut *builder, property_values: None, gate: None , payload: None};
            let read = quench_runtime::staged::environment::dense_capture(&mut stage, values, len, slot, slow)
                .expect("the emit stage continues past its guards");
            emit(builder).goto(join, &[read]);
        }
        None => { emit(builder).goto(slow, &[]); }
    }
    builder.switch_to_block(slow);
    let read = capture_miss(builder, reach, site, (depth, slot), exit);
    emit(builder).goto(join, &[read]);
    builder.switch_to_block(join);
    value
}

/// The dense slots (values, length) the site names, `None` where it has none.
fn dense_slots(site: &CaptureSite) -> Option<(ir::Value, ir::Value)> {
    match site.dense {
        Dense::Held(values, len) => Some((values, len)),
        Dense::Absent => None,
    }
}

/// A read past the dense slots: the VM's projection of the closure an inlined frame's call
/// bound, else the lane's own lookup of the activation's slots where the site allows it; a miss
/// leaves at `exit`.
fn capture_miss(builder: &mut FunctionBuilder<'_>, reach: Reach, site: &CaptureSite, at: (u16, u16),
    exit: ir::Block) -> ir::Value {
    let Some(callee) = site.callee else {
        if site.walks { return capture_get(builder, reach, at, exit); }
        emit(builder).goto(exit, &[]);
        let dead = builder.create_block();
        builder.switch_to_block(dead);
        return emit(builder).raw_const(Lane::I64, 0);
    };
    let (payload, helpers) = reach;
    let vm = emit_js_payload_fixed_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.vm));
    let slot = emit(builder).raw_const(Lane::I64, i64::from(at.1));
    let helper = helpers.address(builder, LaneHelper::FunctionCapture);
    let read = emit_lane_call(builder, helper, &[vm, callee, slot]);
    let hit = emit(builder).raw_compare_imm(Relation::NotEqual, read, Value::DELETED.to_bits() as i64);
    emit_js_guard(builder, hit, exit);
    read
}

/// A capture's read by the lane's own lookup (`dispatch_fast::body_capture_in_lane`); a miss leaves.
fn capture_get(builder: &mut FunctionBuilder<'_>, reach: Reach, at: (u16, u16), exit: ir::Block) -> ir::Value {
    let (context, packed) = capture_operands(builder, reach.0, at);
    let value = call_helper(builder, reach, LaneHelper::CaptureGet, &[context, packed]);
    let hit = emit(builder).raw_compare_imm(Relation::NotEqual, value, Value::DELETED.to_bits() as i64);
    emit_js_guard(builder, hit, exit);
    value
}

/// A capture's store by the lane's own store (`dispatch_fast::body_capture_store_in_lane`); a binding
/// it does not take leaves at `exit`.
pub(crate) fn store_capture(builder: &mut FunctionBuilder<'_>, reach: Reach, at: (u16, u16), value: ir::Value,
    exit: ir::Block) {
    let (context, packed) = capture_operands(builder, reach.0, at);
    let stored = call_helper(builder, reach, LaneHelper::CaptureSet, &[context, packed, value]);
    let stored = emit(builder).raw_compare_imm(Relation::NotEqual, stored, 0);
    emit_js_guard(builder, stored, exit);
}

/// The lane context and the packed capture (depth in the high half, slot in the low) a capture
/// helper takes.
fn capture_operands(builder: &mut FunctionBuilder<'_>, payload: ir::Value, (depth, slot): (u16, u16)) -> (ir::Value, ir::Value) {
    let context = emit_js_payload_word(builder, payload, std::mem::offset_of!(JsStepDispatch, head.lane_context));
    let packed = emit(builder).raw_const(Lane::I64, i64::from(u32::from(depth) << u16::BITS | u32::from(slot)));
    (context, packed)
}

/// `helper`'s call by its address in the running image's table (`LaneHelpers::address`).
fn call_helper(builder: &mut FunctionBuilder<'_>, (payload, helpers): Reach, helper: LaneHelper,
    arguments: &[ir::Value]) -> ir::Value {
    let address = helpers.address(builder, helper);
    emit_lane_helper_call(builder, payload, address, arguments)
}

impl Lowering<'_, '_> {
    /// The site of the capture read at `pc`, `depth` scopes out. A depth-zero read is one load
    /// from the dense slots of the activation that reads it, the running one's
    /// (`Entry::capture_values`) or a published callee's, fixed by the identity its call guards
    /// (`Lowering::frame_captures`), where the slot holds an ordinary value; otherwise, in an
    /// inlined frame, the VM's own projection of the closure the call bound
    /// (`function_capture_read_in_lane`), which also resolves shared and statically bound callees'
    /// slots, else the lane's own read. Each read loads where it runs, after any call that may
    /// have stored to the slot.
    pub(crate) fn capture_site(&mut self, state: &mut State, pc: usize, depth: u16,
        admitted_callee: Option<ir::Value>, rooted: RootedRead) -> CaptureSite {
        let frame = match self.unit.origins.get(pc).copied() {
            Some(super::super::lane_unit::Origin::Inlined { frame, .. }) => Some(frame),
            _ => None,
        };
        let callee = admitted_callee.or_else(|| frame.map(|frame| self.tagged(state, self.unit.frames[frame].callee_register)));
        let dense = if depth == 0 { self.dense_slots(state, frame, callee) } else { Dense::Absent };
        CaptureSite { dense, callee, walks: depth != 0 || self.rooted_walk(rooted) }
    }

    /// Where the activation that runs `frame` keeps its dense slots: the running one's, a
    /// published callee's its call's guarded identity fixes, or else those of the bound `callee`'s
    /// frameless admission.
    fn dense_slots(&mut self, state: &mut State, frame: Option<usize>, callee: Option<ir::Value>) -> Dense {
        match frame {
            None => {
                let entry = self.entry.as_ref().expect("entry facts");
                Dense::Held(self.builder.use_var(entry.capture_values), self.builder.use_var(entry.capture_len))
            }
            Some(frame) if self.unit.frames[frame].target.identity().is_some() => {
                let (values, len) = self.frame_captures.get(frame).copied().unwrap_or((0, 0));
                Dense::Held(self.raw_const(Lane::I64, values as i64), self.raw_const(Lane::I64, len as i64))
            }
            Some(_) => callee.map_or(Dense::Absent, |callee| self.admitted_captures(state, callee)),
        }
    }

    /// The dense capture slots of `callee`'s frameless admission. The probe (`emit_admitted_captures`)
    /// is emitted once per callee on each path, where the state first needs it; a later read on that
    /// path takes its values, which that probe dominates, and a join keeps the view only where every
    /// arrival holds it.
    fn admitted_captures(&mut self, state: &mut State, callee: ir::Value) -> Dense {
        if let Some(&(_, (values, len))) = state.proofs.admitted_captures.iter().find(|(admitted, _)| *admitted == callee) {
            return Dense::Held(values, len);
        }
        let cache = super::super::emit_js_payload_word(&mut self.builder, self.payload,
            std::mem::offset_of!(super::super::JsStepDispatch, call_cache));
        let (values, len) = super::super::frameless::emit_admitted_captures(&mut self.builder, self.payload, callee, cache);
        state.proofs.note_admitted_captures(callee, (values, len));
        Dense::Held(values, len)
    }

    /// Whether a read the dense slots miss may take the lane's own lookup of a root-held
    /// binding. A loop's read leaves at one until the code has left here: the helper is a call
    /// every value live around the loop is spilled across (`left_here`).
    fn rooted_walk(&self, rooted: RootedRead) -> bool {
        let loop_code = matches!(self.coverage, Coverage::Loop { .. });
        rooted != RootedRead::No && self.completion.status_and_pc()
            && (self.left_here || !loop_code || rooted == RootedRead::OnEntry)
    }

    /// A loop's hoisted read of the capture `instruction` names, on the entry edge, with the
    /// callee its inlined frame was admitted for.
    pub(crate) fn load_capture(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, exit: ir::Block,
        admitted_callee: Option<ir::Value>, rooted: RootedRead) -> ir::Value {
        let at = (instruction.capture_depth(), instruction.capture_slot());
        let site = self.capture_site(state, pc, at.0, admitted_callee, rooted);
        let value = read_capture(&mut self.builder, (self.payload, self.helpers), &site, at, exit);
        self.frozen_capture(pc, value, exit)
    }

    /// The read-site guard of a frozen capture read at `pc`: compare the value the read
    /// loaded with the closed program's value, then use that constant. A binding changed
    /// elsewhere (or read before its declaration) fails the guard and leaves to the shared
    /// body, so the fold is sound without an invalidation word.
    fn frozen_capture(&mut self, pc: usize, value: ir::Value, exit: ir::Block) -> ir::Value {
        let Some(quench_runtime::vm::global_facts::GlobalRead::Frozen { value: expected, .. }) =
            self.global_reads.get(&pc).copied() else { return value };
        let same = self.is_identity(value, expected);
        emit_js_guard(&mut self.builder, same, exit);
        self.identity_constant(expected)
    }

    /// The root constant the capture read at `pc` takes (`FactTables::root_constant`), with its
    /// program, where the division folds Stable facts.
    pub(crate) fn root_constant_at(&self, pc: usize) -> Option<(quench_runtime::vm::program_store::ProgramId, Value)> {
        let found = (self.vm.online?.root_constant)(pc)?;
        self.division().folds(BindingTime::Stable).then_some(found)
    }

    /// The capture read at `pc` of a written-once root binding that `root_constant_at` folds: the
    /// binding's function, the result register's value, with no read of the environment. The
    /// function is held as a Stable fact (`hold_stable`), a dependency on the program's root
    /// constants (`StableFacts::root_program`), so the program's second root run retires the code.
    /// `None` where the value is no function.
    pub(crate) fn root_constant_read(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let (program, value) = self.root_constant_at(pc)?;
        let ty = Type::function(value)?;
        let exit = self.guard_exit(state, pc as u32);
        self.hold_stable(state, exit);
        self.stable.root_program = Some(program);
        let fact = RegisterFact::admit(Fact { ty, representation: Representation::Tagged, binding: BindingTime::Stable,
            proof: Proof::Dependency, projection: Projection::Exact }).expect("a Stable fact's dependency proves any type");
        let word = self.identity_constant(value);
        self.define(state, instruction.result_register(), word, fact);
        Some(true)
    }

    /// The capture row `instruction` decodes to, where this lowering makes it: a read at depth
    /// zero, or either at a caller's record, where the lane's own lookup serves it.
    fn capture_row(&self, pc: usize, instruction: WideInstruction) -> Option<quench_runtime::staged::Operation<ir::Value, EmitProof>> {
        use quench_runtime::staged::Operation;
        let row = Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY)?;
        let caller = self.completion.status_and_pc()
            && matches!(self.unit.origins.get(pc), Some(super::super::lane_unit::Origin::Caller(_)));
        match row {
            Operation::LoadCapture { depth: 0, .. } => Some(row),
            Operation::LoadCapture { .. } | Operation::StoreCapture { .. } => caller.then_some(row),
            _ => None,
        }
    }

    /// Whether `instruction` is a capture row this lowering makes.
    pub(crate) fn is_capture_row(&self, pc: usize, instruction: WideInstruction) -> bool {
        self.capture_row(pc, instruction).is_some()
    }

    /// The capture row at `pc`, stepped by its staged definition: a store retires every field
    /// proof, as another register may name what it wrote, and both leave at `pc` where the lane's
    /// access declines.
    pub(crate) fn capture_step(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let row = self.capture_row(pc, instruction).expect("a capture row decodes");
        let store = matches!(row, quench_runtime::staged::Operation::StoreCapture { .. });
        if store { state.proofs.fields.clear(); }
        let exit = self.guard_exit(state, pc as u32);
        let site = if store { CaptureSite::OUTER } else {
            self.capture_site(state, pc, instruction.capture_depth(), None, RootedRead::InBody)
        };
        let sites = frame::Sites { capture: Some(&site), ..frame::Sites::default() };
        self.step_with(state, frame::Edges::ValueGuarded { miss: exit }, row, StepWith { sites, ..StepWith::default() });
        if !store {
            let destination = instruction.result_register();
            let value = self.tagged(state, destination);
            let value = self.frozen_capture(pc, value, exit);
            self.set_tagged(state, destination, value);
        }
        true
    }
}
