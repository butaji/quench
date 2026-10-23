//! What a closure can read of the environments it closes over.
//!
//! A closure's environment is its defining activation's, and every closure that activation
//! makes shares it. The slots each closure's code can read are fixed by its bytecode: its
//! `LoadCapture`/`StoreCapture` instructions, and those of the functions nested in it (which
//! it may still create), name a slot of the environment some distance out. A collection
//! traces a closure's environments through those slots alone (`heap::EnvironmentTrace`), so
//! a binding only a dead sibling closure read, or a local no closure reads, dies with its
//! activation.
//!
//! The table is derived once per program from the compiler's per-function capture facts
//! (`compile::capture_layout::FunctionFacts`), the same authority the activation layout
//! reads. A function that resolves names dynamically, or that is nested in or encloses such
//! a function, reads its whole scope chain: direct `eval`, `with` and binding sites may name
//! any binding there.

/// One function's inputs to `ClosureReads::derive`.
pub(crate) struct FunctionReadFacts<'a> {
    pub(crate) parent: Option<u32>,
    pub(crate) is_arrow: bool,
    /// The (distance, slot) of each capture in the function's own code: the slot of the
    /// environment `distance` activations out, where distance 1 is the closure's own.
    pub(crate) captures: &'a [(u32, u16)],
    /// Slots of the function's own activation that the runtime finds by name rather than by
    /// capture (`local_name_read_by_name`): home objects and declared compiler bindings.
    pub(crate) named_slots: &'a [u16],
    /// Whether a by-name lookup may reach the function's scope chain
    /// (`capture_layout::dynamic_name_scopes`).
    pub(crate) dynamic: bool,
    /// The function a class constructor runs on its own closure environment to define its
    /// instance's fields (`Function::instance_initializer`), whose reads are the
    /// constructor's too. It is the constructor's sibling, so distances agree.
    pub(crate) instance_initializer: Option<u32>,
}

#[derive(Clone, Debug)]
pub(super) struct FunctionReads {
    pub(super) parent: Option<u32>,
    pub(super) is_arrow: bool,
    /// The (distance, slot) pairs the function and every function nested in it read of the
    /// environments outside the function, sorted; `None` for the whole chain.
    pub(super) reads: Option<Box<[(u32, u16)]>>,
    pub(super) named_slots: Box<[u16]>,
}

/// Each function's closure reads (`FunctionReads`), indexed by function id. A program's
/// encoding carries it (`binary::write_closure_reads`), so a cached program reads as its
/// compilation does.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClosureReads {
    pub(super) functions: Box<[FunctionReads]>,
}

/// What a closure needs of the environment `distance` activations out.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EnvironmentDemand<'a> {
    /// Every binding and enclosing environment: a dynamic scope, a program root (global
    /// lexical and module bindings are read by name and by importers), an environment past
    /// the closure's program, or a closure the table does not describe.
    Whole,
    /// The environment of function `owner`: its `slots` (sorted) and by-name `named_slots`,
    /// and its lexical `this` when the closure is an arrow nested in `owner` through arrows
    /// alone, which resolves `this` there.
    Slots {
        owner: u32,
        slots: &'a [(u32, u16)],
        named_slots: &'a [u16],
        lexical_this: bool,
    },
}

impl<'a> EnvironmentDemand<'a> {
    /// The slots demanded, `slots` then `named_slots`.
    pub(crate) fn slot_iter(&self) -> impl Iterator<Item = u16> + 'a {
        let (slots, named): (&'a [(u32, u16)], &'a [u16]) = match *self {
            Self::Whole => (&[], &[]),
            Self::Slots { slots, named_slots, .. } => (slots, named_slots),
        };
        slots.iter().map(|&(_, slot)| slot).chain(named.iter().copied())
    }
}

impl ClosureReads {
    /// The table for `functions`, indexed by function id.
    pub(crate) fn derive<'f>(functions: &[FunctionReadFacts<'f>]) -> Self {
        let mut reads: Vec<Option<Vec<(u32, u16)>>> = functions
            .iter()
            .map(|function| (!function.dynamic).then(Vec::new))
            .collect();
        for (id, function) in functions.iter().enumerate() {
            for &(distance, slot) in function.captures {
                // The capture leaves each enclosing function `levels` out at distance
                // `distance - levels` from it, until it reaches the owner of the slot.
                let mut current = Some(id);
                for levels in 0..distance {
                    let Some(enclosing) = current else { break };
                    if let Some(Some(reads)) = reads.get_mut(enclosing) {
                        reads.push((distance - levels, slot));
                    }
                    current = functions[enclosing].parent.map(|parent| parent as usize);
                }
            }
        }
        for (id, function) in functions.iter().enumerate() {
            let Some(initializer) = function.instance_initializer else {
                continue;
            };
            let initializer_reads = reads.get(initializer as usize).cloned().flatten();
            match (initializer_reads, &mut reads[id]) {
                (Some(initializer_reads), Some(reads)) => reads.extend(initializer_reads),
                (_, reads) => *reads = None,
            }
        }
        Self {
            functions: functions
                .iter()
                .zip(reads)
                .map(|(function, reads)| FunctionReads {
                    parent: function.parent,
                    is_arrow: function.is_arrow,
                    reads: reads.map(|mut reads| {
                        reads.sort_unstable();
                        reads.dedup();
                        reads.into_boxed_slice()
                    }),
                    named_slots: function.named_slots.into(),
                })
                .collect(),
        }
    }

    /// What a closure of `function` needs of the environment `distance` (at least 1)
    /// activations out of its own.
    pub(crate) fn demand(&self, function: u32, distance: u32) -> EnvironmentDemand<'_> {
        let Some(closure) = self.functions.get(function as usize) else {
            return EnvironmentDemand::Whole;
        };
        let Some(reads) = closure.reads.as_deref() else {
            return EnvironmentDemand::Whole;
        };
        // An arrow resolves `this` through its chain up to the first enclosing non-arrow,
        // whose activation binds it (`Function::activation_bindings_observable`).
        let mut owner = function;
        let mut lexical_this = closure.is_arrow;
        for level in 0..distance {
            if level != 0 {
                lexical_this &= self.functions.get(owner as usize).is_some_and(|function| function.is_arrow);
            }
            match self.functions.get(owner as usize).and_then(|function| function.parent) {
                Some(parent) => owner = parent,
                None => return EnvironmentDemand::Whole,
            }
        }
        let Some(owner_reads) = self.functions.get(owner as usize) else {
            return EnvironmentDemand::Whole;
        };
        if owner_reads.parent.is_none() {
            return EnvironmentDemand::Whole;
        }
        let start = reads.partition_point(|&(at, _)| at < distance);
        let end = reads.partition_point(|&(at, _)| at <= distance);
        EnvironmentDemand::Slots {
            owner,
            slots: &reads[start..end],
            named_slots: &owner_reads.named_slots,
            lexical_this,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClosureReads, EnvironmentDemand, FunctionReadFacts};

    fn function<'a>(parent: Option<u32>, is_arrow: bool, captures: &'a [(u32, u16)]) -> FunctionReadFacts<'a> {
        FunctionReadFacts { parent, is_arrow, captures, named_slots: &[], dynamic: false, instance_initializer: None }
    }

    fn slots(demand: EnvironmentDemand<'_>) -> Option<(u32, Vec<u16>, bool)> {
        match demand {
            EnvironmentDemand::Whole => None,
            EnvironmentDemand::Slots { owner, lexical_this, .. } => {
                Some((owner, demand.slot_iter().collect(), lexical_this))
            }
        }
    }

    #[test]
    fn closure_reads_follow_each_closure_and_its_nested_functions() {
        // 0 root { 1 outer(slots 0, 1, 2) { 2 sibling => slot 0; 3 kept() {}; 4 middle() { 5 inner => slot 1 of outer } } }
        let named = [2];
        let mut functions = vec![
            function(None, false, &[]),
            function(Some(0), false, &[]),
            function(Some(1), true, &[(1, 0)]),
            function(Some(1), false, &[]),
            function(Some(1), false, &[]),
            function(Some(4), true, &[(2, 1)]),
        ];
        functions[1].named_slots = &named;
        let reads = ClosureReads::derive(&functions);
        assert_eq!(slots(reads.demand(2, 1)), Some((1, vec![0, 2], true)));
        assert_eq!(slots(reads.demand(3, 1)), Some((1, vec![2], false)));
        assert_eq!(slots(reads.demand(4, 1)), Some((1, vec![1, 2], false)));
        // The inner arrow reads `this` from `middle`, a non-arrow, not from `outer`.
        assert_eq!(slots(reads.demand(5, 2)), Some((1, vec![1, 2], false)));
        assert_eq!(slots(reads.demand(5, 1)), Some((4, vec![], true)));
        // The root's environment holds global bindings, read by name.
        assert!(slots(reads.demand(2, 2)).is_none());
    }

    #[test]
    fn class_constructors_read_what_their_instance_initializers_read() {
        // 0 root { 1 enclosing(slots 0, 1) { 2 constructor; 3 initializer => slot 1 } }
        let mut functions = vec![
            function(None, false, &[]),
            function(Some(0), false, &[]),
            function(Some(1), false, &[]),
            function(Some(1), false, &[(1, 1)]),
        ];
        functions[2].instance_initializer = Some(3);
        let reads = ClosureReads::derive(&functions);
        assert_eq!(slots(reads.demand(2, 1)), Some((1, vec![1], false)));
    }

    #[test]
    fn dynamic_scopes_read_their_whole_chain() {
        let mut functions = vec![
            function(None, false, &[]),
            function(Some(0), false, &[]),
            function(Some(1), true, &[(1, 0)]),
        ];
        functions[2].dynamic = true;
        let reads = ClosureReads::derive(&functions);
        assert!(slots(reads.demand(2, 1)).is_none());
        assert!(slots(ClosureReads::default().demand(2, 1)).is_none());
    }
}
