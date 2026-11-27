//! What an operation reads, writes and may do, as data (Stage v2): the alias regions a native
//! load or store belongs to, the effect row of an `Operation`. A
//! specializer reads them to keep facts across operations (a fact about a region survives
//! until an operation that writes the region), and the interpreter ignores them.


use super::{Allocation, CallOperation, Operation};
pub use super::region::Region;

/// A set of regions, one bit each, so a fact's invalidation is a mask intersection.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct RegionSet(pub(crate) u16);

impl RegionSet {
    pub(crate) const NONE: Self = Self(0);

    pub(crate) const fn of(region: Region) -> Self {
        Self(1 << region as u16)
    }

    pub(crate) const fn with(self, region: Region) -> Self {
        Self(self.0 | 1 << region as u16)
    }

    pub(crate) const fn contains(self, region: Region) -> bool {
        self.0 & 1 << region as u16 != 0
    }

    pub(crate) const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}

/// An operation's effect row. `may_collect` is separate from `may_alloc`: an allocation whose
/// fast path is inline does not collect; only a slow helper and a safepoint do.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Effects {
    pub(crate) reads: RegionSet,
    pub(crate) writes: RegionSet,
    pub(crate) may_alloc: bool,
    pub(crate) may_collect: bool,
    pub(crate) may_call_js: bool,
    pub(crate) may_throw: bool,
}

impl Effects {
    pub(crate) const NONE: Self = Self { reads: RegionSet::NONE, writes: RegionSet::NONE,
        may_alloc: false, may_collect: false, may_call_js: false, may_throw: false };

    pub(super) const fn reads(region: Region) -> Self { Self { reads: RegionSet::of(region), ..Self::NONE } }

    pub(super) const fn writes(region: Region) -> Self { Self { writes: RegionSet::of(region), ..Self::NONE } }

    /// An operation that may run guest code or collect: it reads and writes every region.
    pub(super) const ANY: Self = Self { reads: RegionSet(!0), writes: RegionSet(!0), may_alloc: true,
        may_collect: true, may_call_js: true, may_throw: true };

    /// The union of two rows: a region is read or written if either row reads or writes it, and a
    /// flag holds if either does, so a sequence of steps keeps every effect any of them may have.
    pub(crate) const fn join(self, other: Self) -> Self {
        Self { reads: RegionSet(self.reads.0 | other.reads.0), writes: RegionSet(self.writes.0 | other.writes.0),
            may_alloc: self.may_alloc || other.may_alloc, may_collect: self.may_collect || other.may_collect,
            may_call_js: self.may_call_js || other.may_call_js, may_throw: self.may_throw || other.may_throw }
    }

    /// The regions an object's shape, property slots, record words and payload live in: a write to one of them, outside an
    /// allocation (which writes only the new object's words), changes an existing object's exact shape or prototype link.
    const OBJECT_REGIONS: RegionSet = RegionSet::of(Region::ShapeWord).with(Region::ObjectSlots).with(Region::Records)
        .with(Region::Payload).with(Region::HeapMeta);

    /// Whether a row with this effect may change an existing object's exact shape or prototype link: it may run guest code (a
    /// getter, a setter, a proxy trap), or it writes an object's shape, slots or record words without allocating.
    pub const fn reshapes(self) -> bool {
        self.may_call_js || (!self.may_alloc && self.writes.intersects(Self::OBJECT_REGIONS))
    }

    /// Whether a row with this effect reads and nothing else: it writes no region, runs no guest code and allocates nothing.
    pub const fn reads_alone(self) -> bool {
        self.writes.0 == RegionSet::NONE.0 && !self.may_call_js && !self.may_alloc
    }

    /// Whether a row with this effect may rewrite the run's stores word: guest code may run in a nested run and specialize or
    /// unspecialize stores. An allocation runs no guest code, so it clears nothing.
    pub const fn clears_stores(self) -> bool {
        self.may_call_js
    }

    /// Whether a step with this row can leave the activation on its own, by its row alone: it may throw,
    /// run guest code or collect. The facts decide a step's leave before this row rule is used
    /// (`signatures::step_leave`); a step the facts leave open has its leave flags cleared.
    pub(crate) const fn can_leave(self) -> bool {
        self.may_throw || self.may_call_js || self.may_collect
    }

    /// The row with its leave flags cleared: the effects a step keeps once the facts decide it cannot leave.
    /// Reads, writes and allocation stay, since the step still touches what it touches.
    pub(crate) const fn without_leaves(self) -> Self {
        Self { may_collect: false, may_call_js: false, may_throw: false, ..self }
    }
}

/// How far a step can take the running activation out of native code (a leave). The lattice is a
/// chain: a function that cannot leave is `Never`; one that can leave only before its first effect
/// is `RestartCall`, so the call starts again from its entry; one that can leave after an effect
/// needs its frame rebuilt from the registers, `Materialize`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Leaves {
    /// No step can leave the activation under the facts.
    Never,
    /// The activation may leave only before its first write or allocation, so the call restarts.
    RestartCall,
    /// A leave needs the frame rebuilt.
    Materialize,
}

impl Leaves {
    /// The larger of the two: the chain's join is its maximum.
    pub const fn join(self, other: Self) -> Self {
        if other as u8 > self as u8 { other } else { self }
    }
}

/// How a step that can leave (`can_leave`, decided by the settled facts or, where no fact decides it,
/// by its row) leaves, given whether an earlier step of the same function wrote or allocated. Before any
/// write or allocation nothing the call did is visible, so the call restarts from its entry; after one,
/// the frame must be rebuilt. Every tier reads this one rule.
const fn leave_of(can_leave: bool, written_before: bool) -> Leaves {
    match (can_leave, written_before) {
        (false, _) => Leaves::Never,
        (true, false) => Leaves::RestartCall,
        (true, true) => Leaves::Materialize,
    }
}

/// One step of a function's code as the fold sees it: an operation's own effect row with whether the
/// settled facts let it leave (`signatures::step_leave`), or a call, which stands for the signature
/// joined over the callees it may reach.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Row { effects: Effects, can_leave: bool },
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
    pub const fn join(self, other: Self) -> Self {
        Self { leaves: self.leaves.join(other.leaves), effects: self.effects.join(other.effects) }
    }

    /// The signature of `steps` in code order. A call step's leave is its callee's, judged after
    /// the writes of the steps before it: a callee that restarts is only a restart before any write.
    pub fn of_steps(steps: impl IntoIterator<Item = Step>) -> Self {
        let mut signature = Self::NEVER;
        let mut written = false;
        for step in steps {
            let (leaves, effects) = match step {
                Step::Row { effects, can_leave } => (leave_of(can_leave, written), effects),
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
            written |= effects.writes != RegionSet::NONE || effects.may_alloc;
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

impl<W> CallOperation<W> {
    /// The effect row of a call-protocol step: the guards leave and write nothing, the window
    /// writes the callee's registers, and a site names a call, which may run anything.
    const fn effects(&self) -> Effects {
        match self {
            Self::Callee { .. } | Self::Receiver { .. } | Self::BindThis { .. } | Self::Room { .. } | Self::Complete { .. } => Effects::NONE,
            Self::Window { .. } => Effects::writes(Region::Frame),
            Self::Site(_) => Effects::ANY,
        }
    }
}

impl<W, P> Operation<W, P> {
    /// Whether the operation may move an array's backing or the record table: it adds or removes
    /// elements (writes `Region::Elements`), allocates (a new record block) or may run guest code,
    /// which can do either. Native code leaves before a collection. An element read's host arm
    /// answers by a pure lookup or leaves where the VM would run a getter, so a read does not.
    pub fn moves_arrays(&self) -> bool {
        let effects = self.effects();
        !matches!(self, Self::ElementRead { .. }) && (effects.writes.contains(Region::Elements) || effects.may_alloc || effects.may_call_js)
    }

    /// Whether the operation may create or replace the running activation's environment, which its environment storage word
    /// names: a closure's allocation promotes the activation's captured locals into one (it allocates and writes
    /// `Region::Frame`), and anything that may run any code is conservative. A guard, a read or a store does not.
    pub fn makes_environment(&self) -> bool {
        let effects = self.effects();
        effects.may_alloc && effects.writes.contains(Region::Frame)
    }

    /// Whether the operation may change an existing object's exact shape or prototype link (`Effects::reshapes`).
    pub fn reshapes(&self) -> bool {
        self.effects().reshapes()
    }

    /// Whether the operation reads and nothing else (`Effects::reads_alone`).
    pub fn reads_alone(&self) -> bool {
        self.effects().reads_alone()
    }

    /// Whether the operation may rewrite the run's stores word (`Effects::clears_stores`).
    pub fn clears_stores(&self) -> bool {
        self.effects().clears_stores()
    }

    /// The effect row of the operation, from its variant alone.
    pub(crate) const fn effects(&self) -> Effects {
        match self.memory_effects() {
            Some(effects) => effects,
            None => self.control_effects(),
        }
    }

    /// Effects of the field, element and allocation operations.
    const fn memory_effects(&self) -> Option<Effects> {
        Some(match self {
            Self::IndexWrite { .. } => Effects::writes(Region::Elements),
            Self::ElementRead { .. } => Effects { reads: RegionSet::of(Region::Elements), may_call_js: true, may_throw: true, ..Effects::NONE },
            Self::ElementWrite { .. } => Effects::ANY,
            Self::DefineElement { .. } => Effects::writes(Region::Elements),
            // A closure promotes the activation's captured locals into its environment.
            Self::Allocate { allocation: Allocation::Closure { .. }, .. } => Effects {
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots).with(Region::Frame)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), may_alloc: true, ..Effects::NONE },
            // A callee-argument regexp literal reads its callee register, and its shared object's
            // `lastIndex` and the proofs it is kept under (`Vm::regexp_literal_argument_of`).
            Self::Allocate { allocation: Allocation::RegExp { callee: Some(_), .. }, .. } => Effects {
                reads: RegionSet::of(Region::Frame).with(Region::ObjectSlots).with(Region::ShapeWord).with(Region::Records),
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), may_alloc: true, ..Effects::NONE },
            Self::Allocate { allocation: Allocation::Empty | Allocation::Array { .. } | Allocation::RegExp { callee: None, .. } | Allocation::ConstArray { .. }, .. } => Effects {
                writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), may_alloc: true, ..Effects::NONE },
            Self::Allocate { allocation: Allocation::ObjectLiteral { .. } | Allocation::ObjectPair { .. } | Allocation::ArrayOf { .. } | Allocation::Template { .. }, .. } => Effects {
                reads: RegionSet::of(Region::Frame), writes: RegionSet::of(Region::HeapMeta).with(Region::ObjectSlots)
                .with(Region::Records).with(Region::ShapeWord).with(Region::Payload), may_alloc: true, ..Effects::NONE },
            _ => return self.property_effects(),
        })
    }

    /// Effects of the property operations: a plan's reads and stores touch object slots and
    /// shapes; a host callout may run guest code (a getter, a proxy trap) or reshape objects.
    const fn property_effects(&self) -> Option<Effects> {
        Some(match self {
            Self::GetField { .. } => Effects { reads: RegionSet::of(Region::ObjectSlots).with(Region::ShapeWord)
                .with(Region::Records).with(Region::Payload), may_call_js: true, may_throw: true, ..Effects::NONE },
            Self::SetField { .. } | Self::StoreField { .. } | Self::FieldAdd { .. } | Self::Delete { .. } => Effects::ANY,
            Self::FieldGetHost { .. } | Self::IndexGetHost { .. } => Effects::ANY,
            _ => return None,
        })
    }

    const fn control_effects(&self) -> Effects {
        match self {
            Self::Move { .. } | Self::LoadConst { .. } | Self::LoadThis { .. } => Effects::NONE,
            Self::LoadLocalPlain { .. } => Effects::reads(Region::Frame),
            Self::StoreLocalPlain { .. } => Effects::writes(Region::Frame),
            Self::LoadLocalChecked { .. } => Effects::reads(Region::Frame),
            Self::StoreLocalChecked { .. } | Self::InitializeLocal { .. } => Effects::writes(Region::Frame),
            Self::Jump | Self::JumpIf { .. } | Self::LogicalNot { .. } | Self::Number(_) | Self::Return { .. } => Effects::NONE,
            Self::CompareJump { comparison, .. } if !comparison.runs_guest_code() => Effects::NONE,
            Self::RequireObjectCoercible { .. } | Self::RequireIteratorResult { .. } => Effects { may_throw: true, ..Effects::NONE },
            Self::Safepoint { .. } => Effects { may_collect: true, ..Effects::NONE },
            Self::Call(operation) => operation.effects(),
            // A suspension resumes the activation after anything: an `await` may run guest code
            // through PromiseResolve, and the frame's state is the generator's again.
            #[cfg(not(stencil_build))]
            Self::Suspend { .. } => Effects::ANY,
            // The memory operations answered in `memory_effects`; anything else is conservative.
            _ => Effects::ANY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::Operand;

    #[test]
    fn only_operations_that_store_allocate_or_run_guest_code_move_arrays() {
        type Row = Operation<u64, ()>;
        let stay: [Row; 5] = [Operation::ElementRead { base: 0, key: Operand(1), destination: 0 }, Operation::Move { destination: 0, source: 1 },
            Operation::Jump, Operation::Safepoint { proof: () }, Operation::LoadThis { destination: 0 }];
        assert!(stay.iter().all(|row| !row.moves_arrays()));
        let move_arrays: [Row; 4] = [Operation::ElementWrite { value: 0, base: 1, key: 2, strict: false }, Operation::IndexWrite { index: 0, value: 0, proof: () },
            Operation::Allocate { destination: 0, allocation: Allocation::Empty, proof: () }, Operation::Binary { destination: 0, left: crate::bytecode::Operand::register(1), right: crate::bytecode::Operand::register(2), operator: 0 }];
        assert!(move_arrays.iter().all(Row::moves_arrays));
    }
}
