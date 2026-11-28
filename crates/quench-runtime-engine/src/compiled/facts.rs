//! Typed facts: the first slice of the typed IR (TOD-1241, Linear TOD-1232).
//!
//! An abstract interpretation of one function's bytecode over a type lattice, run to a
//! fixpoint across its control-flow graph before any code is lowered. It names, for every
//! block leader, the type each register holds on every path into it. A lowering reads the
//! facts to choose each register's representation at block entry, so a fact proved on one
//! side of a jump is not proved again on the other.
//!
//! Entry guards discharge input requirements. Lowering conforms each edge to its target's
//! semantic type and machine view: an edge proves what its state lacks with one guard, and
//! one that cannot leaves at the target, so a fact the analysis gets wrong costs an exit,
//! never the body. Constants, definitions and joins use the one `quench_runtime::typed_ir` lattice.
//!
//! One solver (L2): the transfer rules below now have one home in the whole-program solve, `quench-runtime`'s
//! `compile/aot_facts`, and a reader takes the register's `Cell` from `FunctionFacts::cell_at` (its `ty` carries the
//! integer range) instead of re-solving. This file is deleted once its readers have switched.
//! - `facts/binary.rs` `numeric_binary`, `number_result`, `heap_primitive_binary`: `aot_facts/numeric.rs` `binary_type`.
//! - `facts/ranges.rs` `arithmetic_type`, `exact`, `step`, `bitwise`, `result`: `aot_facts/numeric.rs` `arithmetic`,
//!   `bitwise`, `remainder`, `step`, over `typed_ir::IntRange`'s `add`, `subtract`, `multiply`, `as_number`, `and`, `or`,
//!   `stepped`, `remainder`.
//! - `facts/ranges.rs` `Edge::of`, `relation_ranges`, `effective_relation`: `aot_facts/numeric.rs` `relation_edge` over
//!   `typed_ir::IntRange`'s `ordered_edge` and `without`, applied on the jump's edges by `aot_facts/flow.rs` `refine_ranges`.
//! - `facts/immediates.rs` `constant_type`: `aot_facts/scope.rs` `constant_type`, re-exported here (an int32 constant is its
//!   singleton range, `null`, `undefined` and Booleans their exact types). `immediate` stays: it denotes a `Value`, not a type.
//! - Not portable to a non-relational cell: length relations (`i < a.length`) are the solver's `length_settle` pass, not
//!   this run's; `facts/written.rs` and `facts/widen.rs` (a lane body's entry state); the int32 inductions (`inductive`,
//!   `loop_int32`, lane contracts).
//!   `ranges.rs` `index_range`, `signless_range`, `zero_free` and `arithmetic_type` decode the operator and read its operands'
//!   ranges; the rules over those ranges are `typed_ir::arithmetic`. `overflow_free` (run.rs) reads `ranges::exact`.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationRecordsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::{Constant, ControlFlowLayout, Operand, OperandKind, Register, WideInstruction};
use quench_runtime::staged::{NumberOperation, Operation};
use quench_runtime::typed_ir::Type;
use std::num::NonZeroUsize;

/// Scratch storage spills for larger functions without limiting region size.
const INLINE_ANALYSIS_ITEMS: usize = 16;

/// A guarded block input contract the fixed point keeps: the register holds the type on
/// entry to the leader, which every native edge into it proves (`Lowering::conform`) and
/// every in-loop definition preserves (`loop_int32`).
pub(crate) type Contract = (usize, Register, Type);

/// The registers one instruction writes, inline: an instruction has three register fields (A, B, C).
pub(crate) type Writes = smallvec::SmallVec<[Register; 3]>;

/// The registers' types on entry to each block leader; `None` at a PC that leads no block or
/// that no path from the entry reaches.
#[derive(Default)]
pub(crate) struct Facts {
    offsets: Vec<Option<NonZeroUsize>>,
    types: Vec<Type>,
    registers: usize,
    reached: quench_runtime::dataflow::BitSet,
    /// Join of the entry's backedge inputs, without the unknown external input.
    backedge: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]>,
    contracts: Vec<Contract>,
    /// The typed element reads (sorted PCs), each defining the element type its typed array's kind proves.
    typed_reads: Vec<TypedRead>,
    /// The replays `before` runs from each block leader, keyed by leader PC.
    replays: std::cell::RefCell<std::collections::BTreeMap<usize, Replay>>,
}

/// The transfer `Facts::before` replays from one block leader, kept so each instruction runs once.
struct Replay {
    /// The states before each PC from the leader through `next`, `registers` types apiece.
    rows: Vec<Type>,
    /// The PC the replay has reached: the state before it is the last row.
    next: usize,
    /// The instruction that leaves the function's native code, which ends the replay.
    leaves: Option<usize>,
}

impl Facts {
    pub(crate) fn reached(&self, pc: usize) -> bool { self.reached.contains(pc) }
    pub(crate) fn backedge(&self) -> &[Type] { &self.backedge }
    pub(crate) fn contracts(&self) -> &[Contract] { &self.contracts }

    /// A run over `blocks` PCs of `registers` registers.
    fn new(blocks: usize, registers: usize, seeds: run::Seeds<'_>) -> Self {
        Self { offsets: vec![None; blocks], registers, reached: quench_runtime::dataflow::BitSet::with_capacity(blocks),
            contracts: seeds.contracts.to_vec(), typed_reads: seeds.typed_reads.to_vec(), ..Self::default() }
    }

    /// The state on entry to the block led by `pc`.
    fn point_at(&self, pc: usize) -> run::Point {
        let start = self.offsets[pc].expect("a pending leader has facts").get() - 1;
        run::Point { types: smallvec::SmallVec::from_slice(&self.types[start..start + self.registers]) }
    }

    /// The types on entry to the block led by `pc`. Rows share one flat buffer;
    /// the PC table stores element offsets, including reachable zero-register rows.
    pub(crate) fn at(&self, pc: usize) -> Option<&[Type]> {
        let start = self.offsets.get(pc)?.as_ref()?.get() - 1;
        Some(&self.types[start..start + self.registers])
    }

    /// Select a guarded block input contract. Every native edge must conform
    /// before the lowering consumes this row; this is not a source inference.
    pub(crate) fn require(&mut self, pc: usize, register: Register, required: Type) {
        let start = self.offsets[pc].expect("reachable contract block").get() - 1;
        let known = &mut self.types[start + usize::from(register)];
        if !known.is_subtype(required) { *known = required; }
        // A replay from this leader started from the row just changed.
        self.replays.get_mut().remove(&pc);
    }

    /// Project a block's entry facts to an instruction using the same transfer
    /// semantics as the fixed point. This is a representation candidate; the
    /// lowering must still discharge entry and update requirements.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn before(&self, pc: usize, register: Register, instructions: &[WideInstruction],
        leaders: &quench_runtime::dataflow::BitSet, constants: &[Constant],
        written: &dyn Fn(WideInstruction) -> Writes, native_calls: bool) -> Type {
        let Some(start) = (0..=pc).rev().find(|pc| leaders.contains(*pc)) else { return Type::ANY };
        let Some(entry) = self.at(start) else { return Type::ANY };
        let registers = self.registers;
        let mut replays = self.replays.borrow_mut();
        let replay = replays.entry(start).or_insert_with(|| Replay { rows: entry.to_vec(), next: start, leaves: None });
        if replay.leaves.is_some_and(|leaves| pc > leaves) { return Type::ANY; }
        // Extend the replay to `pc` (or the end of the code), so each instruction runs once.
        while replay.next < pc.min(instructions.len()) {
            let at = replay.next;
            let mut types: smallvec::SmallVec<[Type; INLINE_ANALYSIS_ITEMS]> = smallvec::SmallVec::from_slice(&replay.rows[replay.rows.len() - registers..]);
            if matches!(transfer_instruction(&mut types, instructions[at], constants, written, native_calls, typed_read(&self.typed_reads, at)), Flow::Leaves) {
                replay.leaves = Some(at);
                return Type::ANY;
            }
            replay.rows.extend_from_slice(&types);
            replay.next += 1;
        }
        let index = usize::from(register);
        if index >= registers { return Type::ANY; }
        replay.rows[(pc.min(replay.next) - start) * registers + index]
    }

    /// Project each definition in `body`'s post-state through the same transfer as the fixed
    /// point. Each block replays from its own entry row, so only the blocks reaching `body` run.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn for_each_definition(&self, instructions: &[WideInstruction], leaders: &quench_runtime::dataflow::BitSet,
        constants: &[Constant], written: &dyn Fn(WideInstruction) -> Writes, native_calls: bool,
        body: std::ops::RangeInclusive<usize>, mut visit: impl FnMut(usize, Register, Type)) {
        // The block covering `body.start()` may begin earlier: replay it from its leader.
        let first = (0..=*body.start()).rev().find(|pc| leaders.contains(*pc)).unwrap_or(0);
        let end = instructions.len().min((*body.end()).saturating_add(1));
        let mut types = smallvec::SmallVec::<[Type; INLINE_ANALYSIS_ITEMS]>::new();
        for start in (first..end).filter(|pc| leaders.contains(*pc)) {
            let Some(entry) = self.at(start) else { continue };
            types.clear();
            types.extend_from_slice(entry);
            for (pc, instruction) in instructions.iter().copied().enumerate().skip(start) {
                if pc != start && leaders.contains(pc) { break; }
                let flow = transfer_instruction(&mut types, instruction, constants, written, native_calls, typed_read(&self.typed_reads, pc));
                for register in written(instruction) {
                    visit(pc, register, types.get(usize::from(register)).copied().unwrap_or(Type::ANY));
                }
                if !matches!(flow, Flow::Next) { break; }
            }
        }
    }

    #[cfg(test)]
    fn merge(&mut self, pc: usize, incoming: &[Type]) -> bool {
        self.replays.get_mut().clear();
        let Some(slot) = self.offsets.get(pc) else { return false };
        let Some(offset) = *slot else {
            self.store_row(pc, incoming);
            let start = self.types.len() - incoming.len();
            hold_contracts(&self.contracts, pc, &mut self.types[start..]);
            return true;
        };
        let start = offset.get() - 1;
        let mut changed = join_types(&mut self.types[start..start + self.registers], incoming, false);
        changed |= hold_contracts(&self.contracts, pc, &mut self.types[start..start + self.registers]);
        changed
    }

    /// Stores the row of the block led by `pc`, which has none: its types.
    fn store_row(&mut self, pc: usize, types: &[Type]) {
        self.offsets[pc] = NonZeroUsize::new(self.types.len() + 1);
        self.types.extend_from_slice(types);
    }
}

/// Joins the `incoming` types into the `known` row, widening a range that grows where `widening`;
/// whether the row changed. Most edges bring a row equal to the one held, register by register:
/// the join of equal types is the type, so only differing registers join.
fn join_types(known: &mut [Type], incoming: &[Type], widening: bool) -> bool {
    let mut changed = false;
    for (known, incoming) in known.iter_mut().zip(incoming) {
        if *known == *incoming { continue; }
        let joined = if widening { known.widen(*incoming) } else { known.join(*incoming) };
        changed |= joined != *known;
        *known = joined;
    }
    changed
}

/// Narrows the row `types` of the block led by `pc` to the contracts held there; whether one did.
fn hold_contracts(contracts: &[Contract], pc: usize, types: &mut [Type]) -> bool {
    let mut changed = false;
    for &(_, register, required) in contracts.iter().filter(|(at, _, _)| *at == pc) {
        let known = &mut types[usize::from(register)];
        if !known.is_subtype(required) {
            *known = required;
            changed = true;
        }
    }
    changed
}

/// Where control goes after an instruction under the lowering this models.
enum Flow {
    /// The next instruction.
    Next,
    /// The explicit jump target; conditional fallthrough is handled separately.
    To(usize),
    /// Leaves the function's native code.
    Leaves,
    /// Each entry of a branch table (`bytecode::branch_table_entries`).
    Table,
}

fn register(operand: Operand) -> Option<Register> {
    (operand.kind()? == OperandKind::Register).then(|| operand.register_index()).flatten()
}

/// The semantic type of an operand, independent of its selected machine view.
pub(crate) fn operand_type(constants: &[Constant], operand: Operand, register_type: impl Fn(Register) -> Type) -> Type {
    match operand.kind() {
        Some(OperandKind::Register) => register_type(operand.payload()),
        Some(OperandKind::Constant) => constants.get(usize::from(operand.payload())).map_or(Type::ANY, constant_type),
        _ => Type::ANY,
    }
}

/// Whether a value of type `ty` is known to be no Number (a string, a boolean, `null`): a relation
/// reading it is no Number comparison.
pub(crate) fn numberless(ty: Type) -> bool {
    !ty.is_subtype(Type::NUMBER) && !Type::NUMBER.is_subtype(ty)
}

pub(crate) fn prove_number(types: &mut [Type], operands: [Operand; 2]) {
    for register in operands.into_iter().filter_map(register) {
        if !types.get(usize::from(register)).is_some_and(|ty| ty.is_subtype(Type::NUMBER)) {
            set(types, register, Type::NUMBER);
        }
    }
}

fn set(types: &mut [Type], register: Register, ty: Type) {
    if let Some(slot) = types.get_mut(usize::from(register)) {
        *slot = ty;
    }
}

/// Whether a binary `operator` yields a Boolean however it completes: an equality, a
/// relation, `instanceof` or `in`.
pub(crate) fn boolean_binary(operator: u32) -> bool {
    use oxc_ast::ast::BinaryOperator;
    quench_runtime::vm::operations::equality_operator(operator).is_some()
        || quench_runtime::vm::operations::NumberRelation::from_operator(operator).is_some()
        || [BinaryOperator::Instanceof, BinaryOperator::In].iter().any(|relation| *relation as u32 == operator)
}

/// Applies `instruction` to `types`; returns where control goes next. A call continues where
/// the region makes native calls (`native_calls`), past which no object keeps its shape.
fn transfer(
    types: &mut [Type],
    instruction: WideInstruction,
    operation: Option<&Operation<u64, ()>>,
    constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Writes,
    native_calls: bool,
) -> Flow {
    let op = instruction.op();
    if quench_runtime::staged::loads_constant(instruction.op()) {
        let ty = constants.get(instruction.constant_index()).map_or(Type::ANY, constant_type);
        set(types, instruction.result_register(), ty);
        return Flow::Next;
    }
    // A comparison jump: a nullish test of it folds to its edge (`nullish_comparison`).
    let compares = matches!(operation, Some(Operation::CompareJump { .. }));
    match operation {
        Some(Operation::LoadConst { .. }) => {
            let ty = constants.get(instruction.constant_index()).map_or(Type::ANY, constant_type);
            set(types, instruction.result_register(), ty);
            Flow::Next
        }
        Some(Operation::Move { .. }) => {
            let source = types.get(usize::from(instruction.register_b())).copied().unwrap_or(Type::ANY);
            set(types, instruction.result_register(), source);
            Flow::Next
        }
        // A dense switch's case slot: an int32 from 0 to its count, or the operand leaves.
        Some(Operation::Number(NumberOperation::IntCase { destination, .. })) => {
            set(types, *destination, Type::INT32);
            Flow::Next
        }
        Some(Operation::Number(NumberOperation::IncDec { .. })) => {
            // The operand is proved int32 or the instruction leaves.
            ranges::step(types, instruction);
            Flow::Next
        }
        _ if super::meaning_compiled::is_numeric_binary(instruction) => binary::numeric_binary(types, instruction, constants, written),
        _ if compares && super::nullish_comparison(constants, instruction,
            |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)).is_some() => {
            Flow::To(instruction.jump_target() as usize)
        }
        Some(Operation::CompareJump { .. }) => {
            // An equality, loose or strict, and `instanceof` decide operands of any type
            // (`lane_region::relation`: two objects compare by identity) and prove nothing of
            // them; every other relation compares Numbers.
            let operands = [instruction.operand_b(), instruction.operand_c()];
            let operand_types = operands.map(|operand| operand_type(constants, operand,
                |register| types.get(usize::from(register)).copied().unwrap_or(Type::ANY)));
            let operator = instruction.binary_operator_field();
            // So does a relation with an operand known to hold no Number (`numberless`), which
            // the lane decides by the interpreter's rule (`Lowering::decided_relation`).
            let numeric = quench_runtime::vm::operations::NumberRelation::from_operator(operator).is_some()
                && quench_runtime::vm::operations::equality_operator(operator).is_none()
                && !operand_types.iter().any(|ty| numberless(*ty));
            if numeric && !operand_types.iter().all(|ty| ty.is_subtype(Type::INT32)) { prove_number(types, operands); }
            Flow::To(instruction.jump_target() as usize)
        }
        _ => {
            for register in written(instruction) {
                set(types, register, Type::ANY);
            }
            match op.control_flow_layout() {
                // Any call, a construction's included (`Lowering::construct`): its result is
                // written above, and no object keeps its shape past it.
                ControlFlowLayout::Call if native_calls => forget_shapes(types, true),
                // An instruction the analysis does not model continues with the registers it
                // writes unknown, and with no object shape kept unless it cannot change one: the
                // code after it is typed whether the lowering runs it natively or leaves at it.
                ControlFlowLayout::Fallthrough => forget_shapes(types, !(matches!(operation,
                    Some(Operation::GetField { .. } | Operation::SetField { .. })))
                    && !(Operation::<u64, ()>::is_nop(instruction)
                        // merge: HEAD keeps an element read of a frame local's base (`ElementReadOperand`) too.
                        || matches!(operation, Some(Operation::LoadThis { .. } | Operation::LoadCapture { .. }
                            | Operation::ElementRead { .. } | Operation::ElementReadOperand { .. }
                            | Operation::ElementWrite { .. })))),
                ControlFlowLayout::Jump | ControlFlowLayout::ConditionalJump => {
                    Flow::To(instruction.jump_target() as usize)
                }
                ControlFlowLayout::BranchTable => Flow::Table,
                _ => Flow::Leaves,
            }
        }
    }
}

/// Continues the analysis past an instruction that may change an object's shape (`forget`): no
/// register keeps a shape.
fn forget_shapes(types: &mut [Type], forget: bool) -> Flow {
    if forget { for ty in types.iter_mut() { *ty = ty.without_object_shape(); } }
    Flow::Next
}

/// The transfer of a ToNumeric or a Number's unary operator, whose operand is a Number or whose
/// instruction leaves (`Lowering::instruction`, `Lowering::unary`); `None` for any other.
fn unary_transfer(types: &mut [Type], instruction: WideInstruction, operation: Option<&Operation<u64, ()>>) -> Option<Flow> {
    let op = instruction.op();
    match operation {
        // A Number is its own ToNumeric; an untyped operand is taken for an int32 and tested once.
        Some(Operation::Number(NumberOperation::ToNumeric { .. })) if !instruction.returns_from_frame() => {
            let source = types.get(usize::from(instruction.register_b())).copied().unwrap_or(Type::ANY);
            let ty = if source.is_subtype(Type::NUMBER) { source } else { Type::INT32 };
            set(types, instruction.register_b(), ty);
            set(types, instruction.result_register(), ty);
            Some(Flow::Next)
        }
        _ if super::meaning_compiled::is_unary(instruction) && instruction.result_flags() == 0 && !instruction.writes_current_this()
            && super::meaning_compiled::is_numeric_unary(instruction)
            && quench_runtime::vm::operations::NumberUnary::from_js_instruction(op, instruction.unary_operator()).is_some() => {
            let source = instruction.register_b();
            if !types.get(usize::from(source)).is_some_and(|ty| ty.is_subtype(Type::NUMBER)) { set(types, source, Type::NUMBER); }
            set(types, instruction.result_register(), Type::NUMBER);
            Some(Flow::Next)
        }
        // A null or undefined operand leaves; the operand's type is unchanged.
        Some(Operation::RequireObjectCoercible { .. }) => Some(Flow::Next),
        _ => None,
    }
}

/// The registers `instruction` leaves unless they hold Numbers (what its transfer proves of
/// operands it does not write): a loop that does not write one may have that proved once, on its
/// entering edge, instead of by each use.
pub(crate) fn proves_numbers(instruction: WideInstruction, registers: usize, constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Writes, native_calls: bool) -> Vec<Register> {
    let mut types = vec![Type::ANY; registers];
    let operation = Operation::<u64, ()>::of(instruction);
    if matches!(transfer(&mut types, instruction, operation.as_ref(), constants, written, native_calls), Flow::Leaves) { return Vec::new(); }
    let written = written(instruction);
    (0..registers).filter(|register| types[*register].is_subtype(Type::NUMBER) && !written.contains(&(*register as Register)))
        .map(|register| register as Register).collect()
}

/// A typed element read (its PC) and the element type its typed array's kind proves it yields (`Seeds::typed_reads`).
pub(crate) type TypedRead = (usize, Type);

/// The element type the typed read at `pc` yields, of `reads` (sorted by PC).
pub(crate) fn typed_read(reads: &[TypedRead], pc: usize) -> Option<Type> {
    reads.binary_search_by_key(&pc, |(at, _)| *at).ok().map(|at| reads[at].1)
}

/// `transfer`, where a typed element read (`typed`) defines the element type its kind proves.
fn transfer_instruction(types: &mut [Type], instruction: WideInstruction, constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Writes, native_calls: bool, typed: Option<Type>) -> Flow {
    // The instruction decodes once: its unary and general transfers both read this operation.
    let operation = Operation::<u64, ()>::of(instruction);
    let flow = unary_transfer(types, instruction, operation.as_ref())
        .unwrap_or_else(|| transfer(types, instruction, operation.as_ref(), constants, written, native_calls));
    if let Some(ty) = typed {
        for register in written(instruction) { set(types, register, ty); }
    }
    flow
}

/// Prove a checked integer entry contract by induction. Every native write
/// and every backedge must preserve the entry hypothesis. Rejecting one input
/// removes that assumption from all dependent proofs on the next iteration.
#[allow(clippy::too_many_arguments)]
pub(crate) fn inductive_int32(initial: &mut [Type], candidates: quench_runtime::dataflow::BitSet,
    body: std::ops::RangeInclusive<usize>, instructions: &[WideInstruction], leaders: &quench_runtime::dataflow::BitSet,
    constants: &[Constant], written: &dyn Fn(WideInstruction) -> Writes,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, native_calls: bool, raises: &[Raise]) -> Option<Facts> {
    inductive(initial, candidates, Type::INT32, body, instructions, leaders, constants, written, own_shape, native_calls, raises)
}

/// `inductive_int32` for the header contract `required`: an int32, or a Number (an
/// accumulator that enters as an int32 or a double and every definition keeps a Number).
#[allow(clippy::too_many_arguments)]
pub(crate) fn inductive(initial: &mut [Type], mut candidates: quench_runtime::dataflow::BitSet, required: Type,
    body: std::ops::RangeInclusive<usize>, instructions: &[WideInstruction], leaders: &quench_runtime::dataflow::BitSet,
    constants: &[Constant], written: &dyn Fn(WideInstruction) -> Writes,
    own_shape: &dyn Fn(usize) -> Option<(Register, u32)>, native_calls: bool, raises: &[Raise]) -> Option<Facts> {
    // Each unsuccessful iteration removes at least one candidate. Compile
    // work is bounded by candidate count, without a semantic iteration cap.
    while !candidates.is_empty() {
        let mut inputs = initial.to_vec();
        for register in candidates.iter() { inputs[register] = required; }
        let facts = analyze(*body.start(), &inputs, instructions, leaders, constants, written, own_shape, native_calls, raises, Seeds::typed());
        let mut rejected: quench_runtime::dataflow::BitSet = quench_runtime::dataflow::BitSet::with_capacity(initial.len());
        for register in candidates.iter() {
            if !facts.backedge().get(register).is_some_and(|ty| ty.is_subtype(required)) { rejected.insert(register); }
        }
        facts.for_each_definition(instructions, leaders, constants, written, native_calls, body.clone(), |pc, register, ty| {
            let index = usize::from(register);
            if body.contains(&pc) && candidates.contains(index) && !ty.is_subtype(required) { rejected.insert(index); }
        });
        if rejected.is_empty() {
            initial.copy_from_slice(&inputs);
            return Some(facts);
        }
        for register in rejected.iter() { candidates.remove(register); }
    }
    None
}

/// Prove int32 header contracts of inner loops by induction: each candidate (a loop header
/// and a register) holds an int32 there when every definition in its loop's body yields one
/// under all the contracts. Rejecting one removes it from the next iteration's hypothesis.
#[allow(clippy::too_many_arguments)]
pub(crate) fn loop_int32(entry: usize, initial: &[Type], mut candidates: Vec<(Contract, std::ops::RangeInclusive<usize>)>,
    instructions: &[WideInstruction], leaders: &quench_runtime::dataflow::BitSet, constants: &[Constant],
    written: &dyn Fn(WideInstruction) -> Writes, own_shape: &dyn Fn(usize) -> Option<(Register, u32)>,
    native_calls: bool, raises: &[Raise]) -> Option<Facts> {
    while !candidates.is_empty() {
        let contracts: Vec<Contract> = candidates.iter().map(|(contract, _)| *contract).collect();
        let seeds = Seeds { contracts: &contracts, ..Seeds::typed() };
        let facts = analyze(entry, initial, instructions, leaders, constants, written, own_shape, native_calls, raises, seeds);
        let mut rejected = vec![false; candidates.len()];
        let span = candidates.iter().map(|(_, body)| body);
        let (first, last) = (span.clone().map(|body| *body.start()).min().unwrap_or(entry), span.map(|body| *body.end()).max().unwrap_or(entry));
        facts.for_each_definition(instructions, leaders, constants, written, native_calls, first..=last, |pc, register, ty| {
            for (index, ((_, candidate, required), body)) in candidates.iter().enumerate() {
                if *candidate == register && body.contains(&pc) && !ty.is_subtype(*required) { rejected[index] = true; }
            }
        });
        if !rejected.contains(&true) { return Some(facts); }
        let mut index = 0;
        candidates.retain(|_| { index += 1; !rejected[index - 1] });
    }
    None
}

#[cfg(test)]
#[path = "facts/tests.rs"] pub(crate) mod tests;
#[path = "facts/immediates.rs"] pub(crate) mod immediates;
#[path = "facts/written.rs"] pub(crate) mod written;
#[path = "facts/binary.rs"] pub(crate) mod binary;
#[path = "facts/widen.rs"] pub(crate) mod widen;
#[path = "facts/ranges.rs"] pub(crate) mod ranges;
#[path = "facts/lengths.rs"] pub(crate) mod lengths;
#[path = "facts/run.rs"] pub(crate) mod run;
pub(crate) use immediates::{constant_number, immediate, is_int32};

/// The type a program constant denotes (`aot_facts::constant_type`), without the atom a String constant interns to: these
/// facts read the constant table, not the program's atoms, so a string constant is any string of its exact length.
pub(crate) fn constant_type(constant: &Constant) -> Type {
    quench_runtime::compile::aot_facts::constant_type(constant, None)
}
pub(crate) use binary::{heap_operator, heap_primitive_binary};
pub(crate) use lengths::IndexProof;
pub(crate) use run::{analyze, analyze_text, Model, Point, Raise, Seeds, Text, TypedAccess};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

#[cfg(test)]
#[path = "facts/decode_ratchet.rs"] pub(crate) mod decode_ratchet;
