//! Loop inputs an entry observed to be int32s: the invariants of an OSR loop that its index
//! arithmetic reads, which the entry proves int32 once, so the arithmetic is integer words
//! (`index_math`). No magnitude is assumed of them: the words leave only where a result really
//! passes what a Number holds exactly (`IntRange::EXACT`).

use super::*;
use super::super::operations::NumberArithmetic;

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
                && self.int_input(*register) && self.is_live(header, *register)).collect();
        inputs.sort_unstable();
        inputs.dedup();
        inputs
    }

    /// Whether `register` may enter as an int32: an entry this compilation observed, or, for code
    /// lowered before any run (`EntryInts::Hypothesized`), any input — conformance checks the
    /// assumed int32 once at the entry and leaves where the value is not one.
    fn int_input(&self, register: Register) -> bool {
        self.entry_ints == EntryInts::Hypothesized
            || self.int_entries.iter().any(|(entry, _)| *entry == register)
    }

    /// Takes each input of the loop's index arithmetic (`index_inputs`) for an int32 contract of
    /// its entering edge: proved once there, the loop's index arithmetic runs on integers instead
    /// of converting a Number per use. Whether it took any.
    pub(super) fn seed_loop_ints(&self, initial: &mut [Type], loops: &[(usize, usize)]) -> bool {
        let mut seeded = false;
        let mut inputs = self.index_inputs(loops);
        // A hypothesized entry may also take the function's numeric parameters: a loop's index
        // may be re-initialized from one inside an enclosing loop (`let i = lo;`), which the
        // analysis of this loop's region reaches through the enclosing back edge, and a Number
        // there would widen the index at this loop's header. Conformance checks each once.
        if self.entry_ints == EntryInts::Hypothesized {
            inputs.extend(self.numeric_parameters());
            inputs.sort_unstable();
            inputs.dedup();
            // A scan's bound (`a[i] < p`) travels as a Number across the loop: the comparison
            // reads a double, so an int seed would box it back to a tagged value before each
            // backward edge and unbox it again per use. Conformance takes the Number once.
            let numbers = self.relation_numbers(loops);
            if std::env::var_os("SEED_TRACE").is_some() {
                eprintln!("NUMBERS entry={} numbers={numbers:?} header_r14={:?}",
                    self.coverage.entry(), self.facts.at(self.coverage.entry()).and_then(|t| t.get(14)).copied());
            }
            for register in numbers {
                let slot = &mut initial[usize::from(register)];
                if slot.is_any_or_number() && *slot != Type::INT32 {
                    *slot = Type::NUMBER;
                    seeded = true;
                }
            }
        }
        for register in inputs {
            let slot = &mut initial[usize::from(register)];
            if slot.is_any_or_number() {
                *slot = Type::INT32;
                seeded = true;
            }
        }
        seeded
    }

    /// The loop-invariant operand of a scan's relation (`a[i] < p`) whose comparison reads a
    /// Number: taken as a Float64 at the entry, so no use tags or untags it per iteration.
    fn relation_numbers(&self, loops: &[(usize, usize)]) -> Vec<Register> {
        let header = self.coverage.entry();
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return Vec::new() };
        let body = &self.unit.instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|instruction| writes(*instruction)).collect();
        let mut found = Vec::new();
        for instruction in body {
            if !instruction.op().class().eq(Class::JumpBinaryFalse) { continue; }
            if super::super::operations::NumberRelation::from_operator(instruction.binary_operator_field()).is_none() { continue; }
            let operands = [instruction.operand_b(), instruction.operand_c()];
            let element = |operand: &Operand| operand.register_index().is_some_and(|register|
                body.iter().any(|other| other.op().class().eq(Class::GetIndex) && other.result_register() == register));
            let reads: Vec<bool> = operands.iter().map(element).collect();
            if reads.iter().filter(|read| **read).count() != 1 { continue; }
            for (operand, read) in operands.iter().zip(&reads) {
                if *read { continue; }
                let Some(register) = operand.register_index() else { continue };
                if written.contains(&register) || !self.is_live(header, register) { continue; }
                found.push(register);
            }
        }
        found
    }

    /// The loop-invariant operands a loop's relations compare, taken as Numbers at the header:
    /// a register the loop does not write whose header fact is untyped, compared against an
    /// element read (`a[i] < p`). The comparison then reads an unboxed double, and neither the
    /// NaN test nor the exactness round trip is paid per iteration. The contract is a hypothesis
    /// the lowering discharges at the header (`conform` proves Number once, leaving otherwise).
    pub(super) fn number_contracts(&self, instructions: &[WideInstruction], loops: &[(usize, usize)],
        header: usize) -> Vec<super::super::facts::Contract> {
        let Some((_, back)) = loops.iter().copied().filter(|(start, _)| *start == header)
            .max_by_key(|(_, back)| *back) else { return Vec::new() };
        let body = &instructions[header..=back];
        let written: BTreeSet<Register> = body.iter().flat_map(|instruction| writes(*instruction)).collect();
        let mut found: Vec<super::super::facts::Contract> = Vec::new();
        for instruction in body {
            if !instruction.op().class().eq(Class::JumpBinaryFalse) { continue; }
            if super::super::operations::NumberRelation::from_operator(instruction.binary_operator_field()).is_none() { continue; }
            for operand in [instruction.operand_b(), instruction.operand_c()] {
                let Some(register) = operand.register_index() else { continue };
                if written.contains(&register) || !self.is_live(header, register) { continue; }
                if found.iter().any(|(_, known, _)| *known == register) { continue; }
                let ty = self.facts.at(header).and_then(|types| types.get(usize::from(register))).copied()
                    .unwrap_or(Type::ANY);
                if ty.is_any_or_number() && !ty.is_int32() {
                    found.push((header, register, Type::NUMBER));
                }
            }
        }
        found
    }

    /// The function's parameters no instruction of the unit uses in an object position — a
    /// receiver or index base, a callee, a construction's target. Anywhere else a parameter may
    /// be an int32 (an operand, a copy, a passed argument), and a hypothesized entry takes it as
    /// one, which conformance checks once.
    fn numeric_parameters(&self) -> Vec<Register> {
        let Some(function) = self.program.function_get(self.function_id as usize) else { return Vec::new() };
        let object_position = |instruction: &WideInstruction, register: Register| {
            let class = instruction.op().class();
            let base = matches!(class, Class::GetField | Class::GetIndex | Class::SetIndex)
                && instruction.operand_b().register_index() == Some(register);
            let callee = matches!(class, Class::Call | Class::CallKnown | Class::Construct | Class::LoadNameCall)
                && instruction.register_b() == register;
            base || callee
        };
        (0..function.params).filter(|register| {
            !self.unit.instructions.iter().any(|instruction| object_position(instruction, *register))
        }).collect()
    }

    /// The observed int32 entries live at `entry` (a dead register's value is stale, and nothing
    /// conforms it) whose range is narrower than int32 and whose entry fact is int32: what the
    /// entry guards once (`guard_entering_ranges`) and the ranges pass assumes.
    pub(super) fn entering_ranges(&self, entry: usize) -> Vec<(Register, IntRange)> {
        let types = self.facts.at(entry);
        self.int_entries.iter().copied().filter(|&(register, range)| range != IntRange::FULL && self.is_live(entry, register)
            && types.and_then(|types| types.get(usize::from(register))).is_some_and(|ty| ty.is_subtype(Type::INT32))).collect()
    }

    /// Leaves at `pc` where an entering register is outside its range: the guard the ranges pass's
    /// entry state relies on, of each bound tighter than int32's.
    pub(super) fn guard_entering_ranges(&mut self, state: &mut State, pc: usize) {
        for (register, range) in self.entering.clone() {
            let exit = self.guard_exit(state, pc as u32);
            let value = self.int(state, register, exit);
            if range.min > IntRange::FULL.min {
                let above = self.raw_compare_imm(Relation::SignedGreaterEqual, value, range.min);
                emit_js_guard(&mut self.builder, above, exit);
            }
            if range.max < IntRange::FULL.max {
                let below = self.raw_compare_imm(Relation::SignedLessEqual, value, range.max);
                emit_js_guard(&mut self.builder, below, exit);
            }
        }
    }
}

/// Whether `instruction` is an addition, subtraction or multiplication of Numbers.
fn is_integral_arithmetic(instruction: WideInstruction) -> bool {
    instruction.op().js_denotation() == JsDenotation::NumericBinary && !instruction.returns_from_frame()
        && instruction.result_flags() == 0
        && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator())
            .is_some_and(NumberArithmetic::integral)
}
