//! The scalar entry of a numeric function that calls itself: the same lowering as its numeric
//! region (`Completion::Number`), but its parameters arrive in registers, each an int32 or a
//! binary64, and its result is the status and the number a `Number` return joins to. A call
//! of the function by itself whose arguments are proved those numbers calls the entry through
//! the function's cell (`EntryCell::scalar`): no frame is pushed, no register is filled and
//! nothing is boxed. The entry takes the closure's dense capture slots in the one word a body's
//! entry takes them in (`entry_cell::capture_word`): the function's own binding is a capture
//! read, whose value the entry compares with the callee its call was linked for. A status other than zero (an overflow, a miss of any guard, an interrupt,
//! the stack's end, a retired body's stub) leaves at the call, so the function's own generic
//! call restarts it, as a restartable numeric call does (`CallRecovery::RestartCall`).
//!
//! The callee is the function the global binding names, a Stable fact the call's lowering
//! holds under the body's own dependencies (`GlobalRead::Stable`): the load of the binding
//! defines the callee register as that constant, and the call compares nothing per use.

#[allow(unused_imports)] use quench_runtime::staged::{OperationNamesExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::lane_body_abi::body_signature_in_lane;
use super::*;
use quench_runtime::bytecode::ImmediateLayout;
use quench_runtime::staged::flow::FlowStage;
use quench_runtime::staged::{Access, GlobalBinding, Operation, Region, Stage};
use quench_runtime::staged::machine::{Lane, MachineAccess, MachineScalar, Relation, Unary};
use quench_runtime::vm::global_facts::GlobalRead;
use super::super::object_slots::emit;
use super::super::declared_checks::LANE_TESTED;
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// The alias region of the function's entry cell, which a scalar call reads its entry from.
const CELL: Access = Region::EntryCell.mutable();

/// The parameters a scalar entry takes: one bit of `floats` each.
const MAX_PARAMETERS: usize = u8::BITS as usize;

/// The words a scalar entry takes before its parameters: the context and the captures' word.
const SCALAR_LEADING_WORDS: usize = 2;

/// The words a scalar call passes: those and the parameters.
const ARGUMENT_WORDS: usize = MAX_PARAMETERS + SCALAR_LEADING_WORDS;

/// The words a lane body's entry block takes: context, receiver, captures and register file.
const ENTRY_WORDS: usize = 4;

/// What a scalar entry takes in registers after the context: the function's first `params`
/// parameters, each a binary64 where its bit of `floats` is set and an int32 otherwise.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct ScalarAbi {
    floats: u8,
    params: u8,
}

impl ScalarAbi {
    /// The entry of `function_id` over `unit`, which calls itself through `direct`: where the unit
    /// is numeric but for those calls and the binding loads they take their callee from.
    pub(crate) fn of(program: &ResidualProgram, function_id: u32, unit: &super::super::lane_unit::Unit,
        direct: &DirectSites, facts: Option<&quench_runtime::compile::aot_facts::AotFacts>) -> Option<Self> {
        let function = program.function_get(function_id as usize)?;
        let params = u8::try_from(function.params).ok().filter(|params| usize::from(*params) <= MAX_PARAMETERS)?;
        // SAFETY: the body owns its direct callees for as long as its code runs.
        let recurses = direct.values().flatten().any(|callee| unsafe { &**callee }.target.function == function_id);
        if !recurses || !numeric_unit(program, unit, true, direct)
            || !crate::js_compiled::function_candidate(function, program) { return None; }
        // A parameter is an int32 where the solve settles its cell at entry as one (`FunctionFacts::cell_at`), else a Number.
        let solved = facts.map(|facts| facts.function_facts(function_id));
        let int = |index: usize| solved.as_ref().zip(parameter_register(function, index))
            .and_then(|(solved, register)| solved.cell_at(0, register)).is_some_and(|cell| cell.ty.is_subtype(Type::INT32));
        let floats = (0..usize::from(params)).filter(|index| !int(*index)).fold(0_u8, |bits, index| bits | (1_u8 << index));
        Some(Self { floats, params })
    }

    fn params(self) -> usize { usize::from(self.params) }

    fn is_float(self, index: usize) -> bool { (self.floats >> index) & 1 != 0 }

    /// What the lowering knows of parameter `index` on entry.
    fn fact_type(self, index: usize) -> Type {
        if self.is_float(index) { Type::NUMBER } else { Type::INT32 }
    }

    /// The signature of the entry: the context, the captures' word, the parameters, then a status
    /// and a result word.
    pub(crate) fn signature(self, pointer: ir::Type, call_conv: cranelift_codegen::isa::CallConv) -> Signature {
        let mut signature = Signature::new(call_conv);
        signature.params.extend([AbiParam::new(pointer), AbiParam::new(types::I64)]);
        signature.params.extend((0..self.params()).map(|index| AbiParam::new(if self.is_float(index) { types::F64 } else { types::I32 })));
        signature.returns.extend([AbiParam::new(types::I32), AbiParam::new(types::I64)]);
        signature
    }
}

#[quench_macros::ext(CompletionOps3)]
impl Completion {
    /// Whether a run returns a Number by its status and a result word (a numeric region, or a scalar entry).
    pub(crate) fn numeric(self) -> bool { self != Self::Body }

    /// The signature the run's function has: a scalar entry's under `call_conv` (the default), every other run's the
    /// lane body convention.
    pub(crate) fn signature(self, pointer: ir::Type, call_conv: cranelift_codegen::isa::CallConv) -> Signature {
        match self {
            Self::Scalar(abi) => abi.signature(pointer, call_conv),
            _ => body_signature_in_lane(pointer),
        }
    }

    /// The types of the registers at entry: a numeric region's parameters are Numbers, a scalar
    /// entry's what it takes.
    pub(crate) fn initial_types(self, registers: usize, params: usize) -> Vec<Type> {
        let mut initial = vec![Type::ANY; registers];
        match self {
            Self::Body => {}
            Self::Number => initial[..params].fill(Type::NUMBER),
            Self::Scalar(abi) => initial.iter_mut().take(params).enumerate().for_each(|(index, ty)| *ty = abi.fact_type(index)),
        }
        initial
    }
}

/// The plan of a scalar entry: the whole function, its parameters in registers.
pub(crate) fn plan(abi: ScalarAbi) -> RegionPlan {
    RegionPlan { coverage: Coverage::Full, completion: Completion::Scalar(abi), views: ElementViews::Dense }
}

/// The scalar entry `specializer` lowers, if it fits a function's cell: its result is an int32
/// (which a recursive call assumes) and it polls no loop.
pub(crate) fn lowered(program: &ResidualProgram, specializer: RegionSpecializer<'_>) -> Option<ir::Function> {
    let region = specializer.specialize(program).ok()?;
    (region.number_result == Some(NativeNumberResult::Int32) && !region.fault_poll).then_some(region.function)
}

/// The words an entry block's parameters are: the context, the receiver, the captures and the
/// register file, of which a scalar entry has the context and the captures.
pub(crate) fn entry_words(builder: &mut FunctionBuilder<'_>, completion: Completion, parameters: &[ir::Value]) -> Option<[ir::Value; ENTRY_WORDS]> {
    match (completion, parameters) {
        (Completion::Scalar(_), [context, captures, ..]) => {
            let none = emit(builder).raw_const(Lane::I64, 0);
            Some([*context, none, *captures, none])
        }
        (_, [context, this, captures, registers]) => Some([*context, *this, *captures, *registers]),
        _ => None,
    }
}

/// Whether the unit has no heap, environment or guest-call effect that scalar-call recovery
/// could repeat: no inlined frame, and only numeric instructions, but for a scalar entry's
/// direct calls and the loads of their callees' bindings.
pub(crate) fn numeric_unit(program: &ResidualProgram, unit: &super::super::lane_unit::Unit, scalar: bool,
    direct: &DirectSites) -> bool {
    unit.frames.is_empty() && unit.instructions.iter().enumerate().all(|(pc, instruction)|
        numeric_instruction(program, *instruction)
            || scalar && (direct.contains_key(&pc) || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::ThrowingNameLoad)
                || callee_capture(unit, direct, pc).is_some()))
}

/// The self-call that takes the callee the capture read at `pc` loads: the first direct call
/// after it whose callee register the read defines, where the read is of the activation's own scope.
fn callee_capture<'a>(unit: &super::super::lane_unit::Unit, direct: &'a DirectSites, pc: usize) -> Option<&'a DirectCallee> {
    let read = unit.instructions[pc];
    let own = |at: usize| matches!(unit.origins[at], super::super::lane_unit::Origin::Caller(_));
    if !matches!(crate::compiled::decoded(read), Some(quench_runtime::staged::Operation::LoadCapture { .. })) || read.capture_depth() != 0 || read.result_flags() != 0 || !own(pc) { return None; }
    let call = *direct.range(pc..).find(|(call, _)| own(**call) && unit.instructions[**call].register_b() == read.result_register())?.1.first()?;
    // SAFETY: the body owns its direct callees for as long as its code runs.
    Some(unsafe { &*call })
}

/// Whether the non-empty language set `bits` is one entry conformance tests exactly (`conform_language`):
/// the members of `LANE_TESTED`: a Boolean, `undefined`, `null`, a direct object, array, plain object or
/// function, and a string. A set with a Number, BigInt or Symbol is not checkable here.
pub(crate) fn checkable(bits: u16) -> bool {
    bits != 0 && bits & !LANE_TESTED == 0
}

/// The register parameter `index` enters in.
pub(crate) fn parameter_register(function: &quench_runtime::bytecode::Function, index: usize) -> Option<Register> {
    let index = u16::try_from(index).ok().filter(|index| *index < function.params)?;
    match function.parameter_registers {
        Some(base) => base.checked_add(index),
        None => function.local_registers.iter().find(|entry| entry.local == index).map(|entry| entry.register),
    }
}

impl Lowering<'_, '_> {
    /// The scalar entry the lowering calls and defines: its own, or the one its body's plan
    /// found (`VmFacts::scalar`).
    pub(crate) fn scalar_abi(&self) -> Option<ScalarAbi> {
        match self.completion {
            Completion::Scalar(abi) => Some(abi),
            _ => self.vm.scalar,
        }
    }

    /// A numeric entry's parameters. A window entry proves each a Number once; a scalar entry
    /// holds each as the number it takes, after its check of the machine stack unless `stack_check_free` admits none.
    pub(crate) fn enter_numeric_parameters(&mut self, state: &mut State, pc: usize, parameters: &[ir::Value]) {
        let Completion::Scalar(abi) = self.completion else {
            for register in 0..self.params as Register {
                let miss = self.guard_exit_for(state, pc as u32, ExitFact::NumberKind);
                self.number(state, register, miss);
            }
            return;
        };
        if !super::polls::stack_check_free(self.vm.graph, self.function_id) { self.check_native_stack(state, pc); }
        self.hold_callee_slots(state, pc);
        for (index, value) in parameters.iter().skip(SCALAR_LEADING_WORDS).enumerate() {
            if abi.is_float(index) { self.set_number(state, index as Register, *value); } else { self.set_int(state, index as Register, *value); }
        }
    }

    /// Leaves at `pc` where this entry has run past the machine stack's budget (`CallContext::stack_limit`) less
    /// `STACK_CHECK_MARGIN`, which keeps free the frames of the bounded callees `stack_check_free` admits unchecked.
    /// Entries that recurse, or reach unnamed calls, each make their own check.
    fn check_native_stack(&mut self, state: &State, pc: usize) {
        let limit = self.raw_load(Lane::I64, super::super::invariant(), self.context, super::super::CONTEXT_STACK_LIMIT);
        let limit = self.raw_binary_imm(Binary::Add, limit, super::polls::STACK_CHECK_MARGIN as i64);
        let stack = emit(&mut self.builder).stack_pointer();
        let room = self.raw_compare(Relation::UnsignedGreaterEqual, stack, limit);
        let miss = self.guard_exit_for(state, pc as u32, ExitFact::NumberKind);
        super::super::emit_js_guard(&mut self.builder, room, miss);
    }

    /// The callee of the direct call at `pc` if it is of this function itself.
    fn scalar_target(&self, pc: usize) -> Option<&DirectCallee> {
        // SAFETY: the body owns its direct callees for as long as its code runs.
        let callee = self.direct.get(&pc).and_then(|sites| sites.first()).map(|callee| unsafe { &**callee })?;
        (callee.target.function == self.function_id && callee.forwarder.is_none()).then_some(callee)
    }

    /// Whether the call at `pc` is one of this function by its scalar entry: the callee register
    /// holds the function as a Stable fact, and the arguments are proved the numbers the entry takes.
    pub(crate) fn scalar_call_admitted(&self, state: &State, pc: usize, instruction: WideInstruction) -> bool {
        let (Some(abi), Some(callee)) = (self.scalar_abi(), self.scalar_target(pc).map(|callee| callee.callee)) else { return false };
        if !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Call) || instruction.result_flags() != 0 { return false; }
        let window = usize::from(ImmediateLayout::call_window_base(instruction.imm()));
        let held = state.values[usize::from(instruction.register_b())].value.ty.function_identity();
        held.map(|identity| identity.to_bits()) == Some(callee.to_bits())
            && usize::from(ImmediateLayout::argument_count(instruction.imm())) >= abi.params()
            && (0..abi.params()).all(|index| state.values[window + index].value.ty.is_subtype(abi.fact_type(index)))
    }

    /// The call at `pc` (`scalar_call_admitted`): the entry called with the arguments in registers; the
    /// result is an int32 where it returned, else the call leaves at `pc` for the generic call to run.
    pub(crate) fn scalar_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let abi = self.scalar_abi().expect("an admitted scalar call has its entry");
        let callee = self.scalar_target(pc).expect("an admitted scalar call has its callee");
        let (cell, captures) = (callee.cell, callee.capture_word());
        let miss = self.guard_exit_for(state, pc as u32, ExitFact::NumberKind);
        let window = ImmediateLayout::call_window_base(instruction.imm());
        let mut arguments = smallvec::SmallVec::<[ir::Value; ARGUMENT_WORDS]>::new();
        arguments.push(self.context);
        arguments.push(self.reloc(RelocKind::CaptureWords, self.function_id, captures));
        for index in 0..abi.params() {
            let register = window + index as Register;
            arguments.push(if abi.is_float(index) { self.number(state, register, miss) } else { self.int(state, register, miss) });
        }
        let signature = abi.signature(types::I64, self.builder.func.signature.call_conv);
        let signature = quench_runtime::vm::jit_backend::signature_ref(self.builder.func, signature);
        let cell = self.reloc(RelocKind::EntryCell, self.function_id, cell as u64);
        let mut stage = emit(&mut self.builder);
        let entry = stage.entry_target(crate::compiled::slow_paths::EngineEntry::CompiledEntry, crate::compiled::regions::flags(CELL), cell, quench_runtime::staged::call::ENTRY_CELL.scalar_offset);
        let (status, word) = stage.call_entry(signature, entry, &arguments);
        let returned = stage.raw_compare_imm(Relation::Equal, status, 0);
        let result = stage.raw_unary(Unary::Narrow(Lane::I32), word);
        super::super::emit_js_guard(&mut self.builder, returned, miss);
        self.set_int(state, instruction.result_register(), result);
        true
    }

    /// The dense capture slots (address, count) in the entry's captures word (`entry_cell::capture_word`).
    pub(crate) fn entry_capture_slots(&mut self) -> (ir::Value, ir::Value) {
        let word = self.entry_words.1;
        self.capture_slots_of(word)
    }

    /// The dense capture slots (address, count) a captures word names (`entry_cell::capture_word`).
    pub(crate) fn capture_slots_of(&mut self, word: ir::Value) -> (ir::Value, ir::Value) {
        let mask = self.raw_const(Lane::I64, super::super::entry_cell::CAPTURE_ADDRESS_MASK);
        let values = self.raw_binary(Binary::And, word, mask);
        (values, self.raw_binary_imm(Binary::ShiftRightUnsigned, word, i64::from(super::super::entry_cell::CAPTURE_ADDRESS_BITS)))
    }

    /// Whether `instruction` at `pc` loads the callee of a self-call by the scalar entry: its
    /// capture, in the entry or in the body that calls it, or its global binding, in the entry.
    pub(crate) fn is_scalar_callee_load(&self, pc: usize, instruction: WideInstruction) -> bool {
        if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) { return self.scalar_abi().is_some() && callee_capture(self.unit, self.direct, pc).is_some(); }
        quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::ThrowingNameLoad) && matches!(self.completion, Completion::Scalar(_))
    }

    /// The load `is_scalar_callee_load` admits.
    pub(crate) fn scalar_callee_load(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::LoadCapture { .. })) { self.scalar_callee_capture(state, pc, instruction) } else { self.scalar_callee(state, pc, instruction) }
    }

    /// The address of each capture slot a callee read names, from the entry's dense slots: their
    /// bounds are checked here once, so a read is one load and its comparison with the callee.
    fn hold_callee_slots(&mut self, state: &State, pc: usize) {
        let mut slots: Vec<u16> = (0..self.unit.instructions.len())
            .filter(|read| callee_capture(self.unit, self.direct, *read).is_some()).map(|read| self.unit.instructions[read].capture_slot()).collect();
        slots.sort_unstable();
        slots.dedup();
        if slots.is_empty() { return; }
        let (values, len) = self.entry_capture_slots();
        let miss = self.guard_exit_for(state, pc as u32, ExitFact::NumberKind);
        for slot in slots {
            let address = quench_runtime::staged::environment::slot_address(&mut emit(&mut self.builder), values, len, slot, miss)
                .expect("the emit stage continues past its guards");
            self.callee_slots.push((slot, address));
        }
    }

    /// The capture read of the callee a scalar entry's call was linked for, in the entry or in the body
    /// that calls it: its value (one load from the entry's held slot, in the entry) compared with that
    /// callee, which the register then holds as a constant. A closure the binding names otherwise leaves the entry to the generic call.
    fn scalar_callee_capture(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let direct = callee_capture(self.unit, self.direct, pc).expect("a callee capture read has its call");
        let (callee, body) = (direct.callee, direct.target.function);
        let miss = self.guard_exit_for(state, pc as u32, ExitFact::NumberKind);
        let loaded = match self.completion {
            Completion::Scalar(_) => {
                let address = self.callee_slots.iter().find(|(slot, _)| *slot == instruction.capture_slot()).map(|(_, address)| *address)
                    .expect("a callee's capture slot is held at entry");
                emit(&mut self.builder).load_u64(Region::Cells.mutable(), address, 0)
            }
            _ => self.load_capture(state, pc, instruction, miss, None, RootedRead::No),
        };
        // A loaded closure of a body the closed world makes one object is that object (`is_body_closure_identity`): no compare.
        if !self.is_body_closure_identity(pc, instruction.result_register(), body) {
            let same = self.is_identity(loaded, callee);
            super::super::emit_js_guard(&mut self.builder, same, miss);
        }
        let constant = self.identity_constant(callee);
        let ty = Type::function(callee).expect("a direct callee is a function");
        self.define(state, instruction.result_register(), constant, RegisterFact::definition(ty, Representation::Tagged));
        true
    }

    /// The load of the binding a scalar entry's callee is named by: its Stable value, which the body's
    /// own load holds as a dependency (`global_read`), is the register's constant. A binding no
    /// dependency holds leaves the entry to the generic call.
    fn scalar_callee(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        let Some(&GlobalRead::Stable { value, ty, .. }) = self.global_reads.get(&pc) else {
            self.valid = false;
            return true;
        };
        let proof = super::EmitProof::GlobalRead { miss: None, binding: GlobalBinding::Held { value, ty }, slot: None };
        let row = Operation::decode_load_name(instruction, proof).expect("a global read is a name read");
        self.step(state, frame::Edges::Value, row);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The entry takes the context and its numbers in registers, and answers a status and a word.
    #[test]
    fn the_entry_takes_numbers_in_registers_and_answers_a_status_and_a_word() {
        let abi = ScalarAbi { floats: 0b10, params: 2 };
        let signature = abi.signature(types::I64, cranelift_codegen::isa::CallConv::SystemV);
        let params: Vec<_> = signature.params.iter().map(|param| param.value_type).collect();
        let returns: Vec<_> = signature.returns.iter().map(|ret| ret.value_type).collect();
        assert_eq!((params, returns), (vec![types::I64, types::I64, types::I32, types::F64], vec![types::I32, types::I64]));
        assert_eq!(Completion::Scalar(abi).initial_types(3, 2), [Type::INT32, Type::NUMBER, Type::ANY]);
    }
}
