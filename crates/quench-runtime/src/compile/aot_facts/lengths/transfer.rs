//! The transfer of the relations over one instruction: what a step may resize (`Resize`), the relations a conditional
//! jump proves on its edge (`refine`), and the constant steps and shifts an instruction makes of its registers
//! (`step_of_constant`, `sum_of_registers`, `shift_kind`, `moves_bits`).

use super::super::effects::{Ctx, Events, runs_guest_code};
use super::super::value::{Key, Origin, Word};
use super::super::FunctionFacts;
use super::relations::{Relation, bound_of, complete_arrays, holds_array, is_string, proves_below};
use super::Lengths;
use crate::bytecode::{Operand, OperandKind, Register, WideInstruction};
use crate::typed_ir::{ElementClass, Length, Type};
use crate::staged::{Callee, Comparison, Invocation, Operation};
use crate::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// What a step may resize: no array (`Nothing`), every array (`Every`: a mutator, or guest code the solve does not follow), or the
/// arrays of the element classes the call at a point may write (`Calls`, `Types::call_writes`).
#[derive(Clone, Copy)]
pub(super) enum Resize<'a, 'b> {
    Nothing,
    Every,
    /// A call: it resizes the arrays of the classes its settled effects may write (`FunctionFacts::may_write_elements_of`).
    Calls { facts: &'a FunctionFacts<'b>, pc: u32 },
}

impl Resize<'_, '_> {
    /// Whether the step may resize some array, of any class.
    fn may_resize(self) -> bool {
        match self {
            Self::Nothing => false,
            Self::Every => true,
            Self::Calls { facts, pc } => facts.may_write_any_elements(pc),
        }
    }

    /// Whether the step may resize the array `word` holds.
    pub(super) fn touches(self, word: &Word) -> bool {
        match self {
            Self::Nothing => false,
            Self::Every => true,
            Self::Calls { facts, pc } => facts.may_write_elements_of(pc, word.elements_key()),
        }
    }
}

/// What the step at `pc` may resize, read from the facts before it (`words`): a mutator resizes its receiver's array; an element
/// access resizes through what `element_runs_guest_code` finds; a call resizes what its settled effects may write; any other
/// step that runs guest code resizes every array.
fn resize_of<'a, 'b>(ctx: &Ctx<'_>, facts: &'a FunctionFacts<'b>, pc: usize, instruction: WideInstruction, words: &[Word]) -> Resize<'a, 'b> {
    let mutator = Invocation::of(instruction, ctx.decode).is_some_and(|invocation| match invocation.callee {
        Callee::Method { atom, .. } => ctx.scope.is_array_mutator(atom),
        Callee::Register(register) => matches!(words[usize::from(register)].origin,
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) if ctx.scope.is_array_mutator(atom)),
        Callee::Known(_) => false,
    });
    if mutator {
        Resize::Every
    } else if element_op(instruction) {
        if element_runs_guest_code(facts, instruction, words) { Resize::Every } else { Resize::Nothing }
    } else if Invocation::of(instruction, ctx.decode).is_some() {
        Resize::Calls { facts, pc: pc as u32 }
    } else if runs_guest_code(ctx, instruction, words) {
        Resize::Every
    } else {
        Resize::Nothing
    }
}

/// Whether `instruction` is an element access: a `GetIndex`, or an element read or write.
fn element_op(instruction: WideInstruction) -> bool {
    instruction.op() == crate::bytecode::Op::GetIndex
        || matches!(Operation::<u64, ()>::of(instruction), Some(Operation::ElementRead { .. } | Operation::ElementWrite { .. }))
}

/// Whether the element access `instruction` runs guest code on `words`, the facts before it: through an index accessor, which the
/// world may define (`Scope::index_accessors_absent`), or through the conversion of a key that is no Number or string.
fn element_runs_guest_code(facts: &FunctionFacts<'_>, instruction: WideInstruction, words: &[Word]) -> bool {
    let plain_key = element_key(instruction).is_some_and(|key| words.get(usize::from(key)).is_some_and(|word| word.is_number() || is_string(word)));
    !(facts.index_accessors_absent() && plain_key)
}

/// The register an element access reads its key from: a `GetIndex`'s third operand, or an element operation's key.
fn element_key(instruction: WideInstruction) -> Option<Register> {
    match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementRead { key, .. }) => key.register_index(),
        Some(Operation::ElementWrite { key, .. }) => Some(key),
        _ => (instruction.op() == crate::bytecode::Op::GetIndex).then(|| instruction.register_c()),
    }
}


/// Advances `lengths` over the instruction at `pc`. `run` steps the register words over it (`effects::step`) and returns its events,
/// with the element classes of the arrays the step may resize: a call or guest code that may write an array's elements, a mutator,
/// or an element store the relations do not prove within its array. Each class is one an array the registers hold (after the step)
/// has, so the relations to it are retired (`Lengths::write`). The relations are read from the words before the instruction (a call's
/// callee, the bounds an element store meets, a resize's keys and receivers) and after it (the arrays' kinds, the registers it defines).
pub(in crate::compile::aot_facts) fn advance(ctx: &Ctx<'_>, facts: &FunctionFacts<'_>, pc: usize, instruction: WideInstruction, lengths: &mut Lengths,
    words: &mut [Word], run: impl FnOnce(&mut [Word]) -> Events) -> (Events, Vec<ElementClass>) {
    let resize = resize_of(ctx, facts, pc, instruction, words);
    let stored_below = match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementWrite { base, key, .. }) => proves_below(lengths.relations(), key, base, 0),
        _ => false,
    };
    // An element store over a fill prefix extends it through the stored index (`FilledThrough`).
    let filled_store = match Operation::<u64, ()>::of(instruction) {
        Some(Operation::ElementWrite { base, key, .. }) if lengths.relations.contains(&Relation::Filled { array: base, index: key }) => Some((base, key)),
        _ => None,
    };
    // The step's graded effects (`site_effects`) say whether it may run guest code, which may write any field.
    let calls_js = facts.site_effects(pc as u32).may_call_js;
    let events = run(words);
    let grows = events.wild_elements
        || (!stored_below && events.writes.iter().any(|(key, word)| matches!(key, Key::Element(_)) && word.cell.elements.length != Length::Never));
    let may_resize = grows || resize.may_resize();
    lengths.write(ctx, instruction, words, &events, resize, stored_below);
    lengths.clobber_loads(&events, calls_js);
    lengths.record_load(instruction, calls_js);
    if let Some((array, index)) = filled_store {
        lengths.insert(Relation::FilledThrough { array, index });
    }
    // A write that leaves an index at 0 has an empty prefix in every array (`Filled`, `[0, 0)`).
    for register in instruction.written_registers() {
        if words.get(usize::from(register)).is_some_and(|word| word.cell.ty.int_range().is_some_and(|range| range.min == 0 && range.max == 0)) {
            lengths.zeroed(words, register);
        }
    }
    if let Some(length) = allocated_length(ctx, pc, instruction, words) {
        lengths.insert(length);
    }
    let resized = if may_resize {
        words.iter().filter(|word| holds_array(word) && (grows || resize.touches(word))).map(|word| word.elements_key()).collect()
    } else {
        Vec::new()
    };
    (events, resized)
}

/// The Length relation a `new Array(n)` at `pc` gives its array: `n` holds the array's length. Read from the words after the call;
/// the callee must be the Array constructor, its one argument a Number, and the result the allocation at `pc`.
fn allocated_length(ctx: &Ctx<'_>, pc: usize, instruction: WideInstruction, words: &[Word]) -> Option<Relation> {
    let invocation = Invocation::of(instruction, ctx.decode)?;
    let Callee::Register(callee) = invocation.callee else { return None };
    if !ctx.scope.is_array_constructor(words.get(usize::from(callee))?.origin) { return None; }
    let arguments: Vec<Register> = invocation.arguments.registers(ctx.records).collect();
    let [length] = arguments.as_slice() else { return None };
    let array = instruction.result_register();
    let allocated = words.get(usize::from(array))?.origin == Some(Origin::Alloc { function: ctx.id, pc: pc as u32 });
    (allocated && words.get(usize::from(*length))?.is_number()).then_some(Relation::Length { length: *length, array })
}

/// The relations the edge of the conditional jump `instruction` carries, from the state `lengths` and `words` the jump
/// leaves. `to_target` is whether the edge is the jump's target. Where the comparison holds on the edge, an index whose
/// value (or sum) is `left < right` (or `<=`, `>`, `>=`) of a register holding a bound on an array's length is below that
/// length: by the bound's own relation (`bound_of`), and as a `Less` link to the bound (`proves_below` follows it).
pub(in crate::compile::aot_facts) fn refine(ctx: &Ctx<'_>, instruction: WideInstruction, to_target: bool, words: &[Word], lengths: &Lengths) -> Lengths {
    let mut refined = lengths.clone();
    let Some(Operation::CompareJump { comparison: Comparison::Relation { relation, .. }, left, right, negate }) = Operation::<u64, ()>::of(instruction)
        else { return refined };
    // The comparison holds on this edge when the jump's edge is not the one `negate` takes it on. A failing `<` proves
    // `left >= right`, and a holding `>=` does too: the loop's exit edge of `index < bound` reaches the bound.
    let holds = to_target != negate;
    refined.refine_interval(ctx, left, right, relation, holds, words);
    if let (Some(index), Some(bound)) = (left.register_index(), right.register_index()) {
        let at_least = match relation {
            NumberRelation::LessThan => !holds,
            NumberRelation::GreaterEqual => holds,
            _ => false,
        };
        if at_least && words.get(usize::from(index)).is_some_and(|word| word.is_number()) && words.get(usize::from(bound)).is_some_and(|word| word.is_number()) {
            refined.insert(Relation::AtLeast { index, bound });
            for array in complete_arrays(&refined.relations, index, bound) {
                refined.insert(Relation::Complete { array });
            }
        }
    }
    // `index >= 0` against the constant zero: a counter the test found non-negative on this edge, which a later step
    // down retires (`Relation::NonNegative`).
    if let Some(index) = left.register_index()
        && constant_of(ctx, right) == Some(0)
        && words.get(usize::from(index)).is_some_and(|word| word.is_number())
    {
        let non_negative = match relation {
            NumberRelation::GreaterEqual => holds,
            NumberRelation::LessThan => !holds,
            _ => false,
        };
        if non_negative {
            refined.insert(Relation::NonNegative { index });
        }
    }
    if !holds {
        return refined;
    }
    // `index + slack <= bound`: a strict comparison adds one to the index.
    let (small, big, slack): (Operand, Operand, i64) = match relation {
        NumberRelation::LessThan => (left, right, 1),
        NumberRelation::LessEqual => (left, right, 0),
        NumberRelation::GreaterThan => (right, left, 1),
        NumberRelation::GreaterEqual => (right, left, 0),
        _ => return refined,
    };
    let (Some(small), Some(big)) = (small.register_index(), big.register_index()) else { return refined };
    // The index is the sum the small operand holds, or the small operand itself where it is a Number.
    let indexed = lengths.relations.iter().find_map(|held| match *held {
        Relation::Sum { sum, index, offset } if sum == small => Some((index, offset)),
        _ => None,
    }).or_else(|| words.get(usize::from(small)).filter(|word| word.is_number()).map(|_| (small, 0)));
    let Some((index, offset)) = indexed else { return refined };
    if let Some((array, reach)) = bound_of(&lengths.relations, big) {
        refined.insert(Relation::Within { index, array, reach: offset + slack + reach });
    }
    // `index < bound` between Numbers: `proves_below` follows it through the bound's own relations, and `bound_below` reads
    // the bound an index is below (a loop's counter against its bound, whichever length the bound is at most).
    if slack == 1 && offset == 0 && words.get(usize::from(big)).is_some_and(|word| word.cell.ty.is_subtype(Type::NUMBER)) {
        refined.insert(Relation::Less { index, bound: big });
    }
    refined
}

/// The integer an operand is, where it is a constant of the program that holds one integer (`Scope::constant_type`).
fn constant_of(ctx: &Ctx<'_>, operand: Operand) -> Option<i64> {
    if !matches!(operand.kind(), Some(OperandKind::Constant)) {
        return None;
    }
    let range = ctx.scope.constant_type(usize::from(operand.payload())).int_range()?;
    (range.min == range.max).then_some(range.min)
}

/// Whether `instruction` is a Number binary operation of `operation` with no flags.
pub(super) fn is_number_binary(instruction: WideInstruction, operation: NumberArithmetic) -> bool {
    instruction.op() == crate::bytecode::Op::Binary
        && instruction.result_flags() == 0
        && NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()) == Some(operation)
}

/// The step a constant addition or subtraction makes of a register, as `(destination, source, delta)` where
/// `destination = source + delta`: `source + k`, `k + source` or `source - k`. A `k - source` steps no register by a constant.
pub(super) fn step_of_constant(ctx: &Ctx<'_>, instruction: WideInstruction) -> Option<(Register, Register, i64)> {
    let (left, right) = (instruction.operand_b(), instruction.operand_c());
    let (source, delta) = if is_number_binary(instruction, NumberArithmetic::Add) {
        match (left.register_index(), right.register_index()) {
            (Some(source), None) => (source, constant_of(ctx, right)?),
            (None, Some(source)) => (source, constant_of(ctx, left)?),
            _ => return None,
        }
    } else if is_number_binary(instruction, NumberArithmetic::Subtract) {
        (left.register_index()?, constant_of(ctx, right)?.checked_neg()?)
    } else {
        return None;
    };
    Some((instruction.result_register(), source, delta))
}

/// The registers of `destination = first + second`, a Number addition of two registers.
pub(super) fn sum_of_registers(instruction: WideInstruction) -> Option<(Register, Register, Register)> {
    if !is_number_binary(instruction, NumberArithmetic::Add) {
        return None;
    }
    Some((instruction.result_register(), instruction.operand_b().register_index()?, instruction.operand_c().register_index()?))
}

/// Whether a constant shift count moves a bit: its low bits (`WORD32_SHIFT_MASK`) are not all zero.
pub(super) fn moves_bits(ctx: &Ctx<'_>, count: Operand) -> bool {
    constant_of(ctx, count).is_some_and(|count| ((count as u32) & crate::value::WORD32_SHIFT_MASK) != 0)
}

/// Whether a shift is zero-filling (`>>>`, `true`) or sign-propagating (`>>`, `false`); `None` for any other operation.
pub(super) fn shift_kind(operation: NumberBitwise) -> Option<bool> {
    match operation {
        NumberBitwise::ShiftRight => Some(false),
        NumberBitwise::ShiftRightUnsigned => Some(true),
        _ => None,
    }
}
