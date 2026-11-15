//! Building a module's inlining plan: the leaves of each round, and the plan's decisions kept for a
//! lowering that resumes it (`LeafPlan::facts`, `LeafPlan::resume`).

use super::*;

impl<'m, 'a, I: Operators<'a>> LeafPlan<'m, 'a, I> {
    pub(super) fn new(
        name: &'m str,
        sources: &'m dyn SourceTable<'a, I>,
        bindings: Bindings<'m>,
        signatures: &WasmSignatures,
        budget: usize,
    ) -> Result<Self, Diagnostic> {
        let mut plan = Self {
            name,
            sources,
            bindings,
            signatures: std::rc::Rc::new(Callees::new(signatures)),
            rounds: std::rc::Rc::default(),
            budget,
            marker: std::marker::PhantomData,
        };
        for round in 0..INLINE_ROUNDS {
            // Only the kinds are kept: a lowering builds the forms it copies.
            let kinds = (0..sources.count())
                .map(|index| match plan.rounds.last() {
                    Some(previous) if previous.kinds[index] == LeafKind::None => Ok(LeafKind::None),
                    _ => plan.leaf(round, index, budget).map(|leaf| match leaf {
                        None => LeafKind::None,
                        Some(leaf) if leaf.form.calls => LeafKind::Calls,
                        Some(_) => LeafKind::CallFree,
                    }),
                })
                .collect::<Result<Box<[_]>, _>>()?;
            if kinds.iter().all(|kind| *kind == LeafKind::None) {
                break;
            }
            std::rc::Rc::get_mut(&mut plan.rounds)
                .expect("a plan being made shares no rounds")
                .push(Round { built: std::cell::RefCell::new(BuiltLeaves::new(kinds.len())), kinds });
        }
        Ok(plan)
    }

    /// The plan's decisions, for `resume`: which functions are leaves of each round, without
    /// the forms this plan built.
    pub(super) fn facts(&self) -> PlanFacts {
        PlanFacts {
            signatures: self.signatures.clone(),
            rounds: std::rc::Rc::new(
                self.rounds
                    .iter()
                    .map(|round| Round { kinds: round.kinds.clone(), built: std::cell::RefCell::new(BuiltLeaves::new(round.kinds.len())) })
                    .collect(),
            ),
            budget: self.budget,
        }
    }

    /// The plan `facts` were taken from, reading the same bodies from `sources`.
    pub(super) fn resume(
        name: &'m str,
        sources: &'m dyn SourceTable<'a, I>,
        bindings: Bindings<'m>,
        facts: &PlanFacts,
    ) -> Self {
        Self {
            name,
            sources,
            bindings,
            signatures: facts.signatures.clone(),
            rounds: facts.rounds.clone(),
            budget: facts.budget,
            marker: std::marker::PhantomData,
        }
    }
}
