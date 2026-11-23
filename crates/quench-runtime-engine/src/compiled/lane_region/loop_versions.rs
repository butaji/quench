//! What a loop's entering edge establishes once for its body, over the facts rows (`IntRanges`):
//! the pure integer operations its operands leave invariant (`hoists`), the registers its int32
//! counters are ordered against, proved int32 once (`hoist_bounds`), the bounds of its accesses
//! (below), and the products of its counters it advances by a step instead (`inductions`).
//!
//! Bounds by versioning: an element access in a counted loop whose index is `base + counter * scale`
//! for a `counter` below a bound the loop never writes (`i < n`), and a base and scale it never
//! writes, reaches `base + low * scale` to `base + (bound - 1) * scale` and nothing between, which
//! are within the array when its two ends are. The loop's entry compares the two ends with the
//! array's length once and the accesses of its body lower with their index proved
//! (`IntRanges::in_bounds`), where each tested it per iteration.
//!
//! The comparison is the entry's, so a failed one is the entry's own exact-PC leave: the
//! interpreter runs the loop, which tests every access. A loop that can change an array's length
//! (any instruction `Model::disturbs_lengths`) keeps its per-access tests, and one that a previous
//! body left at its entry (`generic`) compiles again without versioning: a bound that exceeds its
//! array on most entries repays no guard.
//!
//! The words never wrap: a counter and a bound are int32, a scale is an int32 register or constant,
//! and a base is an int32 or an index word of `IntRange::EXACT`, so `base + counter * scale` stays
//! within `i64` and the entry's arithmetic is exact.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::Stage;

#[path = "loop_versions/plan.rs"] pub(crate) mod plan;
#[path = "loop_versions/hoists.rs"] pub(crate) mod hoists;
#[path = "loop_versions/inductions.rs"] pub(crate) mod inductions;
pub(crate) use plan::{Nest, Resolved, Shape};

/// A loop-invariant integer an index is made of, held as a 64-bit word on its checking loop's
/// entering edge.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Term {
    /// An int32 register no instruction of the loop writes.
    Register(Register),
    /// An int32 constant.
    Constant(i32),
    /// The word of the pure operation at this PC, which an enclosing loop's entry evaluated (`Hoist`).
    Hoisted(usize),
}

impl Term {
    /// The register the term reads on the loop's entering edge.
    fn register(self) -> Option<Register> { if let Self::Register(register) = self { Some(register) } else { None } }

    /// The PC of the operation whose word the term is.
    fn hoisted(self) -> Option<usize> { if let Self::Hoisted(pc) = self { Some(pc) } else { None } }
}

/// The indices an access reaches over its loop: `base + counter * scale` for the counter in
/// `low..bound`, where `low` is the counter's least value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Reach {
    /// `base + counter * scale` for the counter in `low..bound`.
    Counted { bound: Register, low: i64, base: Option<Term>, scale: Option<Term> },
    /// Every index in `0..=ceiling`, whatever the loop does (a mask or a range of the index).
    Ranged { ceiling: i64 },
}

/// An access whose indices the entry of the loop at `header` proves within the access's array.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Version {
    pub(crate) pc: usize,
    pub(crate) header: usize,
    pub(crate) reach: Reach,
}

/// The pure integer operation at `pc`, which the entry of the loop at `header` (the outermost one
/// whose body writes none of its operands) evaluates once.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hoist {
    pub(crate) pc: usize,
    pub(crate) header: usize,
}

/// The relation jump at `pc` the VM decides (`Comparison::Decided`), which the entry of the loop at
/// `header` decides once from `sources`: its operand registers, or for one the loop copies into
/// before the jump in the jump's own block (`Move`), the register it copies, which the loop
/// does not write.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Decision {
    pub(crate) pc: usize,
    pub(crate) header: usize,
    pub(crate) sources: [Register; 2],
}

/// The register an int32 counter is ordered against in a loop, which the loop's entry proves int32
/// once (`Lowering::hoist_bounds`): no instruction of the loop writes it, and no int32 proves it at
/// the relation. Its entry guard leaves at `header` where the register is no int32.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Bound {
    pub(crate) header: usize,
    pub(crate) register: Register,
}

/// The product at `multiply` of `counter` and `scale`, which the entry of the loop `span` holds as a
/// word that advances by `scale` at each step of the counter (every write of it in the loop).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Induction {
    pub(crate) multiply: usize,
    pub(crate) counter: Register,
    pub(crate) scale: Term,
    pub(crate) span: (usize, usize),
}

impl Lowering<'_, '_> {
    /// The reads `header`'s loop repeats unchanged, made on its entry edge from `state`: a guard that
    /// fails there leaves at the header, before the loop runs. Its pure integer operations come first,
    /// then the views of its arrays and its other invariant reads, then what they make provable.
    pub(crate) fn hoist_entry(&mut self, state: &mut State, header: usize) {
        self.hoist_pure(state, header);
        self.hoist_decisions(state, header);
        self.hoist_bounds(state, header);
        // Before the views: a scalar native's allocation reloads the arena the views' bases come from.
        self.hoist_scalar_calls(state, header);
        self.hoist_arrays(state, header, false);
        self.hoist_strings(state, header);
        self.hoist_stable(state, header);
        self.hoist_receivers(state, header);
        self.hoist_reads(state, header);
        self.hoist_arrays(state, header, true);
        self.version_loops(state, header);
        self.start_inductions(state, header);
        self.hoist_constructions(header);
    }

    /// The comparison of each reach of the versioned accesses of the loop at `header` with the length
    /// of the array they index, on its entering edge: leaves at the header where it passes the
    /// array's length (a bound that leaves no iteration to test passes).
    fn version_loops(&mut self, state: &mut State, header: usize) {
        let mut compared: Vec<(ArraySource, Reach)> = Vec::new();
        let mut exit = None;
        let versions: Vec<Version> = self.ranges.versioned().iter().copied().filter(|version| version.header == header).collect();
        for version in versions {
            let access = self.unit.instructions[version.pc];
            let base = if matches!(crate::compiled::performed(access), Some(quench_runtime::staged::Operation::ElementRead { .. })) { access.operand_b().register_index() } else { Some(access.register_b()) };
            let array = self.loop_array(version.pc, base).expect("a versioned access indexes a hoisted array");
            if compared.contains(&(array.source, version.reach)) { continue; }
            compared.push((array.source, version.reach));
            let exit = *exit.get_or_insert_with(|| self.guard_exit(state, header as u32));
            self.check_reach(state, array, version.reach, exit);
        }
    }

    /// Continues where both ends of `reach` lie within `array`'s length (and, where a scaled counter's
    /// word is held, within `IntRange::EXACT`), or where its counter has no value; else at `exit`.
    fn check_reach(&mut self, state: &mut State, array: LoopArray, reach: Reach, exit: ir::Block) {
        let (_, length) = self.hoisted_view(array);
        let length = emit(&mut self.builder).uextend32(length);
        let proved = match reach {
            Reach::Ranged { ceiling } => {
                // Every index is at most `ceiling`: within the array when the length exceeds it.
                let ceiling = self.raw_const(Lane::I64, ceiling);
                self.raw_compare(Relation::UnsignedLess, ceiling, length)
            }
            Reach::Counted { bound, low, base, scale } => {
                let bound = self.entry_register(state, bound, exit);
                let low = self.raw_const(Lane::I64, low);
                let last = self.raw_binary_imm(Binary::Add, bound, -1);
                let base = base.map(|term| self.entry_term(state, term, exit));
                let scale = scale.map(|term| self.entry_term(state, term, exit));
                let ends = [low, last].map(|counter| self.reach_at(counter, base, scale));
                let inside = ends.map(|(index, _)| self.raw_compare(Relation::UnsignedLess, index, length));
                let mut within = self.raw_binary(Binary::And, inside[0], inside[1]);
                for (_, scaled) in ends.into_iter().filter(|_| scale.is_some()) {
                    let exact = self.within_exact(scaled);
                    within = self.raw_binary(Binary::And, within, exact);
                }
                let empty = self.raw_compare(Relation::SignedLessEqual, bound, low);
                self.raw_binary(Binary::Or, empty, within)
            }
        };
        emit_js_guard(&mut self.builder, proved, exit);
    }

    /// `base + counter * scale` and `counter * scale`, in words.
    fn reach_at(&mut self, counter: ir::Value, base: Option<ir::Value>, scale: Option<ir::Value>) -> (ir::Value, ir::Value) {
        let scaled = match scale { Some(scale) => self.raw_binary(Binary::Mul, counter, scale), None => counter };
        let index = match base { Some(base) => self.raw_binary(Binary::Add, base, scaled), None => scaled };
        (index, scaled)
    }

    /// `register`'s int32 as a signed 64-bit word, proved on the entering edge.
    fn entry_register(&mut self, state: &mut State, register: Register, exit: ir::Block) -> ir::Value {
        let int = self.int(state, register, exit);
        self.raw_unary(Unary::SignExtend(Lane::I64), int)
    }

    /// `term`'s word on the entering edge of the loop it is a term of.
    fn entry_term(&mut self, state: &mut State, term: Term, exit: ir::Block) -> ir::Value {
        match term {
            Term::Register(register) => self.entry_register(state, register, exit),
            Term::Constant(value) => self.raw_const(Lane::I64, i64::from(value)),
            Term::Hoisted(pc) => {
                let read = self.hoisted.get(&pc).copied().expect("an enclosing loop's entry evaluated the operation");
                self.builder.use_var(read.value)
            }
        }
    }
}
