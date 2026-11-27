//! The per-function tier state machine: the one place that names a function's tier states, the
//! events that move it, and the table of legal transitions. Entry-cell publication
//! (`EntryCell::publish`/`withdraw`) happens only here, as the effect of a transition.
//!
//! Whether a tier may run at all is a separate fact with one home: `tier_policy::tier_may_run`
//! (memory policy and JIT mode). The machine asks `tier_policy::may_compile` and `may_publish` to
//! admit `Compile` and `Publish`, and reads no mode itself.
//!
//! ```text
//! from        event            to
//! Interpreter Warm             Warming
//! Warming     Compile          Compiling
//! Compiling   Publish(tier)    BJit | QJit | UJit      (mode admits the tier; QJit only in --q-jit)
//! Compiling   Abandon          Warming                 (a compile a lifecycle edge outran)
//! published   Demote           Warming                 (respecialize, lifecycle edge, leaves)
//! published   Retire           Retired                 (rejected: waits for a retry)
//! Retired     Rewarm           Warming
//! any live    Decline          Declined                (terminal)
//! ```
//!
//! Existing states map onto it: `LaneBody` Unspecialized/Compiling/Specialized/Declined
//! (`LaneBody::tier_state`), and loop sites (`LoopBody` Idle/Compiling/Ready/Declined) the same way, and `JsTierState` (`js_tier_state`): Hot, PromotionPending and GeneralPromotionPending are
//! Warming, GeneralCompileDue is Compiling, Compiled is UJit, Rejected is Retired, Unsupported
//! is Declined. BaseJIT regions are BJit.
use super::engine_types::EntryCell;
use crate::vm::engine_types::PublishedLaneMethod;
use super::program_store::JsTierState;
use super::tier_policy;
use crate::jit_mode::Tier;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierState {
    Interpreter,
    Warming,
    Compiling,
    BJit,
    QJit,
    UJit,
    Retired,
    Declined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TierEvent {
    Warm,
    Compile,
    Publish(Tier),
    Abandon,
    Demote,
    Retire,
    Rewarm,
    Decline,
}

use TierEvent::*;
use TierState::*;

/// Every legal transition, as data: `(from, event, to)`.
const TRANSITIONS: &[(TierState, TierEvent, TierState)] = &[
    (Interpreter, Warm, Warming),
    (Warming, Compile, Compiling),
    (Compiling, Publish(Tier::Base), BJit),
    (Compiling, Publish(Tier::Quick), QJit),
    (Compiling, Publish(Tier::Ultra), UJit),
    (Compiling, Publish(Tier::Ahead), UJit),
    (Compiling, Abandon, Warming),
    (BJit, Demote, Warming),
    (QJit, Demote, Warming),
    (UJit, Demote, Warming),
    (BJit, Retire, Retired),
    (QJit, Retire, Retired),
    (UJit, Retire, Retired),
    (Retired, Rewarm, Warming),
    (Warming, Decline, Declined),
    (Compiling, Decline, Declined),
    (BJit, Decline, Declined),
    (QJit, Decline, Declined),
    (UJit, Decline, Declined),
    (Retired, Decline, Declined),
];

/// Whether the process's mode admits `event`: a compile or publication of a tier the mode
/// allows, and QuickJIT only where it is the only Cranelift tier (`--q-jit`).
fn admitted(event: TierEvent) -> bool {
    match event {
        Compile => tier_policy::may_compile(),
        Publish(tier) => tier_policy::may_publish(tier),
        _ => true,
    }
}

/// The state `event` takes `from` to, or `None` where the table has no such transition or the
/// mode does not admit it.
pub(super) fn transition(from: TierState, event: TierEvent) -> Option<TierState> {
    if !admitted(event) {
        return None;
    }
    TRANSITIONS.iter().find(|(state, on, _)| *state == from && *on == event).map(|row| row.2)
}

/// Publishes `method`, a body of `tier`, to `cell` as the effect of `Publish` from `from`; the new state, or
/// `None` (nothing published) where the machine does not take the transition.
pub fn publish(cell: &EntryCell, from: TierState, tier: Tier, method: &PublishedLaneMethod) -> Option<TierState> {
    let to = transition(from, Publish(tier))?;
    cell.publish(method);
    Some(to)
}

/// Withdraws the entry cell's body as the effect of `event` (`Demote`, `Retire` or `Decline`)
/// from the published state `from`; the new state.
pub fn withdraw(cell: &EntryCell, from: TierState, event: TierEvent) -> TierState {
    let to = transition(from, event);
    debug_assert!(to.is_some(), "no transition from {from:?} on {event:?}");
    cell.withdraw();
    to.unwrap_or(Warming)
}

/// A JavaScript function tier state's place in the machine.
pub fn js_tier_state(state: Option<&JsTierState>) -> TierState {
    match state {
        None => Interpreter,
        Some(JsTierState::Hot { .. } | JsTierState::PromotionPending) => Warming,
        Some(JsTierState::Compiled(_)) => UJit,
        Some(JsTierState::Rejected { .. }) => Retired,
        Some(JsTierState::Unsupported) => Declined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_is_deterministic_and_quick_is_not_default() {
        for (i, a) in TRANSITIONS.iter().enumerate() {
            assert!(TRANSITIONS[i + 1..].iter().all(|b| (a.0, a.1) != (b.0, b.1)), "{a:?}");
        }
        assert_eq!(transition(Interpreter, Warm), Some(Warming));
        assert_eq!(transition(Compiling, Publish(Tier::Quick)).is_some(), tier_policy::may_publish(Tier::Quick));
        assert_eq!(transition(Declined, Warm), None);
    }
}
