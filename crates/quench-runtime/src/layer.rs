//! One ladder: Native, Fast, Dynamic.
//!
//! A layer is a property of a representation and of an operation, not of a
//! language. Frontends only choose the default; a guard or a box is the only
//! crossing. There is no per-language object type.

use crate::facts::Fact;

/// How a payload is stored. Arena is bump/reset; Gc is QuickJS RC + cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Storage {
    /// Native linear memory, instance GC heap, bytecode. Freed as a region.
    Arena,
    /// Dynamic objects. QuickJS refcount plus a cycle pass. No explicit roots.
    Gc,
}

/// Canonical execution layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Layer {
    /// No remaining uncertainty: unboxed scalars, `v128`, direct calls.
    Native,
    /// Specialised but still guarded: known shape, slot, or number kind.
    Fast,
    /// Meaning resolved at run time.
    Dynamic,
}

/// Why a Fast value is allowed to exist.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum GuardKind {
    I32,
    Number,
}

impl Layer {
    /// Facts `Proven` / `Guarded` / `Unknown` are this ladder, not a second one.
    pub fn of_fact<T>(fact: &Fact<T>) -> Self {
        match fact {
            Fact::Proven(_) => Self::Native,
            Fact::Guarded { .. } => Self::Fast,
            Fact::Unknown => Self::Dynamic,
        }
    }

    /// Join for a mixed program: Native ⊂ Fast ⊂ Dynamic.
    pub fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Dynamic, _) | (_, Self::Dynamic) => Self::Dynamic,
            (Self::Fast, _) | (_, Self::Fast) => Self::Fast,
            _ => Self::Native,
        }
    }

    pub fn storage(self) -> Storage {
        match self {
            Self::Native => Storage::Arena,
            Self::Fast | Self::Dynamic => Storage::Gc,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Layer;
    use crate::facts::{Fact, Guard};

    #[test]
    fn fact_states_are_the_ladder() {
        assert_eq!(Layer::of_fact(&Fact::Proven(1)), Layer::Native);
        assert_eq!(
            Layer::of_fact(&Fact::Guarded {
                value: 1,
                guard: Guard::Number
            }),
            Layer::Fast
        );
        assert_eq!(Layer::of_fact::<i32>(&Fact::Unknown), Layer::Dynamic);
    }

    #[test]
    fn native_is_arena_dynamic_is_gc() {
        assert_eq!(Layer::Native.storage(), super::Storage::Arena);
        assert_eq!(Layer::Fast.storage(), super::Storage::Gc);
        assert_eq!(Layer::Dynamic.storage(), super::Storage::Gc);
    }
}
