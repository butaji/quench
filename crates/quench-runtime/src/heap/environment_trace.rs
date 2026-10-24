//! A full collection's trace of environments through the slots their readers can read.
//!
//! An environment reached from anywhere but a closure (a frame, a generator, a binding
//! reference, a module record) is read whole, with every enclosing environment, as code
//! running in it may read any of them. A closure's environment edge is instead traced
//! through `bytecode::ClosureReads`: at each distance out, only the slots the closure's code
//! and the functions nested in it can read, the slots the runtime finds by name, and the
//! lexical `this` an arrow resolves there. A closure whose table cannot say (a dynamic scope,
//! an environment past its program, a layout the table does not describe) reads its chain
//! whole. No reader reads the value of a program root's projection slot: reads of a global
//! var or module import go through it, and stores check only that it is initialized.
//!
//! Once marking ends, every slot that no edge read is cleared (`EnvironmentSlots::
//! clear_unread`): no code can read it, and its old value may be swept. An environment read
//! whole that holds no projection is marked and traced as any other cell, without an entry.

use super::{Cell, EnvironmentBindings, EnvironmentSlotState, EnvironmentSlots, FunctionKind, Heap, SlotUse};
use crate::bytecode::{Atom, ClosureReads, EnvironmentDemand};
use crate::value::Value;
use smallvec::SmallVec;

/// The closure reads of the programs a collection's closures run, from the VM.
pub(crate) trait ClosureReadSource {
    /// The table of the program `program` (`ProgramId::raw`) names, when it has one.
    fn closure_reads(&self, program: u32) -> Option<&ClosureReads>;
    /// The atom of the lexical `this` binding (`LEXICAL_THIS_BINDING`).
    fn lexical_this(&self) -> Atom;
    /// Whether an environment of `function` may hold root projection slots: a program root.
    fn may_hold_projections(&self, function: u32) -> bool;
    /// Whether `slot` of an environment of `function` in `program` is a root projection of a
    /// global var or module import, which every read goes through
    /// (`RootSlotRule::Projection`), rather than a binding.
    fn slot_is_projection(&self, program: u32, function: u32, slot: u16) -> bool;
}

/// The `function` of a function-less environment layer: a `with` object's or a lexical
/// `this` wrapper's, which name lookups and captures pass through.
const FUNCTION_LESS_LAYER: u32 = u32::MAX;

/// What one edge into an environment needs of it.
#[derive(Clone, Copy, Debug)]
enum EnvironmentEdge {
    /// A closure of `function` in `program` reaches the environment `distance` activations
    /// out of its own.
    Closure { environment: Value, program: u32, function: u32, distance: u32 },
    /// A reader of the whole scope chain reaches the environment: a frame, another cell, or
    /// a closure its table does not describe.
    Whole { environment: Value },
    /// Another environment shares `slot` with this one, which owns it.
    Slot { environment: Value, slot: u16 },
    /// Another environment shares `slot` with this one, whose initialization alone is read.
    Initialization { environment: Value, slot: u16 },
    /// Another environment shares its dynamic bindings with this one, and its lexical
    /// `this` is read.
    LexicalThis { environment: Value },
    /// The environment is named by a traced one (as a parent or a sharing owner) and must
    /// stay allocated, whether or not any of its slots is read.
    Keep { environment: Value },
}

impl EnvironmentEdge {
    fn environment(self) -> Value {
        match self {
            Self::Closure { environment, .. }
            | Self::Whole { environment }
            | Self::Slot { environment, .. }
            | Self::Initialization { environment, .. }
            | Self::LexicalThis { environment }
            | Self::Keep { environment } => environment,
        }
    }
}

/// A set of slot indexes, one bit each, inline for an environment of up to one word's slots.
#[derive(Debug)]
struct SlotSet {
    words: SmallVec<[u64; 1]>,
}

impl SlotSet {
    fn new(slots: usize) -> Self {
        Self { words: smallvec::smallvec![0; slots.div_ceil(u64::BITS as usize)] }
    }

    fn contains(&self, slot: usize) -> bool {
        self.words
            .get(slot / u64::BITS as usize)
            .is_some_and(|word| word & (1 << (slot % u64::BITS as usize)) != 0)
    }

    /// Adds `slot`; `false` when it was in the set (or past its slots).
    fn insert(&mut self, slot: usize) -> bool {
        let Some(word) = self.words.get_mut(slot / u64::BITS as usize) else {
            return false;
        };
        let bit = 1 << (slot % u64::BITS as usize);
        let new = *word & bit == 0;
        *word |= bit;
        new
    }
}

/// What the edges traced so far read of an environment.
#[derive(Debug)]
struct PartialEnvironment {
    /// Slots whose value is read.
    read: SlotSet,
    /// Slots whose initialization alone is read (`SlotUse::Initialization`): a program
    /// root's projections, which few environments hold.
    initialization: Option<Box<SlotSet>>,
    lexical_this: bool,
    /// Whether a `Whole` edge reached it.
    whole: bool,
    /// What the closure edges followed through it have cost: one for each edge and its edge
    /// to the parent, and one for each slot its demand walked (`whole_cost`).
    closure_cost: usize,
}

impl PartialEnvironment {
    fn new(slots: usize) -> Self {
        Self {
            read: SlotSet::new(slots),
            initialization: None,
            lexical_this: false,
            whole: false,
            closure_cost: 0,
        }
    }

    /// What tracing an environment of `slots` whole costs, in the units of `closure_cost`:
    /// each slot, and the edge to its parent. Closure edges into one environment may cost
    /// as much again before it is traced whole, so a full collection's environment trace
    /// costs at most about twice the whole-chain trace it refines. A scope that thousands of
    /// functions close over, as a module's is, otherwise walked a demand per function
    /// (typescript-eslint, npm lane 2026-10-24: 24M slot reads over 8 full collections and
    /// a 40% slower run).
    fn whole_cost(slots: usize) -> usize {
        slots + 1
    }

    /// Records `slot` read for its initialization alone; `false` when it already was.
    fn read_initialization(&mut self, slot: usize, slots: usize) -> bool {
        self.initialization
            .get_or_insert_with(|| Box::new(SlotSet::new(slots)))
            .insert(slot)
    }

    fn use_of(&self, slot: usize) -> SlotUse {
        if self.read.contains(slot) {
            SlotUse::Value
        } else if self.initialization.as_ref().is_some_and(|set| set.contains(slot)) {
            SlotUse::Initialization
        } else {
            SlotUse::Unread
        }
    }
}

/// One full collection's environment trace (see the module documentation). Without a
/// source, as in a minor collection, every environment is traced whole as any other cell.
pub(crate) struct EnvironmentTrace<'s> {
    source: Option<&'s dyn ClosureReadSource>,
    /// What the edges traced so far read of each environment they reached, in the order
    /// they reached them.
    partial: Vec<PartialEnvironment>,
    /// Each reached environment's heap slot and its position in `partial`. Sized by the
    /// environments closure edges reached, not the heap: a dense per-slot index touched a
    /// page of it for nearly every environment, at each full collection's peak.
    entries: rustc_hash::FxHashMap<u32, u32>,
    pending: Vec<EnvironmentEdge>,
    /// The closure edges followed, as (environment heap slot, program, function, distance):
    /// closures of one function made in one activation read the same slots, so each is
    /// followed once. One set for the trace, since a module environment reaches thousands
    /// of functions' edges and a per-environment list made each check linear in them.
    followed: rustc_hash::FxHashSet<(u32, u32, u32, u32)>,
    /// The last program whose table a closure edge read, which the next one most often reads.
    last_reads: Option<(u32, Option<&'s ClosureReads>)>,
}


impl<'s> EnvironmentTrace<'s> {
    /// A trace through `source`.
    pub(crate) fn new(source: Option<&'s dyn ClosureReadSource>) -> Self {
        Self {
            source,
            partial: Vec::new(),
            entries: rustc_hash::FxHashMap::default(),
            pending: Vec::new(),
            followed: rustc_hash::FxHashSet::default(),
            last_reads: None,
        }
    }

    /// The position in `partial` of the entry for heap slot `index`.
    fn entry(&self, index: u32) -> Option<usize> {
        self.entries.get(&index).map(|&entry| entry as usize)
    }

    /// Adds `partial` as the entry for heap slot `index`, at the position it returns.
    fn insert(&mut self, index: u32, partial: PartialEnvironment) -> usize {
        let position = self.partial.len();
        self.partial.push(partial);
        self.entries.insert(index, u32::try_from(position).expect("heap slots fit u32"));
        position
    }

    /// A function's edge to its closure environment `environment`.
    pub(super) fn closure(&mut self, environment: Value, kind: &FunctionKind, work: &mut Vec<Value>) {
        match (self.source, kind) {
            (Some(_), FunctionKind::User(program, function)) if environment.legacy_heap_index().is_some() => {
                self.pending.push(EnvironmentEdge::Closure {
                    environment,
                    program: program.raw(),
                    function: *function,
                    distance: 1,
                });
            }
            _ => work.push(environment),
        }
    }

    /// Takes an edge that reads the environment `environment` (heap `index`, of `function`)
    /// whole, when it needs this trace: closure edges reached it first, or it may hold
    /// projections. `false` leaves it to the caller to mark and trace as any other cell.
    pub(super) fn whole(&mut self, environment: Value, index: u32, function: u32) -> bool {
        let Some(source) = self.source else {
            return false;
        };
        let traced = self.entry(index).is_some() || source.may_hold_projections(function);
        if traced {
            self.pending.push(EnvironmentEdge::Whole { environment });
        }
        traced
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    fn closure_reads(&mut self, program: u32) -> Option<&'s ClosureReads> {
        if let Some((cached, reads)) = self.last_reads
            && cached == program
        {
            return reads;
        }
        let reads = self.source.and_then(|source| source.closure_reads(program));
        self.last_reads = Some((program, reads));
        reads
    }
}

/// Records `slot` of an environment with `slots` read, tracing its value or the owner it
/// shares the slot with the first time.
fn read_slot(
    slots: &EnvironmentSlots,
    partial: &mut PartialEnvironment,
    slot: u16,
    work: &mut Vec<Value>,
    pending: &mut Vec<EnvironmentEdge>,
) {
    if !partial.read.insert(usize::from(slot)) {
        return;
    }
    match slots.state(usize::from(slot)) {
        Some(EnvironmentSlotState::Owned(value)) => work.push(value),
        Some(EnvironmentSlotState::Shared(owner)) => {
            pending.push(EnvironmentEdge::Slot { environment: owner, slot });
        }
        None => {}
    }
}

/// Records `slot` of an environment with `slots` read for its initialization alone, which a
/// shared slot reads from its owner.
fn read_initialization(
    slots: &EnvironmentSlots,
    partial: &mut PartialEnvironment,
    slot: u16,
    pending: &mut Vec<EnvironmentEdge>,
) {
    if partial.read_initialization(usize::from(slot), slots.len())
        && let Some(EnvironmentSlotState::Shared(owner)) = slots.state(usize::from(slot))
    {
        pending.push(EnvironmentEdge::Initialization { environment: owner, slot });
    }
}

impl Heap {
    /// Follows the pending environment edges of `trace`, pushing the values they read onto
    /// `work`, with the environments read whole that need no entry.
    pub(super) fn trace_environment_edges(&self, trace: &mut EnvironmentTrace<'_>, work: &mut Vec<Value>) {
        while let Some(edge) = trace.pending.pop() {
            self.trace_environment_edge(trace, edge, work);
        }
    }

    fn trace_environment_edge(&self, trace: &mut EnvironmentTrace<'_>, edge: EnvironmentEdge, work: &mut Vec<Value>) {
        let environment = edge.environment();
        let (Some(index), Some(Cell::Environment { parent, function, slots, scope })) =
            (environment.legacy_heap_index(), self.get(environment))
        else {
            work.push(environment);
            return;
        };
        let (parent, function, program) = (*parent, *function, scope.program());
        if let EnvironmentEdge::Closure { program, function: closure, distance, .. } = edge
            && (trace.entry(index).is_some_and(|entry| trace.partial[entry].whole)
                || !trace.followed.insert((index, program, closure, distance)))
        {
            return;
        }
        // A closure edge resolves against the environment's layout: the slots its table
        // names when the environment is the activation the table expects there, its chain
        // whole otherwise. A function-less layer passes the edge on, as captures pass it.
        let demand = match edge {
            EnvironmentEdge::Closure { program: closure_program, function: closure, distance, .. }
                if function != FUNCTION_LESS_LAYER =>
            {
                match trace.closure_reads(closure_program).map(|reads| reads.demand(closure, distance)) {
                    Some(demand @ EnvironmentDemand::Slots { owner, .. })
                        if owner == function && program == Some(closure_program) =>
                    {
                        Some(demand)
                    }
                    _ => {
                        work.push(environment);
                        return;
                    }
                }
            }
            _ => None,
        };
        let lexical_this = trace.source.map(|source| source.lexical_this());
        let entry = match trace.entry(index) {
            Some(entry) => entry,
            None => {
                if !Self::mark_serial(&self.marks, index as usize) {
                    // Marked without an entry: already traced whole.
                    return;
                }
                // What every traced environment keeps whatever its readers: the environments it
                // names stay allocated, and `with` objects and dynamic bindings other than the
                // lexical `this` stay traced. Closure and whole edges go on to the parent.
                let unmarked = |value: Value| {
                    value
                        .legacy_heap_index()
                        .is_some_and(|index| !Self::marked(&self.marks, index as usize))
                };
                if !matches!(edge, EnvironmentEdge::Closure { .. } | EnvironmentEdge::Whole { .. }) && unmarked(parent) {
                    trace.pending.push(EnvironmentEdge::Keep { environment: parent });
                }
                if let Some(owner) = slots.default_owner().filter(|owner| unmarked(*owner)) {
                    trace.pending.push(EnvironmentEdge::Keep { environment: owner });
                }
                match scope.dynamic_bindings() {
                    EnvironmentBindings::Owned(bindings) => work.extend(
                        bindings
                            .iter()
                            .filter(|(atom, _)| Some(*atom) != lexical_this)
                            .map(|(_, value)| *value),
                    ),
                    EnvironmentBindings::Shared(owner) => {
                        if unmarked(*owner) {
                            trace.pending.push(EnvironmentEdge::Keep { environment: *owner });
                        }
                    }
                }
                work.extend(scope.with_objects().iter().copied());
                trace.insert(index, PartialEnvironment::new(slots.len()))
            }
        };
        let partial = &mut trace.partial[entry];
        let pending = &mut trace.pending;
        let has_parent = parent.legacy_heap_index().is_some();
        let read_lexical_this = match edge {
            EnvironmentEdge::Closure { program, function: closure, distance, .. } => {
                let walked = match demand {
                    Some(demand @ EnvironmentDemand::Slots { .. }) => demand.slot_count(),
                    _ => slots.len(),
                };
                partial.closure_cost += 1 + walked;
                if partial.closure_cost > PartialEnvironment::whole_cost(slots.len()) {
                    pending.push(EnvironmentEdge::Whole { environment });
                    return;
                }
                match demand {
                    Some(demand @ EnvironmentDemand::Slots { lexical_this, .. }) => {
                        for slot in demand.slot_iter() {
                            read_slot(slots, partial, slot, work, pending);
                        }
                        if has_parent {
                            pending.push(EnvironmentEdge::Closure {
                                environment: parent,
                                program,
                                function: closure,
                                distance: distance + 1,
                            });
                        }
                        lexical_this
                    }
                    _ => {
                        // A function-less layer.
                        for slot in (0..slots.len()).filter_map(|slot| u16::try_from(slot).ok()) {
                            read_slot(slots, partial, slot, work, pending);
                        }
                        if has_parent {
                            pending.push(EnvironmentEdge::Closure { environment: parent, program, function: closure, distance });
                        }
                        true
                    }
                }
            }
            EnvironmentEdge::Whole { .. } => {
                if std::mem::replace(&mut partial.whole, true) {
                    return;
                }
                let source = trace.source.expect("edges are traced only with a source");
                for slot in (0..slots.len()).filter_map(|slot| u16::try_from(slot).ok()) {
                    // A projection's reads go through it, and its stores check only that it
                    // is initialized.
                    if program.is_some_and(|program| source.slot_is_projection(program, function, slot)) {
                        read_initialization(slots, partial, slot, pending);
                    } else {
                        read_slot(slots, partial, slot, work, pending);
                    }
                }
                if has_parent {
                    work.push(parent);
                }
                true
            }
            EnvironmentEdge::Slot { slot, .. } => {
                read_slot(slots, partial, slot, work, pending);
                false
            }
            EnvironmentEdge::Initialization { slot, .. } => {
                read_initialization(slots, partial, slot, pending);
                false
            }
            EnvironmentEdge::LexicalThis { .. } => true,
            EnvironmentEdge::Keep { .. } => false,
        };
        if read_lexical_this && !std::mem::replace(&mut partial.lexical_this, true) {
            match scope.dynamic_bindings() {
                EnvironmentBindings::Owned(bindings) => work.extend(
                    bindings
                        .iter()
                        .filter(|(atom, _)| Some(*atom) == lexical_this)
                        .map(|(_, value)| *value),
                ),
                EnvironmentBindings::Shared(owner) => {
                    pending.push(EnvironmentEdge::LexicalThis { environment: *owner });
                }
            }
        }
    }

    /// Clears what no edge read of the environments the trace reached.
    pub(super) fn clear_unread_environment_slots(&mut self, trace: EnvironmentTrace<'_>) {
        let lexical_this = trace.source.map(|source| source.lexical_this());
        for (&index, &entry) in &trace.entries {
            let partial = &trace.partial[entry as usize];
            let Some(Cell::Environment { slots, scope, .. }) =
                self.slots.get_mut(index as usize).and_then(|slot| slot.cell.as_mut())
            else {
                continue;
            };
            slots.clear_unread(|slot| partial.use_of(slot));
            if partial.lexical_this {
                continue;
            }
            let unread_this = matches!(
                scope.dynamic_bindings(),
                EnvironmentBindings::Owned(bindings)
                    if bindings.iter().any(|(atom, value)| Some(*atom) == lexical_this && !value.is_undefined())
            );
            if unread_this
                && let EnvironmentBindings::Owned(bindings) = &mut scope.make_mut().dynamic_bindings
            {
                for (atom, value) in bindings.iter_mut() {
                    if Some(*atom) == lexical_this {
                        *value = Value::UNDEFINED;
                    }
                }
            }
        }
    }
}
