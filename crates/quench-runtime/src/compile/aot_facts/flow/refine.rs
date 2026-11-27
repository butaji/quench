//! The facts a conditional jump proves on each of its two edges (`Solver::refine`): a register tested for truth or for
//! `null`/`undefined`, and the integer ranges a relational jump leaves its operands on.

use super::{Solver, conditional_target};
use super::super::effects::Before;
use super::super::value::Word;
use crate::bytecode::WideInstruction;
use crate::staged::{Comparison, Operation};
use crate::staged::typing;
use crate::typed_ir::{Cell, Classes, Elements, Type, language_bit};
use crate::ValueType;

/// What a conditional jump's test proves about the value it tests, on one of its edges.
#[derive(Clone, Copy)]
enum Refinement {
    /// The value is `null` or `undefined` (or, by a loose test, an object an IsHTMLDDA cell may be).
    Nullish,
    /// The value is neither `null` nor `undefined`.
    NotNullish,
}

impl Solver<'_> {
    /// The state the normal edge from `block` to `target` carries, from `block`'s output `out`. Edge-fact
    /// law: the jump at the end of a block proves a fact about the register it tests on each of its two
    /// edges, and that fact is proven at the test, not assumed. The refined word is carried on the value
    /// from the edge on, and every read after the edge trusts it, so `x.f` after `x != null` is one load
    /// (the true edge carries `x: C`, not `C | Null`). An edge the test proves nothing on, or a jump whose
    /// operands the state does not name with certainty, carries `out` as it is.
    pub(super) fn refine(&self, block: usize, target: usize, out: &Vec<Word>) -> Vec<Word> {
        let last = self.blocks.ranges[block].end - 1;
        let instruction = self.function.instruction_at(last);
        let Some(jump) = conditional_target(instruction) else { return out.clone() };
        let fallthrough = last + 1;
        let edge = self.blocks.ranges[target].start;
        // A jump to the next instruction has one edge that two indices name: no test tells them apart.
        if jump == fallthrough || (edge != jump && edge != fallthrough) { return out.clone(); }
        if let Some(refined) = self.refine_ranges(instruction, out, edge == jump) { return refined; }
        let Some((register, fact)) = edge_fact(instruction, out, edge == jump) else { return out.clone() };
        let word = out[register];
        let mut refined = out.clone();
        refined[register] = Word { cell: refine_cell(word.cell, fact), origin: word.origin };
        refined
    }
    /// The state an edge of a relational jump carries when its two operands are integer ranges: each keeps the range
    /// the comparison leaves it on that edge (`typing::relation_edge`). `None` where the jump compares no two integer
    /// ranges, one register against itself, or no value reaches the edge (the edge then keeps the state it has).
    fn refine_ranges(&self, instruction: WideInstruction, out: &Vec<Word>, to_target: bool) -> Option<Vec<Word>> {
        let Some(Operation::CompareJump { comparison: Comparison::Relation { relation, .. }, left, right, negate }) =
            Operation::<u64, ()>::of(instruction) else { return None };
        let operands = [left, right];
        if left.register_index().is_some() && left.register_index() == right.register_index() { return None; }
        let env = Before { ctx: self.ctx, state: out };
        let [Some(left_range), Some(right_range)] = operands.map(|operand| typing::operand_cell(&env, operand).ty.int_range())
            else { return None };
        // The jump is taken where the comparison fails when `negate`, so its target's edge holds the relation false.
        let [left_kept, right_kept] = typing::relation_edge(relation, to_target != negate, left_range, right_range)?;
        let mut refined = out.clone();
        for (operand, range) in operands.into_iter().zip([left_kept, right_kept]) {
            if let Some(register) = operand.register_index() {
                let word = refined[usize::from(register)];
                refined[usize::from(register)] = Word { cell: Cell { ty: Type::int(range), ..word.cell }, origin: word.origin };
            }
        }
        Some(refined)
    }
}

/// The fact a conditional jump's test proves about the register it tests, on its target edge (`to_target`)
/// or its fallthrough, from the state `state` the jump leaves. Recognized with certainty only:
/// - `JumpIf` on a register `r` is a truth test. Its truthy edge proves `r` not nullish; its falsy edge may
///   be `null`, `undefined`, `false`, `0` or `""`, so it proves nothing.
/// - `CompareJump` with an equality operator (`==`, `===`, `!=`, `!==`) of one register against a register
///   whose word is exactly `null` or `undefined`. The equal edge proves `r` nullish. The unequal edge of a
///   loose test proves `r` not nullish; a strict unequal edge may still be the other nullish value, so it proves nothing.
/// Every other jump, and a comparison of two registers neither of which is a nullish constant, is `None`.
fn edge_fact(instruction: WideInstruction, state: &[Word], to_target: bool) -> Option<(usize, Refinement)> {
    match Operation::<u64, ()>::of(instruction)? {
        Operation::JumpIf { register, taken } => {
            // The target is taken where `r` is truthy when `taken`, and where it is falsy when not.
            let truthy = if to_target { taken } else { !taken };
            truthy.then_some((usize::from(register), Refinement::NotNullish))
        }
        // The jump is taken where the comparison fails (`negate`), so the target's edge holds the comparison false.
        Operation::CompareJump { comparison: Comparison::Equality { strict, negated }, left, right, negate: true } => {
            let holds = !to_target;
            let equal = holds != negated;
            let (left, right) = (usize::from(left.register_index()?), usize::from(right.register_index()?));
            let register = match (is_nullish_constant(state[left]), is_nullish_constant(state[right])) {
                (false, true) => left,
                (true, false) => right,
                _ => return None,
            };
            if equal {
                // A loose `== null` also holds of a legacy heap cell an IsHTMLDDA object is, so only a type that
                // holds no such cell proves the value nullish.
                (strict || !may_be_legacy_nullish(state[register].cell.ty)).then_some((register, Refinement::Nullish))
            } else {
                (!strict).then_some((register, Refinement::NotNullish))
            }
        }
        _ => None,
    }
}

/// Whether a word is exactly `null` or exactly `undefined`: its type holds no other value, so it is that constant.
fn is_nullish_constant(word: Word) -> bool {
    word.cell.ty == Type::language(ValueType::Null) || word.cell.ty == Type::language(ValueType::Undefined)
}

/// Whether a value of type `ty` may be a legacy heap cell (an IsHTMLDDA object) that a loose `== null` holds of:
/// any type outside `null`, `undefined`, Number and Boolean, except a direct object, which no such cell is.
fn may_be_legacy_nullish(ty: Type) -> bool {
    let plain = language_bit(ValueType::Null) | language_bit(ValueType::Undefined)
        | language_bit(ValueType::Number) | language_bit(ValueType::Boolean);
    ty.direct_object_part().is_none() && ty.may_be_languages(Type::languages(!plain))
}

/// The cell a value of cell `cell` holds on an edge proving `fact`. A nullish value holds no object and no array.
fn refine_cell(cell: Cell, fact: Refinement) -> Cell {
    match fact {
        Refinement::NotNullish => Cell { ty: cell.ty.not_nullish(), ..cell },
        Refinement::Nullish => Cell { ty: cell.ty.nullish_part(), classes: Classes::Never, elements: Elements::NEVER, global: false },
    }
}
