//! The call signature of an operation: its step, callee signature, call form and tail-call site.

use super::*;

/// One step of a function's code as the fold sees it: an operation's graded effect row (`signatures::site_grade`),
/// whose leave is its `can_leave`, or a call, which stands for the signature joined over the callees it may reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Row { effects: Effects },
    Call(Signature),
}

/// The composed effect signature of a function: the leaves its steps can cause and the effects they
/// have together. Composing steps is Kleisli composition in an effect-graded monad: each step is
/// graded by its row, sequencing joins the rows, and a later step's leave is judged against the
/// writes of the steps before it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signature {
    pub leaves: Leaves,
    pub effects: Effects,
}

impl Signature {
    /// Bottom: no leave and no effect. A function whose steps no solve has seen yet has it.
    pub const NEVER: Self = Self { leaves: Leaves::Never, effects: Effects::NONE };
    /// Top: any leave and any effect. A call whose targets the facts do not name has it.
    pub const ANY: Self = Self { leaves: Leaves::Materialize, effects: Effects::ANY };

    /// The component-wise join: the larger leave and the union of the effect rows.
    pub fn join(self, other: Self) -> Self {
        Self { leaves: self.leaves.join(other.leaves), effects: self.effects.join(other.effects) }
    }

    /// The signature of `steps` in code order. A call step's leave is its callee's, judged after
    /// the writes of the steps before it: a callee that restarts is only a restart before any write.
    pub fn of_steps(steps: impl IntoIterator<Item = Step>) -> Self {
        let mut signature = Self::NEVER;
        let mut written = false;
        for step in steps {
            let (leaves, effects) = match step {
                Step::Row { effects } => (leave_of(effects.can_leave(), written), effects),
                Step::Call(callee) => {
                    let leaves = match callee.leaves {
                        Leaves::Never => Leaves::Never,
                        _ if written => Leaves::Materialize,
                        leaves => leaves,
                    };
                    (leaves, callee.effects)
                }
            };
            signature = signature.join(Self { leaves, effects });
            written |= effects.writes != RegionSet::NONE || effects.elements != ElementWrites::Nothing || effects.may_alloc;
        }
        signature
    }
}

/// How a call site is lowered, by one rule in every tier (interpreter, b-jit, u-jit, AOT): an
/// inlinable callee is inlined; a callee that cannot leave the activation, collect or run guest code
/// needs no frame, so it is called frameless; any other callee is called with a frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallForm {
    Inline,
    Frameless,
    Framed,
}

/// The call form of a call whose callee has signature `callee`.
pub fn call_form(callee: Signature, inlinable: bool) -> CallForm {
    if inlinable {
        CallForm::Inline
    } else if callee.leaves == Leaves::Never && !callee.effects.may_collect && !callee.effects.may_call_js {
        CallForm::Frameless
    } else {
        CallForm::Framed
    }
}

/// Whether a call site is a tail site, by one rule in every tier: the call's completion is its activation's
/// (`returns`: the record carries the return flag, so the callee's value is the caller's result), and the caller
/// has nothing to do after the callee returns (`caller_done`: no handler covers the call, and the caller is
/// strict, which leaves no `with` scope open and no `arguments` object mapped to its parameters). A tier that
/// takes a tail site replaces the caller's frame with the callee's (`CallPlan::tail`), so a chain of tail calls
/// does not grow the stack. The callee's own kind (async or generator) is a separate fact of the callee.
pub const fn tail_site(returns: bool, caller_done: bool) -> bool {
    returns && caller_done
}

impl<W> CallOperation<W> {
    /// The effect row of a call-protocol step: the guards leave and write nothing, the window
    /// writes the callee's registers, and a site names a call, which may run anything.
    pub(super) const fn effects(&self) -> Effects {
        match self {
            Self::Callee { .. } | Self::Receiver { .. } | Self::BindThis { .. } | Self::Room { .. } | Self::Complete { .. } => Effects::NONE,
            Self::Window { .. } => Effects::writes(Region::Frame),
            Self::Site(_) => Effects::ANY,
        }
    }
}
