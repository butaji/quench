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

use super::*;
use super::super::{ScalarConstructPlan, ScalarField};

/// A receiver kept as its field values.
#[derive(Clone, PartialEq)]
pub(super) struct VirtualObject {
    /// The register the construction defined.
    pub(super) register: Register,
    /// The plan's callee, which the allocation at a leave constructs again.
    callee: Value,
    /// Per field in slot order: its name, and the variable holding its tagged value (an
    /// argument's, or a constant's) with the type the value had.
    fields: smallvec::SmallVec<[(Atom, Option<Variable>, Value, Type); 4]>,
    /// The arguments as passed, which the allocation passes the plan again.
    arguments: smallvec::SmallVec<[Variable; 4]>,
}

/// The most arguments a kept construction passes.
const MAX_ARGUMENTS: u16 = 8;

impl Lowering<'_, '_> {
    /// The plan the construction at `pc` would build its receiver by, where the body may keep
    /// that receiver as its field values: a plain construction of registers whose site named one
    /// callee with such a plan (`OnlineFacts::construct_plans`).
    pub(super) fn virtual_construct_plan(&self, pc: usize, instruction: WideInstruction) -> Option<ScalarConstructPlan> {
        if self.completion.numeric() || instruction.result_flags() != 0 || !self.completion.status_and_pc() { return None; }
        let crate::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return None };
        if window.count > MAX_ARGUMENTS || !self.is_construction(instruction) { return None; }
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
    pub(super) fn virtual_construct(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, plan: ScalarConstructPlan) -> bool {
        let crate::bytecode::ConstructArguments::Registers(window) = instruction.construct_arguments() else { return false };
        let exit = self.guard_exit(state, pc as u32);
        let callee = self.tagged(state, instruction.register_b());
        let expected = self.identity_constant(plan.callee);
        let same = self.raw_compare(Relation::Equal, callee, expected);
        emit_js_guard(&mut self.builder, same, exit);
        let payload = self.payload;
        let holds = self.helper_call(LaneHelper::ConstructPlanHolds, &[payload, expected]);
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
    pub(super) fn virtual_field(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
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
    pub(super) fn drop_overwritten_virtuals(&self, state: &mut State, instruction: WideInstruction) {
        let (_, definitions) = crate::compile::liveness::transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
        state.virtuals.retain(|kept| !definitions.contains(usize::from(kept.register)));
    }

    /// Whether the instruction at `pc` leaves every kept receiver kept: it reads none of them
    /// but as a field the kept values answer, and it neither allocates nor calls nor ends the
    /// block with one live.
    pub(super) fn keeps_virtuals(&self, state: &State, pc: usize, instruction: WideInstruction) -> bool {
        let (uses, _) = crate::compile::liveness::transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
        let virtual_use = state.virtuals.iter().any(|kept| uses.contains(usize::from(kept.register)));
        let ends_block = self.blocks.get(pc + 1).is_some_and(Option::is_some)
            || !matches!(instruction.op().control_flow_layout(), crate::bytecode::ControlFlowLayout::Fallthrough)
            || instruction.returns_from_frame();
        if ends_block && state.virtuals.iter().any(|kept| self.is_live(pc + 1, kept.register)) { return false; }
        if virtual_use {
            return instruction.op().is_get_field() && self.field_base(pc, instruction)
                .and_then(|base| base.register_index())
                .is_some_and(|base| state.virtuals.iter().any(|kept| kept.register == base && self.virtual_reads(instruction, kept)));
        }
        let op = instruction.op();
        let numbers = || (0..state.values.len()).filter(|register| uses.contains(*register))
            .all(|register| state.values[register].value.ty.is_subtype(Type::NUMBER));
        op.copies_register()
            || op.js_denotation() == JsDenotation::LoadConst
            || matches!(op.js_denotation(), JsDenotation::NumericBinary | JsDenotation::NumericUnary) && numbers()
    }

    /// Whether `kept`'s values answer the read `instruction`: one plain read of a field it keeps.
    fn virtual_reads(&self, instruction: WideInstruction, kept: &VirtualObject) -> bool {
        instruction.result_flags() == 0 && !instruction.writes_current_this()
            && self.field_chain(instruction).is_some_and(|(parts, sink)| sink.is_none()
                && matches!(parts[..], [(atom, _)] if kept.fields.iter().any(|(name, ..)| *name == atom)))
    }

    /// Allocates each kept receiver live at `pc` by its plan, from the arguments it kept, into
    /// its register, and drops the rest: what the instruction at `pc`, or the interpreter
    /// resuming there, reads as the object.
    pub(super) fn materialize_virtuals(&mut self, state: &mut State, pc: usize) {
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
            let callee = self.identity_constant(kept.callee);
            let argc = self.raw_const(Lane::I64, count as i64);
            let payload = self.payload;
            let this = self.helper_call(LaneHelper::Construct, &[payload, callee, arguments, argc]);
            // The plan held at the construction and nothing since could retire it or make a
            // collection due: the allocation cannot refuse. Were it to, the code stops rather
            // than resume with a register that names no object.
            let built = self.raw_compare_imm(Relation::NotEqual, this, Value::DELETED.to_bits() as i64);
            self.builder.ins().trapz(built, ir::TrapCode::unwrap_user(1));
            state.proofs.layouts.clear();
            self.reload_property_arena_if_entered();
            // The register's tagged view and fact alone: a leave's state holds no other proofs.
            let index = usize::from(kept.register);
            self.written.insert(index);
            self.builder.def_var(self.registers.tagged[index], this);
            let mut fact = RegisterFact::definition(Type::ANY, Representation::Tagged);
            fact.tagged = TaggedView::Value(this);
            state.values[index] = fact;
            state.proofs.homed.remove(index);
            if let Some(header) = state.proofs.headers.get_mut(index) { *header = None; }
        }
    }

    /// The property arena's base again after an allocation, where the body holds one.
    fn reload_property_arena_if_entered(&mut self) {
        if self.entry.is_some() { self.reload_property_arena(); }
    }
}
