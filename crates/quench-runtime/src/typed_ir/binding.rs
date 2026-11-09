//! Binding times, the proofs that hold facts of each, and Ershov's division over them.

/// The Ershov division of a source fact. Location and Site fix an address or
/// feedback index, never the contents read from it. Stable is heap state that rarely
/// changes (a prototype's value, a shape's layout, a callee's identity): a specializer
/// folds it as a constant only under a code dependency, whose change retires the code
/// instead of a guard at each use (AGENTS.md, Execution laws 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BindingTime {
    Static,
    Location,
    Stable,
    Site,
    Dynamic,
}

/// Proof obligations are part of IR, not inferred from the selected encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Proof {
    Static,
    CanonicalValue,
    EntryGuard,
    Definition,
    /// A code dependency on the fact's subject: its change retires the code that holds it.
    Dependency,
}

/// Which binding times a specializer treats as known (Ershov's division).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Division {
    /// Build-time facts only: static inputs and storage locations. AOT images
    /// and eager publication need no training run, so they never fold feedback.
    Offline,
    /// Also the contents of feedback sites, each behind a guard whose failure
    /// resumes the interpreter at the exact PC, and Stable heap facts, each under a
    /// code dependency whose change retires the code: the JIT's regions.
    Online,
}

impl Division {
    pub(crate) const fn folds(self, time: BindingTime) -> bool {
        match time {
            BindingTime::Static | BindingTime::Location => true,
            BindingTime::Stable | BindingTime::Site => matches!(self, Self::Online),
            BindingTime::Dynamic => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    #[test]
    fn stable_facts_are_proved_by_dependencies_alone() {
        let callee = Type::function(Value::direct_function(7)).unwrap();
        let fact = Fact { ty: callee, representation: Representation::Tagged,
            binding: BindingTime::Stable, proof: Proof::Dependency, projection: Projection::Exact };
        assert!(fact.verify(), "a callee's identity is stable under its dependency");
        for proof in [Proof::Static, Proof::CanonicalValue, Proof::EntryGuard, Proof::Definition] {
            assert!(!Fact { proof, ..fact }.verify(), "{proof:?} does not hold a stable fact");
        }
        for binding in [BindingTime::Static, BindingTime::Location, BindingTime::Site, BindingTime::Dynamic] {
            assert!(!Fact { binding, ..fact }.verify(), "only a stable fact rests on a dependency: {binding:?}");
        }
    }
}
