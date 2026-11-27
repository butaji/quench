//! Receivers kept as their field values: a construction by a plan of fields passed or constant
//! (`compiled::ScalarConstructPlan`) whose result only has its fields read, as in
//! `const p = new Pair(i, 1); s += p.x + p.y`, allocates nothing. The body tests that the plan
//! holds where the construction would run, keeps each argument in a variable, and answers a
//! read of a field with its value. The receiver is allocated, by the same plan, only where the
//! code needs it as an object: before an instruction the kept values cannot answer, before a
//! block ends with the register live, and in every leave that finds it live, so the
//! interpreter resumes with the object the construction would have made. Between the
//! construction and its last use no instruction allocates or calls (`keeps_virtuals`), so the
//! plan that held at the construction still holds where the receiver is allocated, and no
//! collection can run while only these variables name the arguments.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::{PlanCallee, ScalarConstructPlan, ScalarField};

/// A receiver kept as its field values.
#[derive(Clone, PartialEq)]
pub(crate) struct VirtualObject {
    /// The register the construction defined.
    pub(crate) register: Register,
    /// The plan's callee, which the allocation at a leave constructs again.
    callee: PlanCallee,
    /// Per field in slot order: its name, and the variable holding its tagged value (an
    /// argument's, or a constant's) with the type the value had.
    fields: smallvec::SmallVec<[(Atom, Option<Variable>, Value, Type); 4]>,
    /// The arguments as passed, which the allocation passes the plan again.
    arguments: smallvec::SmallVec<[Variable; 4]>,
}

/// The most arguments a kept construction passes.
const MAX_ARGUMENTS: u16 = 8;

/// What the arrivals at a forward-only join (`Lowering::sealable`) carried of the receivers kept
/// into it (`carry_virtuals`): how many arrived, the join's own receivers, with variables of
/// their own for their arguments, and whether an arrival carried other receivers than the first.
#[derive(Default)]
pub(crate) struct JoinedVirtuals {
    arrivals: usize,
    objects: smallvec::SmallVec<[VirtualObject; 1]>,
    mismatch: bool,
}

/// Whether `live` carries the same receivers as `template`: each register, callee, argument
/// count, and per field the name, constant, type and whether it is an argument's.
fn same_receivers(template: &[VirtualObject], live: &[VirtualObject]) -> bool {
    template.len() == live.len() && template.iter().zip(live).all(|(kept, other)| kept.register == other.register
        && kept.callee == other.callee && kept.arguments.len() == other.arguments.len()
        && kept.fields.len() == other.fields.len()
        && kept.fields.iter().zip(&other.fields).all(|(field, theirs)| field.0 == theirs.0 && field.1.is_some() == theirs.1.is_some()
            && field.2 == theirs.2 && field.3 == theirs.3))
}

/// A loop `header..=end` whose entering edge tests, once, what each kept construction of
/// `callee` in it would test (`LaneHelper::ConstructPlanHolds`): `variable` holds the answer
/// there, 1 where it held.
///
/// The invariant: where `variable` holds 1 inside the loop, the test held on the path that led
/// here and only quiet instructions ran since (`keeps_constructions_ready`): moves, constants,
/// jumps, Number arithmetic and comparisons, reads of kept receivers and kept constructions,
/// none of which allocates, calls, stores or constructs. Each condition the test reads is then
/// unchanged: the frame's permission to construct and `construct_target` change only by a
/// call, a due collection only by an allocation, and the plan only by a store or a layout
/// change (`watch_construct_plan`). Every other instruction, and every allocation of a kept
/// receiver (`materialize_virtuals`), defines `variable` as 0 before its code, so the next
/// construction tests again at its site. Polls and failed guards leave. The loop is entered
/// only at its header (`enters_only_at_header`), each edge testing again (`hoist_entry`), and
/// a construction outside every such loop tests at its site as before; a path that defined no
/// value reads 0 (Cranelift's undefined variable). No code depends on the plan, so a loop
/// region with static facts needs no Stable division.
#[derive(Clone, Copy)]
pub(crate) struct ConstructReady {
    callee: PlanCallee,
    variable: Variable,
    header: usize,
    end: usize,
}

impl Lowering<'_, '_> {
    /// The word of a plan's callee: the closure's identity, which the code's roots keep, or the
    /// symbol of the image's closure, which the loader resolves and roots (`aot_image::live`).
    fn plan_callee_word(&mut self, callee: PlanCallee) -> ir::Value {
        match callee {
            PlanCallee::Live(callee) => self.identity_constant(callee),
            PlanCallee::Image(key) => self.reloc(RelocKind::HeapValue, key, Value::UNDEFINED.to_bits()),
        }
    }

    /// The plan the construction at `pc` would build its receiver by, where the body may keep
    /// that receiver as its field values: a plain construction of registers whose site named one
    /// callee with such a plan (`OnlineFacts::construct_plans`).
    pub(crate) fn virtual_construct_plan(&self, pc: usize, instruction: WideInstruction) -> Option<ScalarConstructPlan> {
        if self.completion.numeric() || instruction.result_flags() != 0 || !self.completion.status_and_pc() { return None; }
        if !self.is_construction(instruction) { return None; }
        let quench_runtime::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return None };
        if window.count > MAX_ARGUMENTS { return None; }
        let plans = self.vm.construct_plans?;
        let (function, at) = match self.unit.origins.get(pc).copied()? {
            super::super::lane_unit::Origin::Caller(record) => (self.function_id, record),
            super::super::lane_unit::Origin::Inlined { frame, pc } => (self.unit.frames[frame].target.function, pc),
        };
        let plan = plans(function, at)?;
        // The register must be read as an object by nothing before its first field read: a
        // construction whose result is live into its own next instruction as anything else
        // allocates there anyway.
        // A construction that ends its block (`inner = new P(..)` as a branch's last
        // instruction) would carry the kept receiver into a join, where no state keeps it.
        if self.blocks.get(pc + 1).is_some_and(Option::is_some) { return None; }
        let result = instruction.result_register();
        (result < self.unit.registers && self.is_live(pc + 1, result)).then_some(plan)
    }

    /// The construction at `pc` by `plan`, kept as its field values: the callee is the plan's
    /// and the plan holds, else it leaves before any effect; the result register holds no
    /// object until one is needed (`materialize_virtuals`).
    pub(crate) fn virtual_construct(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, plan: ScalarConstructPlan) -> bool {
        let quench_runtime::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return false };
        let exit = self.guard_exit(state, pc as u32);
        let callee = self.tagged(state, instruction.register_b());
        let expected = self.plan_callee_word(plan.callee);
        let same = self.raw_compare(Relation::Equal, callee, expected);
        emit_js_guard(&mut self.builder, same, exit);
        let ready = self.construct_ready.iter()
            .find(|ready| ready.callee == plan.callee && (ready.header..=ready.end).contains(&pc))
            .map(|ready| ready.variable);
        let holds = match ready {
            // Tested on the loop's entering edge, or at an earlier construction, with only quiet
            // instructions since (`ConstructReady`): the test runs here only where it was
            // forgotten or refused, and its holding answer is remembered for the next one.
            Some(variable) => {
                let (slow, join) = (self.builder.create_block(), self.builder.create_block());
                self.builder.set_cold_block(slow);
                let holds = self.builder.append_block_param(join, types::I64);
                let flag = self.builder.use_var(variable);
                let one = self.raw_const(Lane::I64, 1);
                self.branch_if(flag, (join, &[one]), (slow, &[]));
                self.builder.switch_to_block(slow);
                let payload = self.payload;
                let tested = self.helper_call(LaneHelper::ConstructPlanHolds, &[payload, expected]);
                self.goto(join, &[tested]);
                self.builder.switch_to_block(join);
                // Past the guard below the test held, and this construction allocates nothing.
                self.builder.def_var(variable, one);
                holds
            }
            None => {
                let payload = self.payload;
                self.helper_call(LaneHelper::ConstructPlanHolds, &[payload, expected])
            }
        };
        let held = self.raw_compare_imm(Relation::NotEqual, holds, 0);
        emit_js_guard(&mut self.builder, held, exit);
        let mut arguments = smallvec::SmallVec::new();
        let mut types = smallvec::SmallVec::<[Type; 8]>::new();
        for register in window.base..window.base + window.count {
            let bits = self.tagged(state, register);
            let variable = self.builder.declare_var(types::I64);
            self.builder.def_var(variable, bits);
            arguments.push(variable);
            types.push(state.values[usize::from(register)].value.ty);
        }
        let fields = plan.fields.iter().map(|(atom, field)| match *field {
            ScalarField::Parameter(parameter) if parameter < window.count =>
                (*atom, Some(arguments[usize::from(parameter)]), Value::UNDEFINED, types[usize::from(parameter)]),
            ScalarField::Parameter(_) => (*atom, None, Value::UNDEFINED, Type::immediate(Value::UNDEFINED)),
            ScalarField::Constant(value) => (*atom, None, value, Type::immediate(value)),
        }).collect();
        let result = instruction.result_register();
        // Nothing reads this word as the object: every reader takes the kept values or
        // allocates first. A Number's bits, so a frame slot written from it holds no reference.
        let placeholder = self.raw_const(Lane::I64, 0);
        self.define(state, result, placeholder, RegisterFact::definition(Type::ANY, Representation::Tagged));
        state.virtuals.retain(|kept| kept.register != result);
        state.virtuals.push(VirtualObject { register: result, callee: plan.callee, fields, arguments });
        true
    }

    /// A plain read of a field of a kept receiver: the field's kept value.
    pub(crate) fn virtual_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        if instruction.result_flags() != 0 || instruction.writes_current_this() { return None; }
        let base = self.field_base(pc, instruction)?.register_index()?;
        let kept = state.virtuals.iter().find(|kept| kept.register == base)?;
        let (parts, sink) = self.field_chain(instruction)?;
        let [(atom, _)] = parts[..] else { return None };
        if sink.is_some() { return None; }
        let (_, variable, constant, ty) = *kept.fields.iter().find(|(name, ..)| *name == atom)?;
        let value = match variable {
            Some(variable) => self.builder.use_var(variable),
            None => self.raw_const(Lane::I64, constant.to_bits() as i64),
        };
        let result = instruction.result_register();
        self.define(state, result, value, RegisterFact::definition(ty, Representation::Tagged));
        // A read into the receiver's own register, as `p = p.y`, ends the receiver there.
        state.virtuals.retain(|kept| kept.register != result);
        Some(true)
    }

    /// Drops each kept receiver whose register the instruction, which reads none of them as an
    /// object, writes: the register names another value from there on.
    pub(crate) fn drop_overwritten_virtuals(&self, state: &mut State, instruction: WideInstruction) {
        let (_, definitions) = quench_runtime::compile::liveness::transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
        state.virtuals.retain(|kept| !definitions.contains(usize::from(kept.register)));
    }

    /// Whether the instruction at `pc` leaves every kept receiver kept: it reads none of them
    /// but as a field the kept values answer, and it neither allocates nor calls nor ends the
    /// block with one live.
    pub(crate) fn keeps_virtuals(&self, state: &State, pc: usize, instruction: WideInstruction) -> bool {
        let (uses, _) = quench_runtime::compile::liveness::transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
        let virtual_use = state.virtuals.iter().any(|kept| uses.contains(usize::from(kept.register)));
        let ends_block = self.blocks.get(pc + 1).is_some_and(Option::is_some)
            || !matches!(instruction.op().control_flow_layout(), quench_runtime::bytecode::ControlFlowLayout::Fallthrough)
            || instruction.returns_from_frame();
        if ends_block && state.virtuals.iter().any(|kept| self.is_live(pc + 1, kept.register)) && !self.edges_carry(pc, instruction) { return false; }
        if virtual_use {
            return crate::compiled::meaning_compiled::is_get_field(instruction) && self.field_base(pc, instruction)
                .and_then(|base| base.register_index())
                .is_some_and(|base| state.virtuals.iter().any(|kept| kept.register == base && self.virtual_reads(instruction, kept)));
        }
        // A jump whose every edge enters a forward-only join carries the receivers into it (`carry_kept`).
        if ends_block && quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Jump) && self.edges_carry(pc, instruction) {
            return true;
        }
        let op = instruction.op();
        let numbers = || (0..state.values.len()).filter(|register| uses.contains(*register))
            .all(|register| state.values[register].value.ty.is_subtype(Type::NUMBER));
        op.copies_register()
            || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst)
            || (quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericUnary)) && numbers()
    }

    /// Whether `kept`'s values answer the read `instruction`: one plain read of a field it keeps.
    fn virtual_reads(&self, instruction: WideInstruction, kept: &VirtualObject) -> bool {
        instruction.result_flags() == 0 && !instruction.writes_current_this()
            && self.field_chain(instruction).is_some_and(|(parts, sink)| sink.is_none()
                && matches!(parts[..], [(atom, _)] if kept.fields.iter().any(|(name, ..)| *name == atom)))
    }

    /// Whether every edge of the block-ending instruction at `pc` enters a forward-only join lowered
    /// after it, which takes the kept receivers into its own variables (`carry_kept`).
    pub(crate) fn edges_carry(&self, pc: usize, instruction: WideInstruction) -> bool {
        let mut any = false;
        let mut all = true;
        quench_runtime::bytecode::for_each_instruction_successor(pc, instruction, |target| {
            any = true;
            all &= target > pc && self.coverage.lowered(target) && self.sealable.contains(target)
                && self.blocks.get(target).is_some_and(Option::is_some);
        });
        any && all
    }

    /// The kept receivers at block leader `pc`, once its entry conforms: a forward-only join lowered
    /// after this edge carries those live there (`carry_virtuals`); any other leader takes none, so a
    /// live one declines the region. The state keeps none after.
    pub(crate) fn carry_kept(&mut self, state: &mut State, pc: usize) {
        if self.sealable.contains(pc) && self.at < pc {
            if !self.carry_virtuals(state, pc) { self.valid = false; declined("a kept receiver reaches a join unevenly", pc); }
        } else {
            self.decline_kept(state, pc);
        }
        state.virtuals.clear();
    }

    /// A kept receiver live at `pc`, reaching an edge that no join carries, declines the region.
    pub(crate) fn decline_kept(&mut self, state: &mut State, pc: usize) {
        if state.virtuals.iter().any(|kept| self.is_live(pc, kept.register)) { self.valid = false; declined("a kept receiver reaches an edge", pc); }
        state.virtuals.clear();
    }

    /// An arrival at the forward-only join `pc` carries the kept receivers live there: the first arrival
    /// declares the join's variables for their arguments, and each arrival defines them from its own.
    /// An arrival carrying other receivers than the first marks the join mismatched (`join_virtuals`),
    /// and one carrying receivers then declines; false where it must decline.
    fn carry_virtuals(&mut self, state: &State, pc: usize) -> bool {
        let live: Vec<VirtualObject> = state.virtuals.iter().filter(|kept| self.is_live(pc, kept.register)).cloned().collect();
        let mut joined = self.joined_virtuals.remove(&pc).unwrap_or_default();
        joined.arrivals += 1;
        if joined.arrivals == 1 {
            match self.join_template(&live) {
                Some(objects) => joined.objects = objects,
                None => joined.mismatch = true,
            }
        } else if !same_receivers(&joined.objects, &live) {
            joined.mismatch = true;
        }
        if !joined.mismatch {
            for (template, kept) in joined.objects.iter().zip(&live) {
                for (to, from) in template.arguments.iter().zip(&kept.arguments) {
                    let bits = self.builder.use_var(*from);
                    self.builder.def_var(*to, bits);
                }
            }
        }
        let carried = !joined.mismatch || live.is_empty();
        self.joined_virtuals.insert(pc, joined);
        carried
    }

    /// The join's own receivers for `live`: each keeps its callee, fields and constants, with fresh
    /// variables for its arguments, and a field that reads an argument reads the join's variable for it.
    fn join_template(&mut self, live: &[VirtualObject]) -> Option<smallvec::SmallVec<[VirtualObject; 1]>> {
        let mut objects = smallvec::SmallVec::new();
        for kept in live {
            let arguments: smallvec::SmallVec<[Variable; 4]> = kept.arguments.iter().map(|_| self.builder.declare_var(types::I64)).collect();
            let mut fields = smallvec::SmallVec::new();
            for (atom, variable, constant, ty) in &kept.fields {
                let variable = match variable {
                    Some(variable) => Some(arguments[kept.arguments.iter().position(|argument| argument == variable)?]),
                    None => None,
                };
                fields.push((*atom, variable, *constant, *ty));
            }
            objects.push(VirtualObject { register: kept.register, callee: kept.callee, fields, arguments });
        }
        Some(objects)
    }

    /// The kept receivers a forward-only join takes from all its arrivals (`carry_virtuals`), at its
    /// entry. Any arrival that carried other receivers, or fewer arrivals than the join's forward
    /// edges, declines the region: a receiver one path holds as an object cannot join as fields.
    pub(crate) fn join_virtuals(&mut self, state: &mut State, pc: usize) {
        let Some(joined) = self.joined_virtuals.remove(&pc) else { return };
        if joined.objects.is_empty() { return; }
        if joined.mismatch || self.forward.get(pc).copied().flatten() != Some(joined.arrivals) {
            self.valid = false;
            declined("a kept receiver reaches a join unevenly", pc);
            return;
        }
        for kept in joined.objects {
            let index = usize::from(kept.register);
            state.values[index] = RegisterFact::definition(Type::ANY, Representation::Tagged);
            state.proofs.homed.remove(index);
            state.virtuals.push(kept);
        }
    }

    /// Allocates each kept receiver live at `pc` by its plan, from the arguments it kept, into
    /// its register, and drops the rest: what the instruction at `pc`, or the interpreter
    /// resuming there, reads as the object.
    pub(crate) fn materialize_virtuals(&mut self, state: &mut State, pc: usize) {
        for kept in std::mem::take(&mut state.virtuals) {
            if !self.is_live(pc, kept.register) { continue; }
            let count = kept.arguments.len();
            let slot = self.builder.create_sized_stack_slot(ir::StackSlotData::new(ir::StackSlotKind::ExplicitSlot,
                (count.max(1) * size_of::<Value>()) as u32, align_of::<Value>().trailing_zeros() as u8));
            for (index, variable) in kept.arguments.iter().enumerate() {
                let bits = self.builder.use_var(*variable);
                self.slot_store(slot, Lane::I64, bits, (index * size_of::<Value>()) as i32);
            }
            let arguments = self.slot_address(slot, 0);
            let callee = self.plan_callee_word(kept.callee);
            let argc = self.raw_const(Lane::I64, count as i64);
            let payload = self.payload;
            let this = self.helper_call(LaneHelper::Construct, &[payload, callee, arguments, argc]);
            // The plan held at the construction and nothing since could retire it or make a
            // collection due: the allocation cannot refuse. Were it to, the code stops rather
            // than resume with a register that names no object.
            let built = self.raw_compare_imm(Relation::NotEqual, this, Value::DELETED.to_bits() as i64);
            self.builder.ins().trapz(built, ir::TrapCode::unwrap_user(1));
            state.proofs.layouts.clear();
            state.proofs.global = None;
            self.reload_property_arena_if_entered();
            // The allocation may make a collection due: a later construction tests again.
            self.unready_constructions();
            // The register's tagged view and fact alone: a leave's state holds no other proofs.
            let index = usize::from(kept.register);
            self.written.insert(index);
            self.builder.def_var(self.registers.tagged[index], this);
            let mut fact = RegisterFact::definition(Type::ANY, Representation::Tagged);
            fact.tagged = TaggedView::Value(this);
            state.values[index] = fact;
            state.proofs.homed.remove(index);
        }
    }

    /// On an entering edge of the loop at `header`, entered there only, the test of each
    /// callee its kept constructions build by (`virtual_construct_plan`), once: its answer is
    /// the callee's readiness in the loop (`ConstructReady`). A refusal costs nothing here;
    /// each construction then tests at its site and leaves there as before.
    pub(crate) fn hoist_constructions(&mut self, header: usize) {
        let Some(end) = self.loop_end(header).filter(|end| self.enters_only_at_header(header, *end)) else { return };
        let mut callees = smallvec::SmallVec::<[PlanCallee; 2]>::new();
        for pc in header..=end {
            let Some(plan) = self.virtual_construct_plan(pc, self.unit.instructions[pc]) else { continue };
            if !callees.contains(&plan.callee) { callees.push(plan.callee); }
        }
        for callee in callees {
            // One variable per callee: an inner loop's entry and an outer one's define the same.
            let variable = match self.construct_ready.iter().find(|ready| ready.callee == callee) {
                Some(ready) => ready.variable,
                None => self.builder.declare_var(types::I64),
            };
            let expected = self.plan_callee_word(callee);
            let payload = self.payload;
            let holds = self.helper_call(LaneHelper::ConstructPlanHolds, &[payload, expected]);
            self.builder.def_var(variable, holds);
            if !self.construct_ready.iter().any(|ready| ready.callee == callee && ready.header == header) {
                self.construct_ready.push(ConstructReady { callee, variable, header, end });
            }
        }
    }

    /// Whether the instruction at `pc` leaves every construction readiness as it found it
    /// (`ConstructReady`): it allocates nothing, calls nothing, stores nothing and constructs
    /// nothing but a kept receiver, and runs no guest code.
    pub(crate) fn keeps_constructions_ready(&self, state: &State, pc: usize, instruction: WideInstruction) -> bool {
        if self.hoisted.contains_key(&pc) { return true; }
        if self.unit.markers.contains(&pc) { return false; }
        if self.virtual_construct_plan(pc, instruction).is_some() { return true; }
        if crate::compiled::meaning_compiled::is_get_field(instruction) {
            return self.field_base(pc, instruction).and_then(|base| base.register_index())
                .is_some_and(|base| state.virtuals.iter().any(|kept| kept.register == base
                    && self.is_live(pc, kept.register) && self.virtual_reads(instruction, kept)));
        }
        use quench_runtime::staged::meaning::{means, Meaning};
        if [Meaning::Move, Meaning::LoadConst, Meaning::Nop, Meaning::Jump].into_iter().any(|meaning| means(instruction, meaning)) { return true; }
        let compare_jump = means(instruction, Meaning::CompareJump);
        // ToBoolean of any immediate, and Number arithmetic and relations, run no guest code
        // and allocate nothing; on any other operand the step may.
        let operands = if means(instruction, Meaning::TruthJump) { Type::IMMEDIATE }
            else if compare_jump || means(instruction, Meaning::IncDec)
                || (quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericUnary)) { Type::NUMBER }
            else { return false };
        let typed = |register: Register| state.values.get(usize::from(register)).map_or(Type::ANY, |fact| fact.value.ty);
        // A binary step's operands may also be constants, fields or locals: a string constant
        // would concatenate, a field read run a getter.
        if (compare_jump || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary))
            && ![instruction.operand_b(), instruction.operand_c()].into_iter().all(|operand|
                super::super::facts::operand_type(&self.program.constants, operand, typed).is_subtype(Type::NUMBER)) {
            return false;
        }
        let (uses, _) = quench_runtime::compile::liveness::transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
        (0..state.values.len()).filter(|register| uses.contains(*register))
            .all(|register| state.values[register].value.ty.is_subtype(operands))
    }

    /// Forgets every construction readiness from here (`ConstructReady`).
    pub(crate) fn unready_constructions(&mut self) {
        if self.construct_ready.is_empty() { return; }
        let zero = self.raw_const(Lane::I64, 0);
        let mut forgotten = smallvec::SmallVec::<[Variable; 2]>::new();
        for ready in &self.construct_ready {
            if !forgotten.contains(&ready.variable) { forgotten.push(ready.variable); }
        }
        for variable in forgotten { self.builder.def_var(variable, zero); }
    }

    /// The property arena's base again after an allocation, where the body holds one.
    fn reload_property_arena_if_entered(&mut self) {
        if self.entry.is_some() { self.reload_property_arena(); }
    }
}
