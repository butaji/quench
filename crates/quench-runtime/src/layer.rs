//! VM DSL: Native | Fast | Dynamic, and Arena + GC.
//!
//! Two independent facts. Layer is how much is known. Storage is where the
//! payload lives. The store uses both: Native i32 is Arena; Native structref
//! is GC. QuickJS (`dynamic::Runtime`) is the JS layer on top, not Storage.

use crate::facts::{Certainty, Fact};

/// Store storage. Arena and GC both exist; neither is "the" layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Storage {
    /// Linear memory, tables, bytecode, unboxed locals. Reset as a region.
    Arena,
    /// Store GC heap: structs, arrays, exns.
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
        match fact.certainty() {
            Certainty::Proven => Self::Native,
            Certainty::Guarded => Self::Fast,
            Certainty::Unknown => Self::Dynamic,
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
    fn layer_and_storage_are_independent() {
        assert_ne!(super::Storage::Arena, super::Storage::Gc);
        assert_eq!(Layer::join(Layer::Native, Layer::Fast), Layer::Fast);
        assert_eq!(Layer::join(Layer::Fast, Layer::Dynamic), Layer::Dynamic);
    }
}
