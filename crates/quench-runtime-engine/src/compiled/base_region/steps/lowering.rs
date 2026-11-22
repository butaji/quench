//! The lowering of an instruction no row takes (`steps`): which step makes it (`lowering`), the fast arms
//! its step carries (`fast_arm`), and the decoded row of a step (`decode_step`). Planning the step's chain is
//! `steps`' own.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::super::*;
use super::super::call_plan::{self};
use super::super::callees::chains;
use super::super::decode::{staged_binary, staged_of, staged_unary};
use super::super::super::{LaneHelper, LaneHelpers};
use super::Step;
use quench_runtime::staged::{Operation, Protocol};

/// The most registers a step the helper makes writes: its result, and the `this` a name call
/// defines beside its callee.
pub(crate) const STEP_WRITES: usize = 2;

/// Whether `instruction` is made in place by `LaneHelper::Step`: no result flag but a numeric operator's local target,
/// which the interpreter's step writes to the frame's local slot (`Vm::step`'s `numeric_local_target`) and defines no
/// register for (`written_registers`).
fn steps_in_place(instruction: WideInstruction) -> bool {
    let flags = instruction.result_flags();
    instruction.op().steps_in_body() && (flags == 0 || instruction.writes_numeric_local() && flags == quench_runtime::bytecode::NUMERIC_LOCAL_TARGET)
        && instruction.written_registers().count() <= STEP_WRITES
}

/// The heap row that is the fast arm of a `Unary` that is a `typeof` string or test (the index form is
/// `Operation::TypeofIndex`), where the test's name index fits its hole.
fn typeof_arm(operator: u32) -> Option<Heap> {
    if operator == quench_runtime::vm::coercion::TYPEOF {
        return Some(Heap::TypeofString);
    }
    let (index, negated) = quench_runtime::bytecode::typeof_test(operator)?;
    Some(Heap::TypeofIs { index: u8::try_from(index).ok()?, negated })
}

/// The heap row that is the fast arm of `instruction`, where its meaning has one that a register's
/// word (`staged::iteration`, `staged::value`) or the backing's own layout words
/// (`elements::append_in_room`) decide.
fn fast_arm(instruction: WideInstruction, known: Known) -> Option<Heap> {
    match staged_of(instruction) {
        Some(Operation::PropertyKey { .. }) => Some(Heap::Key),
        // One element (not a spread, not elisions) defined at the literal's length: the case a capacity word decides.
        Some(Operation::Protocol { step: Protocol::AppendArray { length: 0, spread: false }, operands: [_, Some(_)], .. }) => Some(Heap::Append),
        Some(Operation::ForInNext { .. }) => Some(Heap::ForIn),
        Some(Operation::TypeofIndex { .. }) => Some(Heap::TypeofIndex),
        Some(Operation::RequireIteratorResult { .. }) => Some(Heap::Object),
        Some(Operation::RequireObjectCoercible { .. }) => staged_plain_arm(instruction, numeric::row_names::STAGED_COERCIBLE),
        Some(Operation::Unary { .. }) => staged_unary_arm(instruction),
        Some(Operation::TypeofString { .. } | Operation::TypeofTest { .. }) => typeof_arm(instruction.unary_operator()),
        _ => match instruction.op().class() {
            // derivation debt: no staged::Operation for SetFunctionName (the naming is a recorded site, `names`)
            Class::SetFunctionName => names::of(known, instruction).map(names::Name::heap),
            // derivation debt: no staged::Operation for a Binary with a Number constant operand (`Operation::Binary` takes registers only)
            Class::Binary => staged_binary_arm(instruction),
            _ => None,
        },
    }
}

/// The staged fast arm of a `Binary` no numeric row takes (`staged_binary`), over two register operands: a constant
/// operand has no staged row.
fn staged_binary_arm(instruction: WideInstruction) -> Option<Heap> {
    let code = staged_binary(instruction.binary_operator())?;
    let registers = instruction.operand_b().register_index().is_some() && instruction.operand_c().register_index().is_some();
    (instruction.result_flags() == 0 && registers).then_some(Heap::Staged(code))
}

/// The staged fast arm of a `Unary` no unary row takes (`staged_unary`): a `void` that returns from the frame is the
/// general step's (its record returns the value).
fn staged_unary_arm(instruction: WideInstruction) -> Option<Heap> {
    let code = staged_unary(instruction.unary_operator())?;
    (instruction.result_flags() == 0 && !instruction.returns_from_frame()).then_some(Heap::Staged(code))
}

/// The staged fast arm `code` of an instruction whose only operand is its register `b` (`RequireObjectCoercible`).
fn staged_plain_arm(instruction: WideInstruction, code: u8) -> Option<Heap> {
    (instruction.result_flags() == 0).then_some(Heap::Staged(code))
}

/// The registers a fast arm reads: its operand in `a`, and for an append the array it writes
/// through, the way a store names its array (`Decoded::base`).
fn fast_operands(instruction: WideInstruction, fast: Heap) -> (Option<Source>, Option<Register>) {
    match fast {
        Heap::Add { .. } => adds::operands(instruction),
        Heap::Name { .. } => (Some(Source::Register(instruction.register_a())), None),
        Heap::Append => (instruction.optional_register_b().map(Source::Register), Some(instruction.register_a())),
        Heap::Staged(numeric::row_names::STAGED_VOID) => (None, None),
        Heap::Staged(numeric::row_names::STAGED_BITWISE_NOT | numeric::row_names::STAGED_COERCIBLE) => (Some(Source::Register(instruction.register_b())), None),
        Heap::Staged(_) => (instruction.operand_b().register_index().map(Source::Register), None),
        _ => (Some(Source::Register(instruction.register_b())), None),
    }
}

/// The second register a staged binary's fast arm reads (its right operand), beside `fast_operands`' left.
fn fast_right(instruction: WideInstruction, fast: Heap) -> Option<Source> {
    match fast {
        Heap::Staged(_) if instruction.op().class() == Class::Binary => instruction.operand_c().register_index().map(Source::Register),
        _ => None,
    }
}


/// Whether a loop region makes `instruction` by its call row (`Step::Call`): a call a body's chain makes
/// (`callees::chains`) that completes no activation, where the helper table has the region call
/// helper's address. Any other call leaves, as the rest of the instructions no row takes.
pub(crate) fn calls_in_place(instruction: WideInstruction, helpers: LaneHelpers) -> bool {
    chains(instruction) && instruction.result_flags() == 0 && call_plan::helper_address(helpers, LaneHelper::RegionCall).is_some()
}

/// The packed words of `instruction` where its step is the helper step and the packed helper runs it directly
/// (`dispatch_fast::family_helpers::covers`), with the helper table holding the packed helper: the helper
/// row then runs the instruction as decoded here, not from its PC.
fn packed_words(step: Step, instruction: WideInstruction, helpers: LaneHelpers) -> Option<[u64; 2]> {
    let covered = step == Step::Helper && crate::dispatch_fast::family_helpers::covers(instruction)
        && call_plan::helper_address(helpers, LaneHelper::StepPacked).is_some();
    covered.then(|| {
        let (first, imm) = instruction.words();
        [first, u64::from(imm)]
    })
}

/// Whether `instruction` is a global name store or `typeof`, a call-out the VM makes in place
/// (`LaneHelper::VmStep`) that writes at most one register.
fn calls_out(instruction: WideInstruction) -> bool {
    let call = quench_runtime::staged::Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY).and_then(|row| row.call_out());
    instruction.result_flags() == 0 && matches!(call, Some((quench_runtime::staged::CallOut::GlobalWrite { .. } | quench_runtime::staged::CallOut::GlobalTypeof { .. }, _)))
}

/// Whether `instruction` makes a fixed-shape object by the lane's allocator (`Step::Allocate`): an empty object or an
/// object literal of a site's shape over its registers, which runs no guest code and writes its one destination.
fn allocates_object(instruction: WideInstruction) -> bool {
    instruction.result_flags() == 0 && matches!(
        quench_runtime::staged::Allocation::decode(instruction, &[]),
        Some((_, quench_runtime::staged::Allocation::Empty | quench_runtime::staged::Allocation::ObjectPair { .. } | quench_runtime::staged::Allocation::ObjectLiteral { .. })))
}

/// The lowering of `instruction`, where helpers are `helpers` and `known` holds the sites' facts: where
/// the helper table has the step helper's address, a `throw` is the raising step, and the other steps
/// made in place are helper steps (behind their fast arm, where they have one); an add (`adds`) is a
/// fast arm of its own, whose miss is the helper step where the instruction has one and the leave where
/// it has none (`Step::Guard`); a name store or `typeof` is a call-out where the table has the VM step
/// helper's address; a fixed-shape object is the allocation step (`allocates_object`); every other instruction, and
/// every one without the address, leaves (`unplaced`).
pub(crate) fn lowering(instruction: WideInstruction, known: Known, helpers: LaneHelpers) -> Step {
    let addressed = |helper| call_plan::helper_address(helpers, helper).is_some();
    if addressed(LaneHelper::AllocateStep) && allocates_object(instruction) {
        return Step::Allocate;
    }
    let called_out = || if addressed(LaneHelper::VmStep) && calls_out(instruction) { Step::CallOut } else { unplaced(helpers) };
    let in_place = addressed(LaneHelper::Step) && steps_in_place(instruction);
    let fast = fast_arm(instruction, known).or_else(|| adds::of(known, instruction).map(adds::Add::heap));
    match fast {
        Some(fast @ Heap::Add { .. }) if !in_place => Step::Guard(fast),
        _ if addressed(LaneHelper::Step) && instruction.op().raises() => Step::Throw,
        _ if !in_place => called_out(),
        Some(fast) => Step::Fast(fast),
        None => Step::Helper,
    }
}

/// The step of an instruction no row takes: the leave to the interpreter, or where an image body runs generic
/// rows (`LaneHelpers::generic_rows`) the generic semantic row, which never leaves.
pub(crate) fn unplaced(helpers: LaneHelpers) -> Step {
    if helpers.generic_rows() { Step::Generic } else { Step::Leave }
}

/// Whether `instruction` at `pc` may jump backward, which only a row that polls the interrupt flag
/// may do: a body polls before every row that branches back (`function::preludes`), a region only
/// at its own backedge.
fn branches_back(pc: u32, instruction: WideInstruction) -> bool {
    instruction.op().control_flow_layout().jumps() && instruction.jump_target() <= pc
}

/// The row of an instruction at `pc` that has no numeric or heap row: a `Nop` is the jump to the
/// next instruction, the rest their step. `polls` where a row before it polls the interrupt flag,
/// as a body's do, so a step that jumps backward may be made in place. A helper step names the register it writes, and a fast
/// arm the register it reads; an add row also names its transition and the atom it holds unnamed.
pub(crate) fn decode_step(pc: u32, instruction: WideInstruction, known: Known, (helpers, polls): (LaneHelpers, bool)) -> Decoded {
    match staged_of(instruction) {
        Some(Operation::Nop) => Decoded::number(Kind::Jump, None, None, None, Some(pc + 1)),
        _ => {
            let step = if branches_back(pc, instruction) && !polls { Step::Leave } else { lowering(instruction, known, helpers) };
            let helped = step.runs_helper();
            let mut written = instruction.written_registers().filter(|_| helped);
            let target = (helped && instruction.op().control_flow_layout().jumps()).then(|| instruction.jump_target());
            let (read, base) = match step {
                Step::Fast(fast) | Step::Guard(fast) => fast_operands(instruction, fast),
                _ => (None, None),
            };
            let right = match step {
                Step::Fast(fast) => fast_right(instruction, fast),
                _ => None,
            };
            let add = adds::of(known, instruction).filter(|add| matches!(step, Step::Fast(fast) | Step::Guard(fast) if fast == add.heap()));
            let name = names::of(known, instruction).filter(|name| matches!(step, Step::Fast(fast) | Step::Guard(fast) if fast == name.heap()));
            let result = written.next();
            let packed = packed_words(step, instruction, helpers);
            Decoded { target, a: read, b: right, base, also: written.next(), add, name, atom: add.map(|_| instruction.atom_index()), packed, ..Decoded::step(step, result) }
        }
    }
}
