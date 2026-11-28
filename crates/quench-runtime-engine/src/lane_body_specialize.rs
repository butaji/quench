//! A lane body's specialization at its function's entries and loops (`specialize_lane_body`):
//! the attempt a view makes at its body, and the state each attempt leaves (`body_after`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// Whether the mode compiles function bodies at all (Cranelift's tiers or the b-jit tier's stencils): `--no-jit` compiles none.
fn mode_compiles_bodies() -> bool {
    crate::compiled::admission::specialize(crate::compiled::admission::UnitRequest { kind: crate::compiled::admission::UnitKind::Body, facts: Default::default() }).is_some()
}

#[quench_macros::ext(LaneViewSpecializeOps)]
impl LaneView {
    /// A mode with neither Cranelift function tiers nor the b-jit tier's stencil bodies compiles no
    /// function body: the view's body is declined, once.
    fn declined_by_mode(&self) -> bool {
        let declined = !mode_compiles_bodies();
        if declined {
            // SAFETY: as `lane_body`.
            unsafe { *self.tier().tier_lane.get() = LaneBody::Declined };
        }
        declined
    }

    /// Specializes the frameless body at the function's entry in `tier`, the tier its predicted pick names
    /// (`tier_policy::lane_body_tier`), or none where the interpreter runs the function (`None`). Whether the body is
    /// still compiling or specialized, so its prologue stays. A specialized body is a fact of the view from then on:
    /// its prologue record enters it with `entry`, which tests nothing the specialization proved (`lane_body_entry`).
    pub(crate) fn specialize_lane_body(&self, entry: Handler, tier: Option<quench_runtime::vm::tier_policy::Tier>, work: impl FnOnce(crate::compiled::LaneBodyWork) -> crate::compiled::LaneBodyOutcome) -> bool {
        if self.declined_by_mode() { return false; }
        self.settle_retired_body();
        // SAFETY: as `lane_body`; specializing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        let outcome = match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized => {
                crate::compiled::base_region::count(crate::compiled::base_region::Event::BodyCompile);
                work(crate::compiled::LaneBodyWork::Start { caller: self })
            }
            LaneBody::Compiling { pending } => work(crate::compiled::LaneBodyWork::Poll(pending)),
            settled => { *body = settled; return !matches!(body, LaneBody::Declined); }
        };
        *body = self.body_after(outcome, tier, entry);
        !matches!(body, LaneBody::Declined)
    }

    /// The body of a function of a program with a loaded AOT image: `install` takes it from the
    /// image (`Vm::install_ahead_body`) at once, with no compile budget and no tier gate, since an image
    /// has no profiling phase. It is the ladder's first body. Whether it has a body.
    pub(crate) fn install_ahead_lane_body(&self, entry: Handler, install: impl FnOnce() -> Option<std::rc::Rc<quench_runtime::vm::engine_types::PublishedLaneMethod>>) -> bool {
        self.settle_retired_body();
        // SAFETY: as `lane_body`; installing runs no lane body, and the body is out of the view while the VM works on it.
        let body = unsafe { &mut *self.tier().tier_lane.get() };
        match std::mem::replace(body, LaneBody::Declined) {
            LaneBody::Unspecialized => {
                let outcome = install().map_or(crate::compiled::LaneBodyOutcome::Declined, crate::compiled::LaneBodyOutcome::Published);
                *body = self.body_after(outcome, Some(quench_runtime::vm::tier_policy::Tier::Ahead), entry);
            }
            settled => *body = settled,
        }
        !matches!(body, LaneBody::Declined)
    }

    /// The body a view's compile work came to (`LaneBodyOutcome`) makes of it; a compile still
    /// in flight is polled again at the view's next entry.
    fn body_after(&self, outcome: crate::compiled::LaneBodyOutcome, tier: Option<quench_runtime::vm::tier_policy::Tier>, entry: Handler) -> LaneBody {
        {
            use crate::compiled::{LaneBodyOutcome as O, base_region::{Event, count}};
            count(match &outcome {
                O::Published(_) => Event::BodyPublished, O::Compiling(_) => Event::BodyCompiling, O::Unpaid => Event::BodyUnpaid,
                O::Declined => Event::BodyDeclined, O::Stale => Event::BodyStale, O::Deferred => Event::BodyDeferred,
            });
        }
        match outcome {
            crate::compiled::LaneBodyOutcome::Compiling(pending) => {
                LaneBody::Compiling { pending }
            }
            crate::compiled::LaneBodyOutcome::Published(published) => {
                // A body is published only on a route its pick admits, which names its tier (`lane_body_tier`).
                self.specialized(entry, published, tier.expect("a published lane body has a compiled tier"))
            }
            // A compile a collection or a changed fact outran is a body a lifecycle edge dropped.
            crate::compiled::LaneBodyOutcome::Stale => LaneBody::Unspecialized,
            crate::compiled::LaneBodyOutcome::Deferred => LaneBody::Unspecialized,
            // Its predicted tier is the interpreter (`lane_facts::body_pick`): the interpreter runs it.
            crate::compiled::LaneBodyOutcome::Unpaid => {
                self.tier().history.set(LaneBodyHistory { unpaid: true, ..self.tier().history.get() });
                LaneBody::Declined
            }
            crate::compiled::LaneBodyOutcome::Declined => LaneBody::Declined,
        }
    }

}
