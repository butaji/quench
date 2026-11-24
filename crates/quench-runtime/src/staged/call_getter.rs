//! Composite call row of the getter call family: a read that met an accessor's getter, the
//! `[[Get]]` step that calls it on the base with no arguments (getters-as-calls.md). Stage-generic,
//! no alloc or std, so every tier that meets a getter runs the same decisions; the tier gives the
//! primitives (`GetterCall`).
//!
//! The order, once:
//!
//! - a native getter: a read that writes its register takes the getter's pure value where the
//!   native has one (no call is observable), else calls the native in place; a returning read
//!   (whose completion only an activation makes) activates it;
//! - a script getter: a read that writes its register takes the answer its frameless body gives
//!   by a read (`return k`, `return this`, `return this.f`), memoized for the receiver's layout
//!   where the tier can; anything else activates the getter, its target resolved once.

use super::invoke::{invoke, CallFrame};
use super::{Arguments, Callee, FunctionId, Invocation, PropertyFrame, Receiver, Stage, Step};
use crate::bytecode::Register;

/// Which kind of callee a getter is: a native, or a script function (whose call target the tier
/// resolves once, `GetterAnswer::target`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GetterKind {
    Native,
    Script,
}

/// A tier's primitives for what a getter returns without its call (`step_getter_answer_of`).
pub trait GetterAnswer<S: Stage> {
    /// A script getter's resolved call target.
    type Target;
    /// A value as the tier holds it: the getter, the base and the answer.
    type Value: Copy;
    /// The getter's kind.
    fn kind(&mut self, stage: &mut S, getter: Self::Value) -> GetterKind;
    /// A script getter's call target, where the tier has one.
    fn target(&mut self, stage: &mut S, getter: Self::Value) -> Option<Self::Target>;
    /// A native getter's value for `base` where its call runs no guest code and no effect.
    fn pure_native(&mut self, stage: &mut S, getter: Self::Value, base: Self::Value) -> Option<Self::Value>;
    /// What a script getter's frameless body returns for `base` by a read, where it has such a body.
    fn read_answer(&mut self, stage: &mut S, target: &Self::Target, base: Self::Value) -> Option<Self::Value>;
    /// Records the answer of `getter` on `base` for the receivers of its layout, where the tier can.
    fn memoize(&mut self, stage: &mut S, target: &Self::Target, getter: Self::Value, base: Self::Value);
}

/// A tier's primitives for a getter's call on a base (`step_getter_call_of`).
pub trait GetterCall<S: Stage> {
    /// The tier's answers without the call.
    type Answer: GetterAnswer<S>;
    /// What the read becomes: its value written, an in-place call, or an activation.
    type Out;
    fn answers(&mut self) -> &mut Self::Answer;
    /// The read completes with `value`.
    fn answer(&mut self, stage: &mut S, value: Val<S, Self>) -> Self::Out;
    /// The native getter called in place on `base`.
    fn call_native(&mut self, stage: &mut S, getter: Val<S, Self>, base: Val<S, Self>) -> Self::Out;
    /// The getter called on `base` through the tier's own call path (no call row of this step's:
    /// the lane's is `activate_getter_in_lane`), with its target where it is a script getter.
    fn call(&mut self, stage: &mut S, getter: Val<S, Self>, base: Val<S, Self>, target: Option<<Self::Answer as GetterAnswer<S>>::Target>) -> Self::Out;
}

/// The value type of a getter call's answers.
pub type Val<S, G> = <<G as GetterCall<S>>::Answer as GetterAnswer<S>>::Value;

/// The getter call of a read of `base` that met `getter`; `writes` where the read writes its
/// register (a returning read completes only through the getter's activation).
#[inline(always)]
pub fn step_getter_call_of<S: Stage, G: GetterCall<S>>(stage: &mut S, call: &mut G, getter: Val<S, G>, base: Val<S, G>, writes: bool) -> G::Out {
    match call.answers().kind(stage, getter) {
        GetterKind::Native if writes => match call.answers().pure_native(stage, getter, base) {
            Some(value) => call.answer(stage, value),
            None => call.call_native(stage, getter, base),
        },
        GetterKind::Native => call.call(stage, getter, base, None),
        GetterKind::Script => {
            let target = call.answers().target(stage, getter);
            if writes
                && let Some(known) = &target
                && let Some(value) = call.answers().read_answer(stage, known, base)
            {
                call.answers().memoize(stage, known, getter, base);
                return call.answer(stage, value);
            }
            call.call(stage, getter, base, target)
        }
    }
}

/// What `getter` returns for a read of `base` where its call runs no guest code: a native's pure
/// value, or a script getter's frameless read answer (memoized as `step_getter_call_of` does);
/// `None` where only its call answers.
#[inline(always)]
pub fn step_getter_answer_of<S: Stage, G: GetterAnswer<S>>(stage: &mut S, call: &mut G, getter: G::Value, base: G::Value) -> Option<G::Value> {
    match call.kind(stage, getter) {
        GetterKind::Native => call.pure_native(stage, getter, base),
        GetterKind::Script => {
            let target = call.target(stage, getter)?;
            let value = call.read_answer(stage, &target, base)?;
            call.memoize(stage, &target, getter, base);
            Some(value)
        }
    }
}

/// A script getter's call through the one call row (`invoke`): the getter is the callee, a function
/// of the running program (`function`) whose identity the frame's plan holds, called on the base the
/// read met (`Receiver::Base`) with no arguments, its value the read's register `result`.
#[inline(always)]
pub fn step_getter_invoke_of<S: Stage, F: CallFrame<S> + PropertyFrame<S>>(stage: &mut S, frame: &mut F, miss: S::Miss,
    function: FunctionId, result: Register) -> Step<S::Word, F::Control> {
    let site = Invocation { callee: Callee::Known(function), receiver: Receiver::Base, arguments: Arguments::Window { first: 0, count: 0 },
        result, returns: false, forwards: false };
    invoke(stage, frame, miss, site)
}
