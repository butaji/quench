//! The effects of a `CallOperation` and an `Operation`, as one row each.

use super::*;

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
    pub const fn effects(&self) -> Effects {
        match self.memory_effects() {
            Some(effects) => effects,
            None => self.control_effects(),
        }
    }

    /// Effects of the field, element and allocation operations.
    const fn memory_effects(&self) -> Option<Effects> {
        Some(match self {
            Self::IndexWrite { .. } => Effects { elements: ElementWrites::Any, ..Effects::writes(Region::Elements) },
            Self::ElementRead { .. } => Effects { reads: RegionSet::of(Region::Elements), may_call_js: true, may_throw: true, ..Effects::NONE },
            Self::ElementWrite { .. } => Effects::ANY,
            Self::DefineElement { .. } => Effects { elements: ElementWrites::Any, ..Effects::writes(Region::Elements) },
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
            Self::Jump | Self::JumpIf { .. } | Self::LogicalNot { .. } | Self::Return { .. } => Effects::NONE,
            // A Number operation's miss (an operand that is not a Number: `valueOf`, `ToNumeric`) runs guest code and
            // throws; a site whose operands the facts prove are Numbers grades it to none (`signatures::facts_grade`).
            Self::Number(_) => Effects { may_call_js: true, may_throw: true, ..Effects::NONE },
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
