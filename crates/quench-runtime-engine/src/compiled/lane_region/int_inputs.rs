//! Loop inputs an entry observed to be int32s: the invariants of an OSR loop that its index
//! arithmetic reads, which the entry proves int32 once, so the arithmetic is integer words
//! (`index_math`). No magnitude is assumed of them: the words leave only where a result really
//! passes what a Number holds exactly (`IntRange::EXACT`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::operations::NumberArithmetic;
use quench_runtime::staged::number_machine;
use quench_runtime::vm::operations::NumberRelation;

impl Lowering<'_, '_> {
    /// The registers the loop entered at the region's header reads as operands of index
    /// arithmetic (an add, subtraction or product only an index reads), never writes, and the
    /// entry observed to hold an int32.
    fn index_inputs(&self, loops: &[(usize, usize)]) -> Vec<Register> {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return Vec::new() };
        let body = &self.unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|instruction| writes(*instruction)).collect();
        let mut inputs: Vec<Register> = (header..).zip(body).filter(|(pc, instruction)| is_integral_arithmetic(**instruction)
            && self.demand_from(pc + 1, usize::from(instruction.result_register())).index_only())
            .flat_map(|(_, instruction)| [instruction.operand_b(), instruction.operand_c()])
            .filter_map(Operand::register_index)
            .filter(|register| *register < self.unit.caller_registers && !written.contains(register)
                && self.int_entries.iter().any(|(entry, _)| entry == register) && self.is_live(header, *register)).collect();
        inputs.sort_unstable();
        inputs.dedup();
        inputs
    }

    /// Takes each input of the loop's index arithmetic (`index_inputs`) for an int32 contract of
    /// its entering edge: proved once there, the loop's index arithmetic runs on integers instead
    /// of converting a Number per use. Whether it took any.
    pub(crate) fn seed_loop_ints(&self, initial: &mut [Type], loops: &[(usize, usize)]) -> bool {
        let mut seeded = false;
        for register in self.index_inputs(loops) {
            let slot = &mut initial[usize::from(register)];
            if slot.is_any_or_number() {
                *slot = Type::INT32;
                seeded = true;
            }
        }
        seeded
    }

    /// The observed int32 entries live at `entry` (a dead register's value is stale, and nothing
    /// conforms it) whose range is narrower than int32 and whose entry fact is int32: what the
    /// entry guards once (`guard_entering_ranges`) and the ranges pass assumes.
    pub(crate) fn entering_ranges(&self, entry: usize) -> Vec<(Register, IntRange)> {
        let types = self.facts.at(entry);
        self.int_entries.iter().copied().filter(|&(register, range)| range != IntRange::FULL && self.is_live(entry, register)
            && types.and_then(|types| types.get(usize::from(register))).is_some_and(|ty| ty.is_subtype(Type::INT32))).collect()
    }

    /// Leaves at `pc` where an entering register is outside its range: the guard the ranges pass's
    /// entry state relies on, of each bound tighter than int32's.
    pub(crate) fn guard_entering_ranges(&mut self, state: &mut State, pc: usize) {
        for (register, range) in self.entering.clone() {
            let exit = self.guard_exit_for(state, pc as u32, ExitFact::NumberKind);
            let value = self.int(state, register, exit);
            if range.min > IntRange::FULL.min {
                let min = i32::try_from(range.min).expect("an entering range's bound is an int32");
                number_machine::guard_int_bound(&mut emit(&mut self.builder), NumberRelation::GreaterEqual, value, min, exit);
            }
            if range.max < IntRange::FULL.max {
                let max = i32::try_from(range.max).expect("an entering range's bound is an int32");
                number_machine::guard_int_bound(&mut emit(&mut self.builder), NumberRelation::LessEqual, value, max, exit);
            }
        }
    }
}

/// Whether `instruction` is an addition, subtraction or multiplication of Numbers.
fn is_integral_arithmetic(instruction: WideInstruction) -> bool {
    quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && !instruction.returns_from_frame()
        && instruction.result_flags() == 0
        && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())
            .is_some_and(NumberArithmetic::integral)
}

/// The locals of `function` (`function_id`) its AOT image's closed world proves Numbers at every
/// read and write (`AotFacts::numbers`), added to the annotated locals (`declared::declared_local_types`)
/// as one source of local types: each is conformed where it is written, so a join keeps it unboxed.
/// Parameters enter typed instead (`closed_entry_types`); a register an annotation types keeps it.
pub(crate) fn closed_local_types(vm: VmFacts<'_>, function_id: u32, function: &quench_runtime::bytecode::Function, locals: &mut Vec<(Register, Type)>) {
    let Some(numbers) = vm.closed.and_then(|closed| closed.function_facts(function_id).numbers()) else { return };
    let parameters: Vec<usize> = (0..usize::from(function.params)).filter_map(|index| super::scalar::parameter_register(function, index).map(usize::from)).collect();
    for (register, number) in numbers.iter().enumerate() {
        let ty = match number {
            Some(Representation::Int32) => Type::INT32,
            Some(Representation::Float64) => Type::NUMBER,
            _ => continue,
        };
        let Ok(register) = Register::try_from(register) else { continue };
        if parameters.contains(&usize::from(register)) || locals.iter().any(|(local, _)| *local == register) { continue; }
        locals.push((register, ty));
    }
}

/// The entry types of `function`'s (`function_id`'s) parameters its AOT image's closed world proves
/// Numbers at every read and write (`AotFacts::numbers`): an int32 or a Number, as a declared
/// parameter's type enters (`scalar::declared_entry_types`), so the entry proves each once and the
/// body holds it unboxed. Nothing without a closed world (`VmFacts::closed`).
pub(crate) fn closed_entry_types(vm: VmFacts<'_>, function_id: u32, function: &quench_runtime::bytecode::Function, initial: &mut [Type]) {
    let Some(numbers) = vm.closed.and_then(|closed| closed.function_facts(function_id).numbers()) else { return };
    for index in 0..usize::from(function.params) {
        let Some(register) = super::scalar::parameter_register(function, index).map(usize::from) else { continue };
        let ty = match numbers.get(register) {
            Some(Some(Representation::Int32)) => Type::INT32,
            Some(Some(Representation::Float64)) => Type::NUMBER,
            _ => continue,
        };
        if let Some(entry) = initial.get_mut(register).filter(|entry| **entry == Type::ANY) {
            *entry = ty;
        }
    }
}
