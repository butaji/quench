//! Guarded and keyed steps: the suspended result, the guard steps and their keyed forms.

use super::*;

/// The control a frame answers (`Frame::raise`), or `Unresolved` where it answers none.
#[inline(always)]
fn suspended<W, C>(control: Option<C>) -> Step<W, C> {
    match control {
        Some(control) => Step::Control(control),
        None => Step::Unresolved,
    }
}

/// A tier guard, or a taken back edge's poll and count (`step`): `Step::Value` where the proof holds,
/// `Step::Unresolved` where the row leaves.
#[inline(always)]
pub(super) fn guard_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    let miss = frame.miss();
    let held = match operation {
        Operation::GuardIdentity { source, expected } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_IDENTITY]) } { super::super::guard::identity(stage, source, expected, miss) } else { unreachable!() },
        Operation::GuardLive => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_LIVE]) } { super::super::guard::live(stage, frame, miss) } else { unreachable!() },
        Operation::GuardRealm { expected } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_REALM]) } { super::super::guard::realm(stage, frame, expected, miss) } else { unreachable!() },
        Operation::GuardDeclared { source, mask } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_DECLARED]) } { super::super::guard::declared(stage, source, mask, miss) } else { unreachable!() },
        Operation::GuardPrototype { source, records, shape, link } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_PROTOTYPE]) } { super::super::guard::prototype(stage, records, source, (shape, link), miss).map(drop) } else { unreachable!() },
        Operation::GuardShape { source, records, shape, form } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_SHAPE]) } { super::super::guard::shape(stage, records, source, (shape, form), miss).map(drop) } else { unreachable!() },
        Operation::GuardClass { source, class } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_CLASS]) } { super::super::guard::class(stage, source, class, miss).map(drop) } else { unreachable!() },
        Operation::GuardStores { specialized } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::GUARD_STORES]) } { super::super::guard::stores(stage, specialized, miss) } else { unreachable!() },
        Operation::BackEdge { proof } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::BACK_EDGE]) } { super::super::guard::back_edge(stage, frame, proof) } else { unreachable!() },
        _ => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::GUARD_IDENTITY, super::super::op_kind::GUARD_LIVE, super::super::op_kind::GUARD_REALM, super::super::op_kind::GUARD_DECLARED, super::super::op_kind::GUARD_PROTOTYPE, super::super::op_kind::GUARD_SHAPE, super::super::op_kind::GUARD_CLASS, super::super::op_kind::GUARD_STORES, super::super::op_kind::BACK_EDGE]) } { unreachable!("`guard_step` sends only guards and back edges") } else { unreachable!() },
    };
    match held {
        Some(()) => Step::Value(stage.constant(0)),
        None => Step::Unresolved,
    }
}

/// `guard_step_of` for any operation.
#[inline(always)]
fn guard_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    guard_step_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}

/// A delete or an element access: the keyed operations (`step`).
#[inline(always)]
pub(super) fn keyed_step_of<const KIND: u16, S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    match operation {
        Operation::Delete { destination, base, key, strict } => if const { super::super::op_kind::admits(KIND, &[super::super::op_kind::DELETE]) } { super::super::delete::step(stage, frame, destination, [base, key], strict) } else { unreachable!() },
        element => if const { super::super::op_kind::outside(KIND, &[super::super::op_kind::DELETE]) } { super::super::element_rows::step_of::<KIND, _, _>(stage, frame, element) } else { unreachable!() },
    }
}

/// `keyed_step_of` for any operation.
#[inline(always)]
fn keyed_step<S: NumericStage, F: NumericFrame<S>>(stage: &mut S, frame: &mut F, operation: Operation<S::Word, S::Proof>) -> Step<S::Word, F::Control> {
    keyed_step_of::<{ super::super::op_kind::ANY }, S, F>(stage, frame, operation)
}
