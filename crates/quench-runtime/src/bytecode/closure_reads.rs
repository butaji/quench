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
    /// The function it is nested in, or `NO_PARENT` for a program's root.
    parent: u32,
    pub(super) is_arrow: bool,
    /// The (distance, slot) pairs the function and every function nested in it read of the
    /// environments outside the function, sorted; `None` for the whole chain.
    pub(super) reads: Option<Box<[(u32, u16)]>>,
    pub(super) named_slots: Box<[u16]>,
}

impl FunctionReads {
    /// The `parent` of a program's root function, which nests in none.
    pub(super) const NO_PARENT: u32 = u32::MAX;

    pub(super) fn new(parent: Option<u32>, is_arrow: bool, reads: Option<Box<[(u32, u16)]>>, named_slots: Box<[u16]>) -> Self {
        Self { parent: parent.unwrap_or(Self::NO_PARENT), is_arrow, reads, named_slots }
    }

    /// The function this one is nested in, if any.
    pub(super) fn enclosing(&self) -> Option<u32> {
        (self.parent != Self::NO_PARENT).then_some(self.parent)
    }
}

/// Each function's closure reads (`FunctionReads`), indexed by function id. A program's
/// encoding carries it (`binary::write_closure_reads`), so a cached program reads as its
/// compilation does.
#[derive(Clone, Debug, Default)]
pub(crate) struct ClosureReads {
    pub(super) functions: Box<[FunctionReads]>,
    /// Each function's `ResidualProgram::closure_takes_no_environment`, filled per function on its
    /// first closure creation and read after; never serialized.
    pub(super) environment_elision: std::cell::OnceCell<Box<[std::cell::Cell<u64>]>>,
}

/// Each function's elision answer takes two bits: whether it is known, and whether it takes none.
const ELISION_BITS_PER_FUNCTION: usize = 2;
const ELISION_FUNCTIONS_PER_WORD: usize = u64::BITS as usize / ELISION_BITS_PER_FUNCTION;
const ELISION_KNOWN: u64 = 0b01;
const ELISION_TAKES_NONE: u64 = 0b10;
const ELISION_FIELD: u64 = ELISION_KNOWN | ELISION_TAKES_NONE;

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

    /// How many slots `slot_iter` yields.
    pub(crate) fn slot_count(&self) -> usize {
        match *self {
            Self::Whole => 0,
            Self::Slots { slots, named_slots, .. } => slots.len() + named_slots.len(),
        }
    }
}

/// What a closure reads of the activation that makes it (`ClosureReads::maker_reads`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MakerReads {
    /// A slot, a by-name binding or the lexical `this` of the maker's environment, or a
    /// whole scope chain: the closure takes the maker's environment.
    Maker,
    /// Nothing of the maker's environment, nor of any further out: any environment serves.
    Nothing,
    /// Nothing of the maker's environment, but slots further out: an environment of the maker
    /// holding none of its slots, under the maker's parent, serves, at the distances its code
    /// reads.
    Outer,
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
                .map(|(function, reads)| FunctionReads::new(
                    function.parent,
                    function.is_arrow,
                    reads.map(|mut reads| {
                        reads.sort_unstable();
                        reads.dedup();
                        reads.into_boxed_slice()
                    }),
                    function.named_slots.into(),
                ))
                .collect(),
            environment_elision: Default::default(),
        }
    }

    /// What a closure of `function` reads of the activation that makes it (`MakerReads`):
    /// its demand of the environment one activation out (`demand`), and whether it reads
    /// any further out. `reads_lexical` answers whether an arrow's chain reads the implicit
    /// bindings it would resolve there (`arrow_chain_reads_implicit_bindings`), which the
    /// table assumes of every arrow; it walks the arrow's code, so only an arrow that reads
    /// no slot of its maker asks it.
    pub(crate) fn maker_reads(&self, function: u32, reads_lexical: impl FnOnce() -> bool) -> MakerReads {
        match self.demand(function, 1) {
            EnvironmentDemand::Slots { slots: [], named_slots: [], lexical_this, .. } if !(lexical_this && reads_lexical()) => {
                let reads = self.functions.get(function as usize).and_then(|closure| closure.reads.as_deref());
                match reads {
                    Some([]) => MakerReads::Nothing,
                    Some(_) => MakerReads::Outer,
                    None => MakerReads::Maker,
                }
            }
            _ => MakerReads::Maker,
        }
    }

    /// The (distance, slot) pairs `function` and the functions nested in it capture, sorted, and
    /// its parent; `None` where the table does not describe the function or it reads its whole
    /// chain (`FunctionReads::reads`).
    pub(crate) fn captures(&self, function: u32) -> Option<(&[(u32, u16)], Option<u32>)> {
        let reads = self.functions.get(function as usize)?;
        Some((reads.reads.as_deref()?, reads.enclosing()))
    }

    /// Whether a closure of `function` reads nothing of the environment it closes over, so it
    /// may take none (`ResidualProgram::closure_takes_no_environment`). The function and every function
    /// nested in it read no slot outside themselves (`reads` is empty, not the whole chain),
    /// so no capture reaches an environment. No ancestor holds named slots, which a by-name
    /// lookup walking the chain would find (home objects, private names). An arrow reads no
    /// implicit binding of its enclosing non-arrow (`reads_lexical`).
    pub(crate) fn environment_unobservable(&self, function: u32, reads_lexical: impl FnOnce() -> bool) -> bool {
        let Some(closure) = self.functions.get(function as usize) else {
            return false;
        };
        if !closure.reads.as_deref().is_some_and(|reads| reads.is_empty()) {
            return false;
        }
        if closure.is_arrow && reads_lexical() {
            return false;
        }
        let mut owner = closure.enclosing();
        while let Some(id) = owner {
            let Some(ancestor) = self.functions.get(id as usize) else {
                return false;
            };
            if !ancestor.named_slots.is_empty() {
                return false;
            }
            owner = ancestor.enclosing();
        }
        closure.enclosing().is_some()
    }

    /// Whether a closure of `function` takes no environment, where `derive` answers the
    /// program's part of that (`ResidualProgram::closure_takes_no_environment`). Asked once per
    /// function: its answer is a bit read on every later closure creation.
    pub(crate) fn takes_no_environment(&self, function: u32, derive: impl FnOnce() -> bool) -> bool {
        let words = self.environment_elision.get_or_init(|| {
            let bits = self.functions.len() * ELISION_BITS_PER_FUNCTION;
            vec![std::cell::Cell::new(0); bits.div_ceil(u64::BITS as usize)].into_boxed_slice()
        });
        let function = function as usize;
        let Some(word) = words.get(function / ELISION_FUNCTIONS_PER_WORD) else {
            return derive();
        };
        let shift = (function % ELISION_FUNCTIONS_PER_WORD) * ELISION_BITS_PER_FUNCTION;
        let field = (word.get() >> shift) & ELISION_FIELD;
        if field & ELISION_KNOWN != 0 {
            return field & ELISION_TAKES_NONE != 0;
        }
        let takes = derive();
        let answer = ELISION_KNOWN | if takes { ELISION_TAKES_NONE } else { 0 };
        word.set(word.get() | (answer << shift));
        takes
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
            match self.functions.get(owner as usize).and_then(FunctionReads::enclosing) {
                Some(parent) => owner = parent,
                None => return EnvironmentDemand::Whole,
            }
        }
        let Some(owner_reads) = self.functions.get(owner as usize) else {
            return EnvironmentDemand::Whole;
        };
        if owner_reads.enclosing().is_none() {
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

impl crate::bytecode::ResidualProgram {
    /// Whether a closure of function `id` takes `Value::NULL` for its environment: nothing it
    /// can run reads its environment, so the scope chain it would keep is unobservable and dies
    /// with its maker. Only the program's facts decide it, so it is one cached bit per function.
    /// No `with` check is made at the creation site: a `with` entered in a frame belongs to that
    /// frame's function, whose `contains_with` makes every function nested in it dynamic
    /// (`capture_layout::dynamic_name_resolution`), so the closure's reads are the whole chain
    /// and it is not elided.
    #[inline(always)]
    pub fn closure_takes_no_environment(&self, id: u32) -> bool {
        self.closure_reads.as_deref().is_some_and(|reads| {
            reads.takes_no_environment(id, || self.derive_closure_takes_no_environment(id, reads))
        })
    }

    /// `closure_takes_no_environment` before its cache: not for an eval's program, nor a
    /// call-only closure, whose environment rides in its value (`Value::known_closure` takes a
    /// direct environment only). A module's direct children take none like any other: a hoisted
    /// declaration is found by its instance's table (`ModuleFunctions`) before this applies.
    fn derive_closure_takes_no_environment(&self, id: u32, reads: &ClosureReads) -> bool {
        if self.kind == crate::bytecode::ProgramKind::Eval {
            return false;
        }
        let Some(slot) = self.functions.get(id as usize) else {
            return false;
        };
        let header = slot.header();
        if header.prototype_unobservable || header.parent.is_none() {
            return false;
        }
        reads.environment_unobservable(id, || crate::bytecode::arrow_chain_reads_implicit_bindings(self, id))
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
    fn closures_reading_nothing_take_no_environment() {
        // 0 root { 1 reads nothing; 2 captures root's slot 0 } and 3 nests 4 which reads nothing
        let mut functions = vec![
            function(None, false, &[]),
            function(Some(0), false, &[]),
            function(Some(0), false, &[(1, 0)]),
            function(Some(0), false, &[]),
            function(Some(3), false, &[]),
        ];
        let reads = ClosureReads::derive(&functions);
        assert!(reads.environment_unobservable(1, || false));
        assert!(reads.environment_unobservable(4, || false));
        assert!(!reads.environment_unobservable(2, || false));
        // The root is no closure, and an arrow reading implicit bindings keeps its environment.
        assert!(!reads.environment_unobservable(0, || false));
        functions[1].is_arrow = true;
        let reads = ClosureReads::derive(&functions);
        assert!(!reads.environment_unobservable(1, || true));
        assert!(reads.environment_unobservable(1, || false));
        // A named slot of an ancestor, which a by-name lookup of the chain finds, keeps it.
        let named = [1u16];
        functions[0].named_slots = &named;
        let reads = ClosureReads::derive(&functions);
        assert!(!reads.environment_unobservable(4, || false));
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
