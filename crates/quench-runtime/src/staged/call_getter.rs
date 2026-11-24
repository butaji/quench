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

use super::Stage;

/// Which kind of callee a getter is, as the tier resolved it: a native, or a script function with
/// its call target where the tier has one.
pub enum GetterKind<T> {
    Native,
    Script(Option<T>),
}

/// A tier's primitives for what a getter returns without its call (`step_getter_answer_of`).
pub trait GetterAnswer<S: Stage> {
    /// A script getter's resolved call target.
    type Target;
    /// The getter's kind, and a script getter's target.
    fn kind(&mut self, stage: &mut S, getter: S::Word) -> GetterKind<Self::Target>;
    /// A native getter's value for `base` where its call runs no guest code and no effect.
    fn pure_native(&mut self, stage: &mut S, getter: S::Word, base: S::Word) -> Option<S::Word>;
    /// What a script getter's frameless body returns for `base` by a read, where it has such a body.
    fn read_answer(&mut self, stage: &mut S, target: &Self::Target, base: S::Word) -> Option<S::Word>;
    /// Records the answer of `getter` on `base` for the receivers of its layout, where the tier can.
    fn memoize(&mut self, stage: &mut S, target: &Self::Target, getter: S::Word, base: S::Word);
}

/// A tier's primitives for a getter's call on a base (`step_getter_call_of`).
pub trait GetterCall<S: Stage> {
    /// The tier's answers without the call.
    type Answer: GetterAnswer<S>;
    /// What the read becomes: its value written, an in-place call, or an activation.
    type Out;
    fn answers(&mut self) -> &mut Self::Answer;
    /// The read completes with `value`.
    fn answer(&mut self, stage: &mut S, value: S::Word) -> Self::Out;
    /// The native getter called in place on `base`.
    fn call_native(&mut self, stage: &mut S, getter: S::Word, base: S::Word) -> Self::Out;
    /// The getter activated on `base`, with its target where it is a script getter.
    fn activate(&mut self, stage: &mut S, getter: S::Word, base: S::Word, target: Option<<Self::Answer as GetterAnswer<S>>::Target>) -> Self::Out;
}

/// The getter call of a read of `base` that met `getter`; `writes` where the read writes its
/// register (a returning read completes only through the getter's activation).
#[inline(always)]
pub fn step_getter_call_of<S: Stage, G: GetterCall<S>>(stage: &mut S, call: &mut G, getter: S::Word, base: S::Word, writes: bool) -> G::Out {
    match call.answers().kind(stage, getter) {
        GetterKind::Native if writes => match call.answers().pure_native(stage, getter, base) {
            Some(value) => call.answer(stage, value),
            None => call.call_native(stage, getter, base),
        },
        GetterKind::Native => call.activate(stage, getter, base, None),
        GetterKind::Script(target) => {
            if writes
                && let Some(known) = &target
                && let Some(value) = call.answers().read_answer(stage, known, base)
            {
                call.answers().memoize(stage, known, getter, base);
                return call.answer(stage, value);
            }
            call.activate(stage, getter, base, target)
        }
    }
}

/// What `getter` returns for a read of `base` where its call runs no guest code: a native's pure
/// value, or a script getter's frameless read answer (memoized as `step_getter_call_of` does);
/// `None` where only its call answers.
#[inline(always)]
pub fn step_getter_answer_of<S: Stage, G: GetterAnswer<S>>(stage: &mut S, call: &mut G, getter: S::Word, base: S::Word) -> Option<S::Word> {
    match call.kind(stage, getter) {
        GetterKind::Native => call.pure_native(stage, getter, base),
        GetterKind::Script(Some(target)) => {
            let value = call.read_answer(stage, &target, base)?;
            call.memoize(stage, &target, getter, base);
            Some(value)
        }
        GetterKind::Script(None) => None,
    }
}
