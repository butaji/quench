//! The facts a shape inherits from its chain, which each shape derives once from its parent
//! when it is made, so a read tests a bit where it once walked the chain.
use super::*;

/// Facts a shape inherits from its chain: once a transition introduces one, every descendant
/// keeps it. Two bytes, so `Shape` keeps its size.
#[derive(Clone, Copy, Default)]
pub(super) struct ShapeFacts(u16);
impl ShapeFacts {
    /// A symbol key or a table accessor of the chain may be a GC root.
    const MAY_HAVE_GC_ROOTS: u16 = 1 << 0;
    /// Some property of the chain was a `SLOT_GETTER` accessor, so a raw slot read must check
    /// whether the slot holds a getter rather than a value. A pair cell's slot is told by its
    /// value instead (`Vm::is_accessor_pair`), under `ACCESSOR_PAIRS`, so a shape of data
    /// properties and pairs, as a logger object with a `level` pair, keeps reading raw.
    const SLOT_GETTERS: u16 = 1 << 1;
    /// Some property of the chain was made an accessor. Only a descriptor transition makes
    /// one, as an add carries data attributes, so a chain without this fact holds data
    /// properties alone and a read of any slot is its value.
    const ACCESSORS: u16 = 1 << 2;
    /// Some property of the chain was made an accessor with a setter, as every lazy data
    /// property is, rather than a getter alone: its functions in `Vm::shape_accessors`
    /// (`ShapeAttributes::table_accessors`) or a pair cell in its slot (`slot_accessor_pair`).
    const ACCESSOR_PAIRS: u16 = 1 << 3;
    /// Some property of the chain was deleted, or left a vacant slot when it was rebuilt: a
    /// second deletion makes the object its own dictionary (`DictionaryTrigger::DeletionPattern`).
    const DELETIONS: u16 = 1 << 4;
    /// A transition introduced non-default slot attributes. Without this fact every
    /// allocated slot has the default data descriptor, including vacated slots.
    const NON_DEFAULT_ATTRIBUTES: u16 = 1 << 5;
    /// The chain recorded a function's original name (`ShapeTransition::FunctionName`): its own
    /// `name` and `length`, where it still has them, are the creation-time properties unless
    /// one of the two facts below says a redefinition changed it.
    const FUNCTION_NAMED: u16 = 1 << 6;
    /// The function's own `name` was redefined (`ShapeTransition::FunctionNameReconfigured`).
    const NAME_RECONFIGURED: u16 = 1 << 7;
    /// The function's own `length` was redefined (`ShapeTransition::FunctionLengthReconfigured`).
    const LENGTH_RECONFIGURED: u16 = 1 << 8;

    pub(super) fn extended_by(self, transition: ShapeTransition) -> Self {
        let introduced = |fact: bool, bit: u16| if fact { bit } else { 0 };
        let descriptor = |test: fn(ShapeAttributes) -> bool| {
            matches!(transition, ShapeTransition::Descriptor { attributes, .. } if test(attributes))
        };
        Self(
            self.0
                | introduced(transition.introduces_gc_roots(), Self::MAY_HAVE_GC_ROOTS)
                | introduced(
                    matches!(transition, ShapeTransition::Add { attributes, .. } if attributes != DEFAULT_DATA_ATTRIBUTES)
                        || descriptor(|attributes| attributes != DEFAULT_SHAPE_ATTRIBUTES),
                    Self::NON_DEFAULT_ATTRIBUTES,
                )
                | introduced(descriptor(ShapeAttributes::slot_getter), Self::SLOT_GETTERS)
                | introduced(descriptor(|attributes| attributes.accessor), Self::ACCESSORS)
                | introduced(
                    descriptor(|attributes| attributes.table_accessors() || attributes.slot_accessor_pair()),
                    Self::ACCESSOR_PAIRS,
                )
                | introduced(matches!(transition, ShapeTransition::Delete { .. } | ShapeTransition::Vacant), Self::DELETIONS)
                | introduced(matches!(transition, ShapeTransition::FunctionName { .. }), Self::FUNCTION_NAMED)
                | introduced(matches!(transition, ShapeTransition::FunctionNameReconfigured), Self::NAME_RECONFIGURED)
                | introduced(matches!(transition, ShapeTransition::FunctionLengthReconfigured), Self::LENGTH_RECONFIGURED),
        )
    }

    pub(super) fn accessor_pairs(self) -> bool { self.0 & Self::ACCESSOR_PAIRS != 0 }
    pub(super) fn accessors(self) -> bool { self.0 & Self::ACCESSORS != 0 }
    pub(super) fn default_attributes(self) -> bool { self.0 & Self::NON_DEFAULT_ATTRIBUTES == 0 }
    pub(super) fn deletions(self) -> bool { self.0 & Self::DELETIONS != 0 }
    pub(super) fn may_have_gc_roots(self) -> bool { self.0 & Self::MAY_HAVE_GC_ROOTS != 0 }
    pub(super) fn slot_getters(self) -> bool { self.0 & Self::SLOT_GETTERS != 0 }
    pub(super) fn function_named(self) -> bool { self.0 & Self::FUNCTION_NAMED != 0 }
    pub(super) fn name_reconfigured(self) -> bool { self.0 & Self::NAME_RECONFIGURED != 0 }
    pub(super) fn length_reconfigured(self) -> bool { self.0 & Self::LENGTH_RECONFIGURED != 0 }
}
