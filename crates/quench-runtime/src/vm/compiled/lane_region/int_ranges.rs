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

use super::*;
use super::super::facts::{self, Model, Observed, ObservedRead, Point, Seeds, Text};
use super::loop_versions::{Hoist, Induction, Nest, Resolved, Shape, Version};

/// The facts one analysis proves, by unit PC.
pub(super) struct IntRanges {
    overflow_free: CompoundBitSet,
    in_bounds: CompoundBitSet,
    /// The accesses whose indices their loop's entry compares with the array's length once
    /// (`Lowering::version_loops`), by PC (ascending).
    versioned: Vec<Version>,
    /// The pure integer operations a loop's entry evaluates once (`Lowering::hoist_pure`), by PC (ascending).
    hoisted: Vec<Hoist>,
    /// The products of a counter a loop's entry holds and advances (`Lowering::start_inductions`).
    inductions: Vec<Induction>,
    /// The arithmetic whose result leaves int32 within `IntRange::WORD`, by PC (ascending), with its range.
    index_ranges: Vec<(usize, IntRange)>,
    /// The products no consumer reads the sign of a zero of, by PC (ascending).
    signless: Vec<usize>,
    /// The products no operands make a negative zero of.
    zero_free: CompoundBitSet,
}

impl IntRanges {
    pub(super) fn new(instructions: usize) -> Self {
        Self { overflow_free: CompoundBitSet::with_capacity(instructions), in_bounds: CompoundBitSet::with_capacity(instructions),
            versioned: Vec::new(), hoisted: Vec::new(), inductions: Vec::new(), index_ranges: Vec::new(), signless: Vec::new(), zero_free: CompoundBitSet::with_capacity(instructions) }
    }

    /// Whether the product at `pc` may keep the int32 zero for a negative one: no operands make a
    /// negative zero of it, or no consumer reads its sign.
    pub(super) fn ignores_negative_zero(&self, pc: usize) -> bool {
        self.zero_free.contains(pc) || self.signless.binary_search(&pc).is_ok()
    }

    /// The range of the int64 result of the arithmetic at `pc`, where it leaves int32 but is an
    /// integer Number of `IntRange::WORD`.
    pub(super) fn index_range(&self, pc: usize) -> Option<IntRange> {
        self.index_ranges.binary_search_by_key(&pc, |(at, _)| *at).ok().map(|found| self.index_ranges[found].1)
    }

    /// Whether the int32 addition, subtraction, product or step at `pc` has an exact int32
    /// result, whatever int32 operands reach it.
    pub(super) fn overflow_free(&self, pc: usize) -> bool { self.overflow_free.contains(pc) }

    /// Whether the index of the element access at `pc` lies in `0..length` of its array: proved
    /// at the access, or by its loop's entry (`versioned`).
    pub(super) fn in_bounds(&self, pc: usize) -> bool {
        self.in_bounds.contains(pc) || self.versioned.binary_search_by_key(&pc, |version| version.pc).is_ok()
    }

    /// The accesses whose indices their loop's entry compares with the array's length, by PC.
    pub(super) fn versioned(&self) -> &[Version] { &self.versioned }

    /// The pure integer operations hoisted to a loop's entry, by PC.
    pub(super) fn hoisted(&self) -> &[Hoist] { &self.hoisted }

    /// The products an entry holds and advances.
    pub(super) fn inductions(&self) -> &[Induction] { &self.inductions }

    /// Keeps what a loop's entry proves of an access: its proof, the sum of its index bounded by it
    /// (an index below its array's length, which every layout stores in 32 bits), and its product's
    /// induction.
    fn keep(&mut self, resolved: Resolved) {
        self.versioned.push(resolved.version);
        let known = IntRange { min: 0, max: crate::bytecode::MAX_ARRAY_LENGTH as i64 };
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
        let typed: Vec<ObservedRead> = instructions.iter().enumerate().filter(|(_, instruction)| instruction.op().is_get_index())
            .filter_map(|(pc, instruction)| {
                let array = self.loop_array(pc, instruction.operand_b().register_index())?;
                Some(ObservedRead { pc, ty: array.views.observed_kind().and_then(NativeKind::new)?.element_type() })
            }).collect();
        let mut reads: Vec<ObservedRead> = self.observed.0.iter().filter(|read| typed.iter().all(|seed| seed.pc != read.pc))
            .copied().chain(typed.iter().copied()).collect();
        reads.sort_by_key(|read| read.pc);
        reads
    }

    /// The proofs of the unit entered at `entry` with the checked inputs `initial`, over `loops`, its local loops.
    pub(super) fn prove_ranges(&self, entry: usize, initial: &[Type], instructions: &[WideInstruction],
        leaders: &CompoundBitSet, own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, loops: &[(usize, usize)]) -> IntRanges {
        let dense = |pc: usize| self.hoisted_access(pc);
        let model = Model { program: self.program, unit: self.unit, generic: self.generic, dense: &dense };
        let reads = self.element_reads(instructions);
        let signless = self.signless_products(instructions);
        let seeds = Seeds { contracts: self.facts.contracts(), observed: Observed(&reads), entering: &self.entering, model: Some(model),
            signless: &signless };
        let text = Text { entry, instructions, leaders, constants: &self.program.constants, written: &writes, own_shape,
            native_calls: self.native_calls };
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
        let range = point.index_range(&self.program.constants, instruction, signless.binary_search(&pc).is_ok());
        if let Some(range) = range {
            proved.index_ranges.push((pc, range));
            if self.hoistable(pc, instruction, point) { seen.hoistable.push(pc); }
        }
        if point.zero_free(&self.program.constants, instruction) { proved.zero_free.insert(pc); }
    }

    /// What the entries of `nest`'s loops prove of what `seen`: the operations each evaluates once, and
    /// the accesses each proves within their arrays.
    fn prove_loops(&self, proved: &mut IntRanges, model: &Model<'_>, nest: Nest<'_>, seen: &Seen) {
        proved.hoisted = self.plan_hoists(nest, &seen.hoistable);
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
}
