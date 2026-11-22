//! The integer proofs of a lane unit, computed once before lowering: the int32 arithmetic whose
//! exact result fits (no overflow exit) and the hoisted element accesses whose index is
//! non-negative and below their array's length (no bounds guard), at the access or by one
//! comparison at the entry of its loop (`loop_versions`).
//!
//! A thin view over the facts analysis: one run of it models the unit (`facts::Model`: its
//! inlined frames, its dense accesses, the PCs a previous body left at) and takes the entry's
//! guarded ranges (`Lowering::entering`), so the ranges, the length relations and the proofs
//! are rows of the same fixpoint that types every register, which widens a range that keeps
//! growing instead of assuming one and checking it on the back edge.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::JsDenotation;
use super::*;
use super::super::facts::{self, Model, Observed, ObservedRead, Point, Seeds, Text};
use super::loop_versions::{Bound, Decision, Hoist, Induction, Nest, Resolved, Shape, Version};

/// The facts one analysis proves, by unit PC.
pub(crate) struct IntRanges {
    overflow_free: CompoundBitSet,
    in_bounds: CompoundBitSet,
    /// The string reads whose index the point proves below the string's length (`string_in_bounds`), by PC.
    string_bounds: CompoundBitSet,
    /// The accesses whose indices their loop's entry compares with the array's length once
    /// (`Lowering::version_loops`), by PC (ascending).
    versioned: Vec<Version>,
    /// The pure integer operations a loop's entry evaluates once (`Lowering::hoist_pure`), by PC (ascending).
    hoisted: Vec<Hoist>,
    /// The relations of registers holding no Number that a loop's entry decides once
    /// (`Lowering::hoist_decisions`), by PC (ascending).
    decisions: Vec<Decision>,
    /// The registers a loop's entry proves int32 once, against which its int32 counter is ordered
    /// (`Lowering::hoist_bounds`), by loop (ascending).
    bounds: Vec<Bound>,
    /// The products of a counter a loop's entry holds and advances (`Lowering::start_inductions`).
    inductions: Vec<Induction>,
    /// The arithmetic whose result leaves int32 within `IntRange::WORD`, by PC (ascending), with its range.
    index_ranges: Vec<(usize, IntRange)>,
    /// The products no consumer reads the sign of a zero of, by PC (ascending).
    signless: Vec<usize>,
    /// The products no operands make a negative zero of.
    zero_free: CompoundBitSet,
    /// The remainders whose dividend the point before them proves no negative, so no `-0` (`prove_at`).
    non_negative_dividend: CompoundBitSet,
    /// The remainders whose divisor the point before them proves no zero, so no zero-divisor test.
    nonzero_divisor: CompoundBitSet,
    /// The element stores whose value the point before them holds a Number of (`Point::numeric_store`).
    numeric_stores: CompoundBitSet,
}

impl IntRanges {
    pub(crate) fn new(instructions: usize) -> Self {
        Self { overflow_free: CompoundBitSet::with_capacity(instructions), in_bounds: CompoundBitSet::with_capacity(instructions),
            string_bounds: CompoundBitSet::with_capacity(instructions),
            versioned: Vec::new(), hoisted: Vec::new(), decisions: Vec::new(), bounds: Vec::new(), inductions: Vec::new(), index_ranges: Vec::new(), signless: Vec::new(), zero_free: CompoundBitSet::with_capacity(instructions),
            non_negative_dividend: CompoundBitSet::with_capacity(instructions), nonzero_divisor: CompoundBitSet::with_capacity(instructions),
            numeric_stores: CompoundBitSet::with_capacity(instructions) }
    }

    /// Whether the remainder at `pc` has a dividend no negative: its `-0` is no zero it can make.
    pub(crate) fn non_negative_dividend(&self, pc: usize) -> bool { self.non_negative_dividend.contains(pc) }

    /// Whether the element store at `pc` stores a Number, by the facts' type of its value at `pc` (a
    /// point the analysis never visits proves none).
    pub(crate) fn numeric_store(&self, pc: usize) -> bool { self.numeric_stores.contains(pc) }

    /// Whether the remainder at `pc` has a divisor no zero: its zero-divisor test is redundant.
    pub(crate) fn nonzero_divisor(&self, pc: usize) -> bool { self.nonzero_divisor.contains(pc) }

    /// Whether the product at `pc` may keep the int32 zero for a negative one: no operands make a
    /// negative zero of it, or no consumer reads its sign.
    pub(crate) fn ignores_negative_zero(&self, pc: usize) -> bool {
        self.zero_free.contains(pc) || self.signless.binary_search(&pc).is_ok()
    }

    /// The range of the int64 result of the arithmetic at `pc`, where it leaves int32 but is an
    /// integer Number of `IntRange::WORD`.
    pub(crate) fn index_range(&self, pc: usize) -> Option<IntRange> {
        self.index_ranges.binary_search_by_key(&pc, |(at, _)| *at).ok().map(|found| self.index_ranges[found].1)
    }

    /// Whether the int32 addition, subtraction, product or step at `pc` has an exact int32
    /// result, whatever int32 operands reach it.
    pub(crate) fn overflow_free(&self, pc: usize) -> bool { self.overflow_free.contains(pc) }

    /// Whether the index of the element access at `pc` lies in `0..length` of its array: proved
    /// at the access, or by its loop's entry (`versioned`).
    pub(crate) fn in_bounds(&self, pc: usize) -> bool {
        self.in_bounds.contains(pc) || self.versioned.binary_search_by_key(&pc, |version| version.pc).is_ok()
    }

    /// Whether the string read at `pc` (a `charCodeAt` call or a `base[index]` of a string) has an
    /// index below its string's length, so its row takes no bounds test (`Point::string_index_proven`).
    pub(crate) fn string_in_bounds(&self, pc: usize) -> bool { self.string_bounds.contains(pc) }

    /// The accesses whose indices their loop's entry compares with the array's length, by PC.
    pub(crate) fn versioned(&self) -> &[Version] { &self.versioned }

    /// The pure integer operations hoisted to a loop's entry, by PC.
    pub(crate) fn hoisted(&self) -> &[Hoist] { &self.hoisted }

    /// The relations decided once at a loop's entry, by PC.
    pub(crate) fn decisions(&self) -> &[Decision] { &self.decisions }

    /// The registers the loops' entries prove int32 once (`Lowering::hoist_bounds`).
    pub(crate) fn bounds(&self) -> &[Bound] { &self.bounds }

    /// The products an entry holds and advances.
    pub(crate) fn inductions(&self) -> &[Induction] { &self.inductions }

    /// Keeps what a loop's entry proves of an access: its proof, the sum of its index bounded by it
    /// (an index below its array's length, which every layout stores in 32 bits), and its product's
    /// induction.
    fn keep(&mut self, resolved: Resolved) {
        self.versioned.push(resolved.version);
        let known = IntRange { min: 0, max: quench_runtime::bytecode::MAX_ARRAY_LENGTH as i64 };
        if let Some(found) = resolved.add.and_then(|add| self.index_ranges.binary_search_by_key(&add, |(at, _)| *at).ok()) {
            let range = &mut self.index_ranges[found].1;
            *range = range.intersect(known).unwrap_or(known);
        }
        if let Some(induction) = resolved.induction.filter(|induction| self.inductions.iter().all(|held| held.multiply != induction.multiply)) {
            self.inductions.push(induction);
        }
    }
}

impl Lowering<'_, '_> {
    /// The products of `instructions` whose results only an index or a bitwise operation reads
    /// (`Demand::sign_insensitive`): a negative zero is the zero to them.
    fn signless_products(&self, instructions: &[WideInstruction]) -> Vec<usize> {
        use super::super::operations::NumberArithmetic;
        instructions.iter().enumerate().filter(|(pc, instruction)| {
            instruction.op().js_denotation() == JsDenotation::NumericBinary && !instruction.returns_from_frame()
                && instruction.result_flags() == 0
                && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()) == Some(NumberArithmetic::Multiply)
                && self.demand_from(pc + 1, usize::from(instruction.result_register())).sign_insensitive()
        }).map(|(pc, _)| pc).collect()
    }

    /// The element reads the entry observed to yield Numbers (`observed`), and the reads of a
    /// hoisted typed array of a fixed kind, each of which yields a Number of the kind's element
    /// type: an integer kind's range, a typed-array load being its elements' own integers.
    fn element_reads(&self, instructions: &[WideInstruction]) -> Vec<ObservedRead> {
        let typed: Vec<ObservedRead> = instructions.iter().enumerate().filter(|(_, instruction)| matches!(crate::compiled::performed(**instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })))
            .filter_map(|(pc, instruction)| {
                let array = self.loop_array(pc, instruction.operand_b().register_index())?;
                Some(ObservedRead { pc, ty: array.views.observed_kind().and_then(NativeKind::new)?.element_type() })
            }).collect();
        let mut reads: Vec<ObservedRead> = self.observed.0.iter().filter(|read| typed.iter().all(|seed| seed.pc != read.pc))
            .copied().chain(typed.iter().copied()).collect();
        reads.sort_by_key(|read| read.pc);
        reads
    }

    /// The entering ranges (`entering`) the loop keeps: those still narrower than int32 at the
    /// entry `entry` once its backedges join in. An entry sample is one value; where the loop
    /// writes the register out of the range (a hash, say), the header's fact is the whole int32
    /// range and the guard proves nothing, yet it leaves every later entry the sample misses.
    pub(crate) fn kept_entering(&self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        leaders: &CompoundBitSet, own_shape: &dyn Fn(usize) -> Option<(Register, u32)>) -> Vec<(Register, IntRange)> {
        if self.entering.is_empty() { return Vec::new(); }
        let dense = |pc: usize| self.hoisted_access(pc);
        let typed = |pc: usize| self.typed_access(pc);
        let model = Model { program: self.program, unit: self.unit, generic: self.generic, dense: &dense, typed: &typed };
        let reads = self.element_reads(instructions);
        let signless = self.signless_products(instructions);
        let seeds = Seeds { contracts: self.facts.contracts(), observed: Observed(&reads), entering: &self.entering, model: Some(model),
            signless: &signless };
        let text = Text { entry, instructions, leaders, constants: &self.program.constants, written: &writes, own_shape,
            native_calls: self.native_calls, raises: &self.raises };
        let facts = facts::analyze_text(&text, initial, seeds);
        let Some(header) = facts.at(entry) else { return self.entering.clone() };
        self.entering.iter().copied().filter(|&(register, _)| header.get(usize::from(register))
            .and_then(|ty| ty.int_range()).is_some_and(|range| range != IntRange::FULL)).collect()
    }

    /// The proofs of the unit entered at `entry` with the checked inputs `initial`, over `loops`, its local loops.
    pub(crate) fn prove_ranges(&self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        leaders: &CompoundBitSet, own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, loops: &[(usize, usize)]) -> IntRanges {
        let dense = |pc: usize| self.hoisted_access(pc);
        let typed = |pc: usize| self.typed_access(pc);
        let model = Model { program: self.program, unit: self.unit, generic: self.generic, dense: &dense, typed: &typed };
        let reads = self.element_reads(instructions);
        let signless = self.signless_products(instructions);
        let seeds = Seeds { contracts: self.facts.contracts(), observed: Observed(&reads), entering: &self.entering, model: Some(model),
            signless: &signless };
        let text = Text { entry, instructions, leaders, constants: &self.program.constants, written: &writes, own_shape,
            native_calls: self.native_calls, raises: &self.raises };
        let mut proved = IntRanges::new(instructions.len());
        let mut seen = Seen::default();
        facts::analyze_text(&text, initial, seeds).replay(&text, seeds, |pc, point| self.prove_at((&mut proved, &mut seen), (&model, &signless, instructions), pc, point));
        proved.signless = signless;
        if self.entry_bounds {
            self.prove_loops(&mut proved, &model, Nest { instructions, loops: &self.lowered_loops(loops), hoists: &[] }, &seen);
        }
        proved
    }

    /// What the point before the instruction at `pc` proves of it, and what the loops around it
    /// will decide (`seen`).
    fn prove_at(&self, (proved, seen): (&mut IntRanges, &mut Seen), (model, signless, instructions): (&Model<'_>, &[usize], &[WideInstruction]),
        pc: usize, point: &Point) {
        let instruction = instructions[pc];
        if point.overflow_free(&self.program.constants, instruction) { proved.overflow_free.insert(pc); }
        match point.index_proof(model, pc, instruction) {
            facts::IndexProof::Proven => { proved.in_bounds.insert(pc); }
            proof => seen.shapes.extend(self.shape(instructions, pc, point, proof)),
        }
        if let Some((receiver, index)) = self.string_read(pc, instruction)
            && point.int_range(index).is_some_and(|range| range.min >= 0) && point.string_index_proven(receiver, index) {
            proved.string_bounds.insert(pc);
        } else {
            proved.string_bounds.remove(pc);
        }
        let range = point.index_range(&self.program.constants, instruction, signless.binary_search(&pc).is_ok());
        if let Some(range) = range {
            proved.index_ranges.push((pc, range));
            if self.hoistable(pc, instruction, point) { seen.hoistable.push(pc); }
        }
        if point.numeric_store(instruction) { proved.numeric_stores.insert(pc); } else { proved.numeric_stores.remove(pc); }
        if point.zero_free(&self.program.constants, instruction) { proved.zero_free.insert(pc); }
        if let Some((dividend, divisor)) = self.remainder_ranges(point, instruction) {
            if dividend.min >= 0 { proved.non_negative_dividend.insert(pc); }
            if divisor.min > 0 || divisor.max < 0 { proved.nonzero_divisor.insert(pc); }
        }
        if self.decidable(instruction, point) { seen.decisions.push(pc); }
        if let Some(register) = self.int_bound(instruction, point) { seen.bounds.push((pc, register)); }
    }

    /// The register a relation jump at `instruction` orders against an int32 counter, where no int32
    /// proves it at the point (a Number or tagged bound): its loop's entry may prove it int32 once
    /// (`Lowering::hoist_bounds`), so the comparison is an int32 one in the loop.
    fn int_bound(&self, instruction: WideInstruction, point: &Point) -> Option<Register> {
        use quench_runtime::staged::{Comparison, Operation};
        let Some(Operation::<(), ()>::CompareJump { comparison: Comparison::Relation { .. }, left, right, .. }) =
            Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY) else { return None };
        let int32 = |register: Register| point.int_range(register).is_some_and(|range| range.fits_int32());
        let int = |operand: Operand| operand.register_index().map_or_else(|| self.int32_constant(operand).is_some(), int32);
        let bound = |operand: Operand, counter: Operand| operand.register_index()
            .filter(|&register| !int32(register) && int(counter));
        bound(left, right).or_else(|| bound(right, left))
    }

    /// The string and index register of the read at `pc` that reads a string's units at an index: a
    /// `charCodeAt` call of an intrinsic (`native_calls::reads_string`) or an element read by a register.
    fn string_read(&self, pc: usize, instruction: WideInstruction) -> Option<(quench_runtime::bytecode::Register, quench_runtime::bytecode::Register)> {
        if super::native_calls::reads_string(&self.intrinsics, pc) {
            return Some((instruction.register_c(), quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm())));
        }
        match quench_runtime::staged::Operation::<u64, ()>::of(instruction) {
            Some(quench_runtime::staged::Operation::ElementRead { base, key, .. }) => Some((base, key.register_index()?)),
            _ => None,
        }
    }

    /// The ranges of the operands of the `%` at `instruction` the point proves: a register's integer
    /// range, or an int32 constant's singleton. `None` for any other instruction or an unranged operand.
    fn remainder_ranges(&self, point: &Point, instruction: WideInstruction) -> Option<(IntRange, IntRange)> {
        if instruction.op().js_denotation() != JsDenotation::NumericBinary || instruction.binary_operator() != quench_runtime::vm::operations::REMAINDER {
            return None;
        }
        let range = |operand: Operand| operand.register_index().map_or_else(
            || self.int32_constant(operand).map(|value| IntRange::singleton(i64::from(value))),
            |register| point.int_range(register));
        Some((range(instruction.operand_b())?, range(instruction.operand_c())?))
    }

    /// Whether the jump `instruction` is a relation the VM decides from two registers one of which
    /// holds no Number here (`Comparison::Decided`): a pure decision of values no loop changes
    /// where no instruction of it writes them, so its entry may decide it once.
    fn decidable(&self, instruction: WideInstruction, point: &Point) -> bool {
        use quench_runtime::staged::{Comparison, Operation};
        let Some(Operation::<(), ()>::CompareJump { comparison: Comparison::Relation { .. }, left, right, .. }) =
            Operation::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY) else { return false };
        let registers = [left, right].map(Operand::register_index);
        !self.helpers.is_numeric() && registers.iter().all(Option::is_some)
            && registers.iter().flatten().any(|register| point.numberless(*register))
    }

    /// What the entries of `nest`'s loops prove of what `seen`: the operations each evaluates once, and
    /// the accesses each proves within their arrays.
    fn prove_loops(&self, proved: &mut IntRanges, model: &Model<'_>, nest: Nest<'_>, seen: &Seen) {
        proved.hoisted = self.plan_hoists(nest, &seen.hoistable);
        proved.decisions = self.plan_decisions(nest, &seen.decisions);
        proved.bounds = self.plan_bounds(nest, &seen.bounds);
        let hoists = proved.hoisted.clone();
        let nest = Nest { hoists: &hoists, ..nest };
        for shape in &seen.shapes {
            if let Some(resolved) = self.resolve((nest, &*proved), model, shape) { proved.keep(resolved); }
        }
        proved.versioned.sort_by_key(|version| version.pc);
    }
}

/// What the analysis's points leave to the loops: the accesses whose indices a loop's entry may prove
/// (`Shape`) and the arithmetic a loop's entry may evaluate (`Lowering::hoistable`), by PC.
#[derive(Default)]
struct Seen {
    shapes: Vec<Shape>,
    hoistable: Vec<usize>,
    decisions: Vec<usize>,
    /// The relation jumps' PCs with the register each orders against an int32 counter (`int_bound`).
    bounds: Vec<(usize, Register)>,
}
