//! A lane body's two halves: planning, which reads the VM (call targets, field proofs, direct
//! callees) and lowers the unit, and code generation, which reads nothing of it. The plan
//! keeps everything the method holds but its code, so code generation may run on the code
//! generator's thread (`jit_backend::run_in_background`) while the program runs on.

use super::*;
use std::collections::BTreeMap;

type LaneCode = Result<Vec<u8>, super::jit_backend::NativeCompileRejection>;

/// A lowered lane body: the method it becomes once its code exists.
pub(super) struct LanePlan {
    unit: lane_unit::Unit,
    lowered: lane_region::LoweredLane,
    fields: BTreeMap<u16, Vec<NativeFieldLoad>>,
    direct: Box<[DirectCallee]>,
    direct_sites: BTreeMap<usize, *const DirectCallee>,
    loop_header: Option<u32>,
    function: u32,
    /// The word its publication sets when one of its Stable facts changes.
    invalidation: crate::code_dependencies::Invalidation,
    /// The callee instructions its unit inlined at most (`lane_unit::with_budget`).
    budget: usize,
}

/// A plan whose code compiles on the code generator's thread. It holds while the facts it
/// took hold, under the lifecycle the published method has: `watch` subscribes to its slot
/// types and Stable facts from the plan on, a collection that frees a callee it names
/// retires it (`ProgramStore::note_pending_native_calls`), and so does a shape compaction
/// where it names shapes (`retire_named_shapes`). A collection alone leaves it valid: an
/// allocation-heavy loop collects more often than a body compiles.
pub(in crate::vm) struct PendingLaneMethod {
    plan: LanePlan,
    /// The leaves it was planned from (`LaneBodyWork::Start`), which a plan again reuses.
    generic: Vec<LaneExit>,
    watch: Rc<crate::code_dependencies::PublishedCode<()>>,
    code: super::jit_backend::Pending<LaneCode>,
}

/// What a pending body's compile came to.
enum LaneCompile {
    /// Its code needed more memory than a body may take: plan again from its feedback,
    /// inlining half its budget.
    Replan(Vec<LaneExit>, usize),
    Done(Option<LaneMethod>),
}

/// What a function's lane body asks of the VM at an entry past its warm-up.
pub(in crate::vm) enum LaneBodyWork<'a> {
    /// Plan a body from a predecessor's leaves (`generic`) and start compiling its code.
    Start { generic: &'a [LaneExit] },
    /// Look for the code of the body compiling.
    Poll(Box<PendingLaneMethod>),
}

/// What the VM did for it.
pub(in crate::vm) enum LaneBodyOutcome {
    Compiling(Box<PendingLaneMethod>),
    Published(Rc<PublishedLaneMethod>),
    /// A collection ran while its code compiled: its proofs no longer hold, and it warms again.
    Stale,
    Declined,
}

/// Lane bodies with at least this many IR instructions allocate registers in one pass; smaller
/// ones keep the backtracking allocator. One pass pays off where compile latency dominates (large
/// bodies), backtracking where a small hot loop runs millions of times. Experiment build, 3
/// interleaved rounds (2026-11-07): one pass everywhere cost call-method 2.2 -> 5.2 ns and
/// call-polymorphic 8.6 -> 15.9 ns (bodies of 400-1900 instructions); at 2048 those recover
/// (2.21, 8.63 ns) while richards (bodies 2k-17k) keeps 28.5 vs 26.9 all-backtracking and
/// delta-blue 48.0 vs 43.9.
const LANE_SINGLE_PASS_MIN_INSTS: usize = 2048;
/// A replanned body inlines with this fraction of the budget whose compile failed.
const REPLAN_BUDGET_DIVISOR: usize = 2;

/// The code of one lane body, compiled on the calling thread.
fn lane_code(function: ir::Function) -> LaneCode {
    let isa = if function.dfg.num_insts() >= LANE_SINGLE_PASS_MIN_INSTS {
        super::jit_backend::lane_body_isa()
    } else {
        super::jit_backend::target_isa()
    }
    .ok_or(super::jit_backend::NativeCompileRejection::Publish)?;
    compile_native_function(function, &*isa, lane_region::LANE_BODY_MAX_CODE_BYTES, |code| Some(code.to_vec()))
}

/// The unit PCs of a previous body's leaves: the specialization's feedback.
fn unit_leaves(unit: &lane_unit::Unit, generic: &[LaneExit]) -> Vec<usize> {
    generic.iter().filter_map(|exit| unit.origins.iter().position(|origin| match *exit {
        LaneExit::At(pc) => *origin == lane_unit::Origin::Caller(pc as u32),
        LaneExit::Inlined { frame, pc } => *origin == lane_unit::Origin::Inlined { frame, pc },
        LaneExit::Callee | LaneExit::Returned { .. } | LaneExit::Terminated => false,
    })).collect()
}

/// The calls a unit still makes (none it inlines) whose callees have bodies. Each candidate
/// owns one contiguous stable slice, which the code names by address.
fn unit_direct_calls(unit: &lane_unit::Unit, direct: &[Option<DirectCallee>])
    -> (Box<[DirectCallee]>, BTreeMap<usize, *const DirectCallee>) {
    let (pcs, direct): (Vec<usize>, Vec<DirectCallee>) = unit.instructions.iter().enumerate()
        .filter(|(_, instruction)| instruction.op() == Op::Call)
        .filter_map(|(pc, _)| match unit.origins[pc] {
            lane_unit::Origin::Caller(origin) => direct.get(origin as usize)?.as_ref().cloned().map(|callee| (pc, callee)),
            lane_unit::Origin::Inlined { .. } => None,
        })
        .unzip();
    let direct: Box<[DirectCallee]> = direct.into_boxed_slice();
    let sites = pcs.iter().zip(direct.iter()).map(|(pc, callee)| (*pc, std::ptr::from_ref(callee))).collect();
    (direct, sites)
}

/// Whether a function is pure numeric, observing neither closure environment nor identity.
fn numeric_callee(program: &ResidualProgram, function: u32) -> bool {
    program.function_get(function as usize).is_some_and(|body| body.plain_activation(program))
        && numeric_restartable(program, function)
}

impl<H: Host> Vm<H> {
    /// The call targets a unit binds at `caller`'s `pc`. A call the body left at takes no
    /// inlined callee: its call binds what the inlined frame could not. A pure numeric callee
    /// binds its definition at the edge, as AOT does, so fresh closures of it are one case;
    /// one that is not this realm's own definition makes the unit's bindings invalid.
    fn lane_bindings(&self, program: &ResidualProgram, function: u32, generic: &[LaneExit],
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, valid: &std::cell::Cell<bool>, caller: u32, pc: u32) -> Vec<CallTarget> {
        if caller == function && generic.contains(&LaneExit::At(pc as usize)) { return Vec::new(); }
        let mut bound = targets(caller, pc);
        for target in &mut bound {
            // A forwarded target keeps its identity: the site calls it through its native.
            if !numeric_callee(program, target.function) || target.through().is_some() { continue; }
            if let Some(callee) = target.identity()
                && !self.same_realm_user_target(callee).is_some_and(|(owner, function, _)|
                    owner == self.active_program && function == target.function) {
                valid.set(false);
            }
            target.callee = CalleeBinding::Function;
        }
        each_once(bound)
    }

    /// The unit of `function` inlining at most `budget` callee instructions, if each frame
    /// it inlines is admitted.
    fn lane_plan_unit(&mut self, program: &ResidualProgram, function: u32, targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
        generic: &[LaneExit], budget: usize) -> Option<lane_unit::Unit> {
        let body = program.function_get(function as usize)?;
        let valid = std::cell::Cell::new(true);
        let unit = lane_unit::with_budget(program, function, body, budget,
            &|caller, pc| self.lane_bindings(program, function, generic, targets, &valid, caller, pc),
            &|callee| program.function_get(callee as usize))?;
        if !valid.get() { return None; }
        unit.frames.iter().all(|frame| match frame.target.callee {
            CalleeBinding::Identity(callee) | CalleeBinding::Forwarded { callee, .. } => self.lane_inlined_target_admitted(program, callee, frame.target.function),
            CalleeBinding::Function => frame.recovery == lane_unit::CallRecovery::RestartCall
                && numeric_restartable(program, frame.target.function),
        }).then_some(unit)
    }

    /// Plans `function`'s lane body at `budget`: its unit, proofs and lowering, and the
    /// function the code generator compiles. `None` where it does not lower or runs nothing
    /// natively.
    #[allow(clippy::too_many_arguments)]
    fn plan_lane_method(&mut self, program: &ResidualProgram, function: u32, targets: &dyn Fn(u32, u32) -> Vec<CallTarget>,
        captures: &dyn Fn(Value) -> (usize, usize), direct: &[Option<DirectCallee>], loop_header: Option<u32>,
        generic: &[LaneExit], budget: usize) -> Option<(LanePlan, ir::Function)> {
        let unit = self.lane_plan_unit(program, function, targets, generic, budget)?;
        let loop_entry = match loop_header {
            Some(header) => Some(unit.origins.iter().position(|origin| *origin == lane_unit::Origin::Caller(header))?),
            None => None,
        };
        let left_at = unit_leaves(&unit, generic);
        let fields = self.lane_unit_fields(program, &unit, &left_at);
        let global_reads = self.lane_global_reads(program, function, &unit);
        let (direct, direct_sites) = unit_direct_calls(&unit, direct);
        let frame_captures: Vec<(usize, usize)> = unit.frames.iter()
            .map(|frame| frame.target.identity().map_or((0, 0), captures)).collect();
        let allow_first_entry = loop_header.is_none() || self.programs.js_jit_lane_methods_enabled();
        let helpers = LaneHelper::addresses::<H>();
        crate::jit_coverage::note_native_helpers(helpers);
        let helpers = LaneHelpers::Process(helpers);
        // The body folds Stable facts under its own invalidation word, which its publication
        // sets when one changes (`ProgramStore::publish_lane_method`).
        let invalidation = crate::code_dependencies::Invalidation::default();
        let method_cache_atoms = self.method_cache_atoms.bits().clone();
        let active = self.active_program;
        let constants = self.programs.residual(active).filter(|running| std::ptr::eq(*running, program))
            .map_or(&[][..], |_| self.programs.constants(active));
        let facts = VmFacts { online: Some(OnlineFacts { method_cache_atoms: &method_cache_atoms,
            invalidation: invalidation.address(), constants }), ..self.vm_facts() };
        let mut lowered = lane_region::lower_body(program, function, &unit, helpers, &fields, &global_reads, facts,
            &left_at, &direct_sites, &frame_captures, loop_entry, allow_first_entry)?;
        if lowered.region.native == 0 { return None; }
        let native = std::mem::replace(&mut lowered.region.function, ir::Function::new());
        Some((LanePlan { unit, lowered, fields, direct, direct_sites, loop_header, function, invalidation, budget }, native))
    }

    /// Specialize `function` for lane activations, admitting a body only where its first
    /// instruction runs natively: an entry that leaves at once is pure cost. Code that needs
    /// more memory than a body may take is planned again inlining less.
    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(in crate::vm) fn compile_lane_method(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: Vec<Option<DirectCallee>>, loop_header: Option<u32>, generic: &[LaneExit]) -> Option<LaneMethod> {
        self.host.before_native_compilation();
        let mut budget = lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS;
        let method = loop {
            let Some((plan, native)) = self.plan_lane_method(program, function, targets, captures, &direct, loop_header, generic, budget)
            else { break None };
            match lane_code(native) {
                Ok(code) => break plan.finish(&code),
                Err(reason) if plan.replans(reason) => budget /= 2,
                Err(_) => break None,
            }
        };
        self.host.after_native_compilation();
        method
    }

    /// Plans `function`'s lane body at `budget` and starts compiling its code on the code
    /// generator's thread (`PendingLaneMethod`).
    #[allow(clippy::too_many_arguments)]
    fn start_lane_method(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: &[Option<DirectCallee>], loop_header: Option<u32>, generic: &[LaneExit], budget: usize) -> Option<PendingLaneMethod> {
        self.host.before_native_compilation();
        let planned = self.plan_lane_method(program, function, targets, captures, direct, loop_header, generic, budget);
        self.host.after_native_compilation();
        let (plan, native) = planned?;
        let watch = Rc::new(crate::code_dependencies::PublishedCode::new(()));
        let watcher: Rc<dyn crate::code_dependencies::DependentCode> = watch.clone();
        if !subscribe_lane_dependencies(&plan.slot_dependencies(), &plan.lowered.region.stable,
            &watcher, &mut self.heap, self.method_cache_atoms.readers()) {
            return None;
        }
        let code = super::jit_backend::run_in_background(move || lane_code(native));
        self.programs.note_pending_native_calls(&watch, plan.callees());
        Some(PendingLaneMethod { plan, generic: generic.to_vec(), watch, code })
    }

    /// Does `work` for `function`'s lane body (`LaneBodyWork`), whose direct callees
    /// `direct` gives; publishes a body whose code compiled while its proofs held.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::vm) fn lane_body_work(&mut self, program: &ResidualProgram, function: u32,
        targets: &dyn Fn(u32, u32) -> Vec<CallTarget>, captures: &dyn Fn(Value) -> (usize, usize),
        direct: &dyn Fn(&Self) -> Vec<Option<DirectCallee>>, loop_header: Option<u32>, work: LaneBodyWork<'_>) -> LaneBodyOutcome {
        let mut pending = match work {
            LaneBodyWork::Start { generic } => (generic.to_vec(), lane_unit::MAX_UNIT_INLINED_INSTRUCTIONS),
            LaneBodyWork::Poll(pending) => match self.lane_body_poll(*pending) {
                Ok(outcome) => return outcome,
                Err(replan) => replan,
            },
        };
        // A plan again after its code was too large starts at once, from the background
        // thread's result as from the calling thread's; code the thread compiled at once (or
        // the calling thread, without one) publishes at once.
        loop {
            let (generic, budget) = pending;
            let direct = direct(self);
            let Some(started) = self.start_lane_method(program, function, targets, captures, &direct, loop_header, &generic, budget)
            else { return LaneBodyOutcome::Declined };
            match self.lane_body_poll(started) {
                Ok(outcome) => return outcome,
                Err(replan) => pending = replan,
            }
        }
    }

    /// What a pending body's compile came to, or the feedback and budget to plan it again with
    /// where its code needed more memory than a body may take: half the inlining.
    fn lane_body_poll(&mut self, pending: PendingLaneMethod) -> Result<LaneBodyOutcome, (Vec<LaneExit>, usize)> {
        if !pending.watch.is_published() || pending.plan.direct.iter().any(|callee| !callee.method.is_published()) {
            return Ok(LaneBodyOutcome::Stale);
        }
        Ok(match pending.poll() {
            Err(pending) => LaneBodyOutcome::Compiling(pending),
            Ok(LaneCompile::Replan(generic, budget)) => return Err((generic, budget / REPLAN_BUDGET_DIVISOR)),
            Ok(LaneCompile::Done(method)) => method
                .and_then(|method| self.programs.publish_lane_method(method, &mut self.heap, self.method_cache_atoms.readers()))
                .map_or(LaneBodyOutcome::Declined, LaneBodyOutcome::Published),
        })
    }
}

/// Whether a callee bound by its definition restarts its call on a leave, as a pure numeric
/// function does.
fn numeric_restartable(program: &ResidualProgram, function: u32) -> bool {
    program.function_get(function as usize).is_some_and(|body|
        lane_region::restartable_numeric_function(program, body, &body.instructions().collect::<Vec<_>>()))
}

impl PendingLaneMethod {
    /// Makes it stale where its code names shape ids, which a compaction renumbers.
    pub(in crate::vm) fn retire_named_shapes(&self) {
        if self.plan.names_shapes() { self.watch.retire(); }
    }

    /// What its compile came to, or itself while its code compiles.
    fn poll(mut self) -> Result<LaneCompile, Box<Self>> {
        Ok(match self.code.poll() {
            std::task::Poll::Pending => return Err(Box::new(self)),
            std::task::Poll::Ready(Some(Ok(code))) => LaneCompile::Done(self.plan.finish(&code)),
            std::task::Poll::Ready(Some(Err(reason))) if self.plan.replans(reason) =>
                LaneCompile::Replan(std::mem::take(&mut self.generic), self.plan.budget),
            std::task::Poll::Ready(_) => LaneCompile::Done(None),
        })
    }

}

impl LanePlan {
    /// Whether its code names shape ids: its field sites', or its direct callees' bodies'.
    fn names_shapes(&self) -> bool {
        !self.fields.is_empty() || !self.direct.is_empty()
    }

    /// The callees its code names by identity: inlined frames' and direct calls'.
    fn callees(&self) -> Box<[Value]> {
        self.unit.frames.iter().filter_map(|frame| frame.target.identity())
            .chain(self.direct.iter().map(|callee| callee.callee)).collect()
    }

    /// Whether a rejection of its code is one planning again with less inlined addresses.
    fn replans(&self, reason: super::jit_backend::NativeCompileRejection) -> bool {
        reason.exceeds_code_budget() && !self.unit.frames.is_empty() && self.budget > 0
    }

    /// The slot types its code relies on, its direct callees' included.
    fn slot_dependencies(&self) -> Vec<crate::code_dependencies::SlotDependency> {
        lane_slot_dependencies(&self.fields, &self.direct)
    }

    /// The method its compiled `code` makes, published to executable memory. The method
    /// takes its direct callees whole: the code names them by address.
    fn finish(mut self, code: &[u8]) -> Option<LaneMethod> {
        let stable = std::mem::take(&mut self.lowered.region.stable);
        let unit = &self.unit;
        let loops = unit.instructions.iter().enumerate().any(|(pc, instruction)|
            instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget && (instruction.imm() as usize) <= pc);
        let amortizes = loops || !unit.frames.is_empty() || unit.instructions.len() >= TIERS.lane_body.min_instructions;
        let closed = unit.instructions.iter().enumerate()
            .all(|(pc, instruction)| instruction.op() != Op::Call || self.direct_sites.contains_key(&pc));
        let (slot_dependencies, recovery_space) = (self.slot_dependencies(), super::dispatch_fast::LaneRecoverySpace::inlined(&unit.frames));
        let (scratch_width, caller_registers) = (usize::from(unit.registers - unit.caller_registers), unit.caller_registers);
        let names_shapes = self.names_shapes();
        let memory = CodeMemory::new(code)?;
        if let (Some(directory), Some(clif)) = (native_dump_directory(), self.lowered.clif.as_deref()) {
            dump_native_code(directory, self.function, &memory, clif);
        }
        // SAFETY: the lane region lowers the shared native-entry ABI.
        let entry = unsafe { std::mem::transmute::<*mut u8, Entry>(memory.pointer()) };
        #[cfg(test)]
        LANE_METHODS_COMPILED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        #[cfg(test)]
        LANE_CALLS_INLINED.fetch_add(unit.frames.len(), std::sync::atomic::Ordering::Relaxed);
        Some(LaneMethod {
            slot_dependencies, stable, invalidation: self.invalidation, _code: memory, entry, first_entry: self.lowered.first_entry, recovery_space,
            fault_poll: self.lowered.region.fault_poll || self.direct.iter().any(|callee| callee.body.fault_poll),
            // A body that names a callee's body retires with it (`DirectCallee::method`).
            names_shapes, inlined: self.unit.frames,
            scratch_width, caller_registers,
            loop_header: self.loop_header, amortizes, direct: self.direct, closed,
        })
    }
}

impl LaneMethod {
    /// A leave as the next body's feedback (`LaneLeaves`): one at an inlined call whose
    /// callee was none of the alternatives it inlined is the leave of the caller's call that
    /// inlined its frame, which the next body then makes itself (`Vm::lane_bindings`), so its
    /// callees run in their own bodies instead of the whole caller's remainder in the lane.
    pub(in crate::vm) fn feedback(&self, exit: LaneExit) -> LaneExit {
        let LaneExit::Inlined { frame, pc } = exit else { return exit };
        if !self.inlined.iter().any(|call| call.parent == Some(frame) && call.call == pc) {
            return exit;
        }
        let mut root = &self.inlined[frame];
        while let Some(parent) = root.parent { root = &self.inlined[parent]; }
        LaneExit::At(root.call as usize)
    }
}

/// `targets`, each once, in first-seen order.
fn each_once(mut targets: Vec<CallTarget>) -> Vec<CallTarget> {
    let mut kept = 0;
    for index in 0..targets.len() {
        let target = targets[index];
        if !targets[..kept].contains(&target) { targets[kept] = target; kept += 1; }
    }
    targets.truncate(kept);
    targets
}
