//! The staged rows of the operations no other row covers: a Number operation the planner's
//! operator tables do not name (`%`, a relation's value, `~`), `InitializeTdz`, `void`, a
//! value-form equality, and the null/undefined guard of `RequireObjectCoercible`. Each row builds
//! the staged `Operation` over the stencil's places (`DEST`, `LEFT`, `RIGHT`, `places.rs`) and runs
//! its family step (`step_number`, `step_local`, `step_value`, `step_generic`) on `StencilStage`,
//! so the semantics are the interpreter's. A row that does not complete leaves through EXIT0 with
//! the registers as they were; the planner lays the helper step after it.
//!
//! The call-out arm (`exit_here`, `words.rs`) is the leave for the operations that need an allocation, a
//! guest call or a host callout (`Throw`, `Return`, `Await`, `Yield`, `Allocate`, ...): it writes
//! nothing and leaves the registers as they came, as the call rows' exit does (`call_rows.rs`).
//!
//! The rows are heap rows: `row_catalog.rs` names their places (entry false), `build.rs` instantiates each as
//! a `heap_row!`, and `heap_rows::run` routes the `STAGED_FIRST..=STAGED_LAST` codes (`codes.rs`) to `run` below, a hit
//! hopping past the helper step the planner lays behind the row. `staged_row!` is unused: it would define a second symbol
//! for a row `heap_name` already names.

use crate::bytecode::Operand;
use crate::codes::{STAGED_BITWISE_NOT, STAGED_COERCIBLE, STAGED_LOOSE_EQ, STAGED_INIT_LOCAL, STAGED_LOOSE_NE, STAGED_RELVAL_EQ, STAGED_RELVAL_GE, STAGED_RELVAL_GT, STAGED_RELVAL_LE, STAGED_RELVAL_LT, STAGED_RELVAL_NE, STAGED_REMAINDER, STAGED_STRICT_EQ, STAGED_STRICT_NE, STAGED_VOID};
use crate::numeric::{Kind, any_double};
use crate::places::{DEST, LEFT, Places, RIGHT, finish};
use crate::stage::StencilStage;
use crate::staged::{Concrete, Domain, NegativeZero, NumberOperation, Operation, Step, numeric, op_kind, step_generic_of, step_local_of, step_value_of};
use crate::vm::operations::NumberRelation;
use crate::words::{HOLE_A, Out, S, exit_here, hole};

/// The Number operation over the row's places, whether it completed.
#[inline(always)]
fn number<const KIND: u8>(frame: &mut Places, operation: NumberOperation, _: Kind<KIND>) -> bool {
    // `step_number`'s arm for a Number operation, with the variant `KIND` known (`numeric::step_of`).
    matches!(numeric::step_of::<KIND, _, _>(&mut Concrete::from_hole(0), frame, operation), Step::Value(_))
}

/// The Number relation a value-form relation row of code `kind` computes: the staged relation's own variant, the one
/// table the operator rows define (`vm/operations/number_rows.rs`).
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

/// `Binary` `%` of the two operands into the destination (`NumberOperation::Remainder`): the int32
/// computation, while both operand words are ints. It leaves where its result is no int32 (a zero
/// divisor's NaN, a negative dividend's `-0`) and for a double: the binary64 remainder is libm's
/// `fmod`, a call a stencil cannot hold, so the VM's step takes the row then.
#[inline(always)]
pub(crate) fn remainder_row<const D: u8, const A: u8, const B: u8>(mut frame: Places) -> Out {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let remainder = NumberOperation::Remainder { destination: DEST, left, right, domain: Domain::Int32,
        zero: NegativeZero::Exit, divisor_nonzero: false };
    let done = if const { !any_double(D, A, B) } && frame.prove_ints() {
        number(&mut frame, remainder, Kind::<{ numeric::kind::REMAINDER }>)
    } else {
        false
    };
    finish(frame, done)
}

/// A relation as a value (`Binary` of `<`, `<=`, `>`, `>=`, `==`, `!=`): its Boolean into the destination
/// (`NumberOperation::RelationValue`). The destination is a frame slot (a Boolean is no int word).
#[inline(always)]
pub(crate) fn relation_value_row<const K: u8, const D: u8, const A: u8, const B: u8>(mut frame: Places) -> Out {
    let (left, right, relation) = (Operand::register(LEFT), Operand::register(RIGHT), value_relation_of(K));
    let value = |domain: Domain| NumberOperation::RelationValue { destination: DEST, left, right, relation, domain };
    let done = if const { any_double(D, A, B) } {
        number(&mut frame, value(Domain::Float64), Kind::<{ numeric::kind::RELATION_VALUE }>)
    } else if frame.prove_ints() {
        number(&mut frame, value(Domain::Int32), Kind::<{ numeric::kind::RELATION_VALUE }>)
    } else {
        number(&mut frame, value(Domain::Float64), Kind::<{ numeric::kind::RELATION_VALUE }>)
    };
    finish(frame, done)
}

/// `~` of the operand into the destination as an int32 (`NumberOperation::BitwiseNot`); a Number that is no
/// int32 leaves.
#[inline(always)]
pub(crate) fn bitwise_not_row(mut frame: Places) -> Out {
    let done = number(&mut frame, NumberOperation::BitwiseNot { destination: DEST, source: Operand::register(LEFT) }, Kind::<{ numeric::kind::BITWISE_NOT }>);
    finish(frame, done)
}

/// `InitializeTdz` of local slot `hole 1` (`Operation::InitializeLocal`): the slot takes the deleted marker. The
/// row reads no operand place, so it is instantiated over `(MEM, MEM, MEM)` only.
#[inline(always)]
pub(crate) fn initialize_local_row(mut frame: Places) -> Out {
    let slot = hole::<HOLE_A>() as u16;
    let done = matches!(step_local_of::<{ op_kind::INITIALIZE_LOCAL }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::InitializeLocal { slot }), Step::Value(_));
    finish(frame, done)
}

/// `RequireObjectCoercible` of the operand (`Operation::RequireObjectCoercible`): continues where the operand is
/// neither `null` nor `undefined` (it writes nothing); a nullish operand leaves for the helper step, which raises.
#[inline(always)]
pub(crate) fn coercible_row(mut frame: Places) -> Out {
    let done = matches!(step_value_of::<{ op_kind::REQUIRE_OBJECT_COERCIBLE }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::RequireObjectCoercible { register: LEFT }), Step::Value(_));
    finish(frame, done)
}

/// `void` of anything into the destination (`Operation::Void`, `undefined` as the constant). The destination is a
/// frame slot (`undefined` is no int word). Needs `Places::constant` to answer the constant (places.rs).
#[inline(always)]
pub(crate) fn void_row(mut frame: Places) -> Out {
    let done = matches!(step_generic_of::<{ op_kind::VOID }, _, _>(&mut Concrete::from_hole(0), &mut frame, Operation::Void { destination: DEST }), Step::Value(_));
    finish(frame, done)
}

/// The value-form equality of the two operands (`Operation::Equality`), negated where `NEGATED`: the Boolean where the
/// two words decide it (`generic::decide`); any other pair (a string, a BigInt, an object) leaves for the VM.
/// The destination is a frame slot.
#[inline(always)]
pub(crate) fn equality_row<const STRICT: bool, const NEGATED: bool>(mut frame: Places) -> Out {
    let (left, right) = (Operand::register(LEFT), Operand::register(RIGHT));
    let done = matches!(step_generic_of::<{ op_kind::EQUALITY }, _, _>(&mut Concrete::from_hole(0), &mut frame,
        Operation::Equality { destination: DEST, left, right, strict: STRICT, negated: NEGATED }), Step::Value(_));
    finish(frame, done)
}

/// The staged row of code `KIND` over the places `D`, `A`, `B`.
#[inline(always)]
pub(crate) fn run<const KIND: u8, const D: u8, const A: u8, const B: u8>(s: S) -> Out {
    if const { KIND == STAGED_REMAINDER } {
        remainder_row::<D, A, B>(Places::new::<D, A, B>(s))
    } else if const { (STAGED_RELVAL_LT <= KIND && KIND <= STAGED_RELVAL_NE) } {
        relation_value_row::<KIND, D, A, B>(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_BITWISE_NOT } {
        bitwise_not_row(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_INIT_LOCAL } {
        initialize_local_row(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_COERCIBLE } {
        coercible_row(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_VOID } {
        void_row(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_LOOSE_EQ } {
        equality_row::<false, false>(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_LOOSE_NE } {
        equality_row::<false, true>(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_STRICT_EQ } {
        equality_row::<true, false>(Places::new::<D, A, B>(s))
    } else if const { KIND == STAGED_STRICT_NE } {
        equality_row::<true, true>(Places::new::<D, A, B>(s))
    } else {
        exit_here(s)
    }
}
