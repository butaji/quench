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
