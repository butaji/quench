//! The staged rows of the operations no other row covers: a Number operation the planner's
//! operator tables do not name (`%`, a relation's value, `~`), `InitializeTdz`, `void`, a
//! value-form equality, and the null/undefined guard of `RequireObjectCoercible`. Each row builds
//! the staged `Operation` over the stencil's places (`DEST`, `LEFT`, `RIGHT`, `places.rs`) and runs
//! its family step (`step_number`, `step_local`, `step_value`, `step_generic`) on `StencilStage`,
//! so the semantics are the interpreter's. A row that does not complete leaves through EXIT0 with
//! the registers as they were; the planner lays the helper step after it.
//!
//! The call-out row (`staged_callout`) is the leave for the operations that need an allocation, a
//! guest call or a host callout (`Throw`, `Return`, `Await`, `Yield`, `Allocate`, ...): it writes
//! nothing and leaves the registers as they came, as `exit_here` (`call_rows.rs`) does.
//!
//! The rows are heap rows: `library.rs` `HEAP_USES` names their places (entry false), `build.rs` instantiates each as
//! a `heap_row!`, and `heap_rows::run` routes the `STAGED_FIRST..=STAGED_LAST` codes (`codes.rs`) to `run` below, a hit
//! hopping past the helper step the planner lays behind the row. `staged_row!` is unused: it would define a second symbol
//! for a row `heap_name` already names.

use crate::bytecode::Operand;
use crate::codes::{PINF, STAGED_BITWISE_NOT, STAGED_COERCIBLE, STAGED_LOOSE_EQ, STAGED_INIT_LOCAL, STAGED_LOOSE_NE, STAGED_RELVAL_EQ, STAGED_RELVAL_GE, STAGED_RELVAL_GT, STAGED_RELVAL_LE, STAGED_RELVAL_LT, STAGED_RELVAL_NE, STAGED_REMAINDER, STAGED_STRICT_EQ, STAGED_STRICT_NE, STAGED_VOID};
use crate::places::{DEST, LEFT, Places, RIGHT};
use crate::stage::StencilStage;
use crate::staged::{Concrete, Domain, NumberOperation, Operation, Step, step_generic, step_local, step_number, step_value};
use crate::vm::operations::NumberRelation;
use crate::words::{HOLE_A, Out, S, hole};

/// The step's word where it completed (it wrote its destination), else the row leaves.
#[inline(always)]
fn finish<const D: u8, const A: u8, const B: u8>(frame: Places<D, A, B>, done: bool) -> Out {
    if frame.writes_fit(done) { Out::Next(frame.s) } else { Out::Exit(frame.s) }
}

/// Whether one of a row's places is a pinned double, so the row computes in binary64.
const fn wide(d: u8, a: u8, b: u8) -> bool {
    d >= PINF || a >= PINF || b >= PINF
}

/// The Number operation over the row's places, whether it completed.
#[inline(always)]
fn number<const D: u8, const A: u8, const B: u8>(frame: &mut Places<D, A, B>, operation: NumberOperation) -> bool {
    matches!(step_number(&mut Concrete::from_hole(0), frame, Operation::Number(operation)), Step::Value(_))
}

/// The relation a value-form relation row of code `kind` computes.
const fn value_relation_of(kind: u8) -> NumberRelation {
    match kind {
        STAGED_RELVAL_LT => NumberRelation::LessThan,
        STAGED_RELVAL_LE => NumberRelation::LessEqual,
        STAGED_RELVAL_GT => NumberRelation::GreaterThan,
        STAGED_RELVAL_GE => NumberRelation::GreaterEqual,
        STAGED_RELVAL_EQ => NumberRelation::Equal,
        _ => NumberRelation::NotEqual,
    }
}

/// `Binary` `%` of the two operands into the destination (`NumberOperation::Remainder`). The int32
/// computation leaves where its result is no int32 (a zero divisor's NaN, a negative dividend's
/// `-0`), so the binary64 computation takes the row then, as the arithmetic rows do.
#[inline(always)]
pub(crate) fn remainder_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let remainder = |domain: Domain| NumberOperation::Remainder { destination: DEST, left, right, domain };
    let done = if wide(D, A, B) {
        number(&mut frame, remainder(Domain::Float64))
    } else if frame.both_int() {
        frame.prove_ints();
        number(&mut frame, remainder(Domain::Int32)) || number(&mut frame, remainder(Domain::Float64))
    } else {
        number(&mut frame, remainder(Domain::Float64))
    };
    finish(frame, done)
}

/// A relation as a value (`Binary` of `<`, `<=`, `>`, `>=`, `==`, `!=`): its Boolean into the destination
/// (`NumberOperation::RelationValue`). The destination is a frame slot (a Boolean is no int word).
#[inline(always)]
pub(crate) fn relation_value_row<const K: u8, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let (left, right, relation) = (Operand::register(LEFT), Operand::register(RIGHT), value_relation_of(K));
    let value = |domain: Domain| NumberOperation::RelationValue { destination: DEST, left, right, relation, domain };
    let done = if wide(D, A, B) {
        number(&mut frame, value(Domain::Float64))
    } else if frame.both_int() {
        frame.prove_ints();
        number(&mut frame, value(Domain::Int32))
    } else {
        number(&mut frame, value(Domain::Float64))
    };
    finish(frame, done)
}

/// `~` of the operand into the destination as an int32 (`NumberOperation::BitwiseNot`); a Number that is no
/// int32 leaves.
#[inline(always)]
pub(crate) fn bitwise_not_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let done = number(&mut frame, NumberOperation::BitwiseNot { destination: DEST, source: Operand::register(LEFT) });
    finish(frame, done)
}

/// `InitializeTdz` of local slot `hole 1` (`Operation::InitializeLocal`): the slot takes the deleted marker. The
/// row reads no operand place, so it is instantiated over `(MEM, MEM, MEM)` only.
#[inline(always)]
pub(crate) fn initialize_local_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let slot = hole::<HOLE_A>() as u16;
    let done = matches!(step_local(&mut Concrete::from_hole(0), &mut frame, Operation::InitializeLocal { slot }), Step::Value(_));
    finish(frame, done)
}

/// `RequireObjectCoercible` of the operand (`Operation::RequireObjectCoercible`): continues where the operand is
/// neither `null` nor `undefined` (it writes nothing); a nullish operand leaves for the helper step, which raises.
#[inline(always)]
pub(crate) fn coercible_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let done = matches!(step_value(&mut Concrete::from_hole(0), &mut frame, Operation::RequireObjectCoercible { register: LEFT }), Step::Value(_));
    finish(frame, done)
}

/// `void` of anything into the destination (`Operation::Void`, `undefined` as the constant). The destination is a
/// frame slot (`undefined` is no int word). Needs `Places::constant` to answer the constant (places.rs).
#[inline(always)]
pub(crate) fn void_row<const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let done = matches!(step_generic(&mut Concrete::from_hole(0), &mut frame, Operation::Void { destination: DEST }), Step::Value(_));
    finish(frame, done)
}

/// The value-form equality of the two operands (`Operation::Equality`), negated where `NEGATED`: the Boolean where the
/// two words decide it (`generic::decide`); any other pair (a string, a BigInt, an object) leaves for the VM.
/// The destination is a frame slot.
#[inline(always)]
pub(crate) fn equality_row<const STRICT: bool, const NEGATED: bool, const D: u8, const A: u8, const B: u8>(mut frame: Places<D, A, B>) -> Out {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let done = matches!(step_generic(&mut Concrete::from_hole(0), &mut frame,
        Operation::Equality { destination: DEST, left, right, strict: STRICT, negated: NEGATED }), Step::Value(_));
    finish(frame, done)
}

/// The call-out row: the operation needs an allocation, a guest call or a host callout, so the row writes
/// nothing and leaves through EXIT0 with the registers as they came; the helper step after it runs the operation.
#[inline(always)]
pub(crate) fn staged_callout(s: S) -> Out {
    Out::Exit(s)
}

/// The staged row of code `KIND` over the places `D`, `A`, `B`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    match KIND {
        STAGED_REMAINDER => remainder_row::<D, A, B>(Places::new(s)),
        STAGED_RELVAL_LT..=STAGED_RELVAL_NE => relation_value_row::<KIND, D, A, B>(Places::new(s)),
        STAGED_BITWISE_NOT => bitwise_not_row::<D, A, B>(Places::new(s)),
        STAGED_INIT_LOCAL => initialize_local_row::<D, A, B>(Places::new(s)),
        STAGED_COERCIBLE => coercible_row::<D, A, B>(Places::new(s)),
        STAGED_VOID => void_row::<D, A, B>(Places::new(s)),
        STAGED_LOOSE_EQ => equality_row::<false, false, D, A, B>(Places::new(s)),
        STAGED_LOOSE_NE => equality_row::<false, true, D, A, B>(Places::new(s)),
        STAGED_STRICT_EQ => equality_row::<true, false, D, A, B>(Places::new(s)),
        STAGED_STRICT_NE => equality_row::<true, true, D, A, B>(Places::new(s)),
        _ => staged_callout(s),
    }
}
