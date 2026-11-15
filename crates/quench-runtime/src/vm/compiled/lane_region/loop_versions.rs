//! What a loop's entering edge establishes once for its body over the facts rows: the pure integer
//! operations its operands leave invariant (`hoists`), and the bounds of its accesses.
//!
//! Bounds by loop versioning: an element access in a counted loop whose index is below a bound
//! the loop never writes (`i < n`) is within its array when the bound is, so the loop's entry
//! compares the bound with the array's length once (`n <= length`) and the accesses of its body
//! lower with their index proved (`IntRanges::in_bounds`), where each tested it per iteration.
//!
//! The comparison is the entry's, so a failed one is the entry's own exact-PC leave: the
//! interpreter runs the loop, which tests every access. A loop that can change an array's length
//! (any instruction `Model::disturbs_lengths`) keeps its per-access tests, and one that a previous
//! body left at its entry (`generic`) compiles again without versioning: a bound that exceeds its
//! array on most entries repays no guard.

use super::*;
use super::super::facts::{Model, Point};
use crate::staged::Stage;

#[path = "loop_versions/plan.rs"] mod plan;
#[path = "loop_versions/hoists.rs"] mod hoists;
pub(super) use plan::Nest;

/// The pure integer operation at `pc`, which the entry of the loop at `header` (the outermost one
/// whose body writes none of its operands) evaluates once.
#[derive(Clone, Copy, Debug)]
pub(super) struct Hoist {
    pub(super) pc: usize,
    pub(super) header: usize,
}

impl Lowering<'_, '_> {
    /// The reads `header`'s loop repeats unchanged, made on its entry edge from `state`: a guard that
    /// fails there leaves at the header, before the loop runs. Its pure integer operations come first.
    pub(super) fn hoist_entry(&mut self, state: &mut State, header: usize) {
        self.hoist_pure(state, header);
        self.hoist_loop_entry(state, header);
    }

    /// The access at `pc` whose index is `base + iv` (an addition just before it, in its block)
    /// with `iv` below a bound (`i < n`) and both non-negative here: below `base + bound`, which
    /// the loop's entry may compare with the array's length once. Its array, bound and base.
    pub(super) fn affine_bound(&self, instructions: &[WideInstruction], pc: usize, point: &Point)
        -> Option<(usize, Register, (Register, Option<Register>))> {
        use super::super::operations::NumberArithmetic;
        let access = instructions[pc];
        let (array, index) = if access.op().is_get_index() { (access.operand_b().register_index()?, access.operand_c().register_index()?) }
            else if access.op().is_set_index() { (access.register_b(), access.register_c()) } else { return None };
        let mut at = pc;
        let definition = loop {
            if at == 0 || self.blocks[at].is_some() { return None; }
            at -= 1;
            if writes(instructions[at]).contains(&index) { break instructions[at]; }
        };
        let add = definition.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary && !definition.returns_from_frame()
            && definition.result_flags() == 0
            && NumberArithmetic::from_js_instruction(definition.op(), definition.binary_operator()) == Some(NumberArithmetic::Add);
        let [left, right] = [definition.operand_b().register_index()?, definition.operand_c().register_index()?];
        let (base, step) = if point.under_of(right).is_some() { (left, right) } else { (right, left) };
        let bound = point.under_of(step).filter(|bound| *bound != array && *bound != base)?;
        let unchanged = instructions[at + 1..pc].iter().all(|between| !writes(*between).contains(&base));
        (add && unchanged && base != index && step != index && point.non_negative(base) && point.non_negative(step))
            .then_some((pc, array, (bound, Some(base))))
    }

    /// The accesses of `candidates` (PC, the register holding the array, the bound and the base of
    /// an affine index) whose loop can be versioned, by PC with the bound and base: the array is a
    /// register's (hoisted on the loop's entering edge), no instruction of the loop writes the
    /// bound or base or may change a length, and both are int32 there.
    pub(super) fn versioned_bounds(&self, model: &Model<'_>, instructions: &[WideInstruction],
        candidates: &[(usize, Register, (Register, Option<Register>))]) -> Vec<(usize, Register, Option<Register>)> {
        candidates.iter().filter(|&&(pc, array, (bound, base))| self.version_holds(model, instructions, pc, (array, bound, base)))
            .map(|&(pc, _, (bound, base))| (pc, bound, base)).collect()
    }

    /// Whether the access at `pc` of the array held in `base` below `bound` (plus an affine base) can be versioned.
    fn version_holds(&self, model: &Model<'_>, instructions: &[WideInstruction], pc: usize,
        (array_base, bound, base): (Register, Register, Option<Register>)) -> bool {
        let Some(array) = self.loop_array(pc, Some(array_base)).filter(|array| matches!(array.source, ArraySource::Register(_))) else { return false };
        let span = array.header..=array.back;
        let int32 = |register: Register| self.facts.at(array.header).and_then(|types| types.get(usize::from(register)))
            .is_some_and(|ty| ty.is_subtype(Type::INT32))
            && !instructions[span.clone()].iter().any(|instruction| writes(*instruction).contains(&register));
        !self.generic.contains(&array.header) && int32(bound) && base.is_none_or(int32)
            && span.clone().all(|at| !model.disturbs_lengths(at, instructions[at]))
    }

    /// The comparison of each bound of the versioned accesses of the loop at `header` (plus the base
    /// of an affine index) with the length of the array they index, on its entering edge: leaves at
    /// `exit` where it passes its array's length (a bound below zero leaves no iteration to test).
    pub(super) fn version_bounds(&mut self, state: &mut State, header: usize, exit: ir::Block) {
        let mut compared: Vec<(ArraySource, Register, Option<Register>)> = Vec::new();
        for (pc, bound, base) in self.ranges.versioned().to_vec() {
            let instruction = self.unit.instructions[pc];
            let array_base = if instruction.op().is_get_index() { instruction.operand_b().register_index() } else { Some(instruction.register_b()) };
            let Some(array) = self.loop_array(pc, array_base).filter(|array| array.header == header) else { continue };
            if compared.contains(&(array.source, bound, base)) { continue; }
            compared.push((array.source, bound, base));
            let (_, length) = self.hoisted_view(array);
            let mut reach = self.entry_long(state, bound, exit);
            if let Some(base) = base {
                let base = self.entry_long(state, base, exit);
                reach = self.raw_binary(Binary::Add, base, reach);
            }
            let length = emit(&mut self.builder).uextend32(length);
            let within = self.raw_compare(Relation::SignedLessEqual, reach, length);
            emit_js_guard(&mut self.builder, within, exit);
        }
    }

    /// `register`'s int32 as a signed 64-bit word, proved on the entering edge.
    fn entry_long(&mut self, state: &mut State, register: Register, exit: ir::Block) -> ir::Value {
        let int = self.int(state, register, exit);
        self.raw_unary(Unary::SignExtend(Lane::I64), int)
    }
}
